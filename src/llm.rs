use std::num::NonZeroU32;
use std::sync::mpsc::Receiver;

use llama_cpp_2::{context::{LlamaContext, params::LlamaContextParams}, llama_backend::LlamaBackend, llama_batch::LlamaBatch, model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel, Special, params}, sampling::LlamaSampler};

use crate::input::InputEvent;
use crate::ui;
use rand::RngCore;

pub struct Llm {
    _backend: Box<LlamaBackend>,
    _model: Box<LlamaModel>,
    _chat_template: LlamaChatTemplate,
    ctx: LlamaContext<'static>,
    n_past: i32,
    batch: LlamaBatch<'static>,
    exchange_checkpoints: Vec<i32>, // n_past values before each exchange
}

impl Llm {
    pub fn new(path: String, llm_threads: i32, llm_context_size: u32, system_prompt: String) -> anyhow::Result<Self> {
        let mut backend = Box::new(LlamaBackend::init()?);
        backend.void_logs();

        ui::status_llm_loaded();

        let model = Box::new(LlamaModel::load_from_file(
            &backend,
            &path,
            &params::LlamaModelParams::default()
        )?);

        let context_params = LlamaContextParams::default()
            .with_n_threads(llm_threads)
            .with_n_ctx(NonZeroU32::new(llm_context_size));

        // THIS WAS CLAUDES IDEA, I'M NOT A HUGE FAN, BUT IT SEEMS TO WORK AND I DON'T KNOW ENOUGH TO AVOID THIS
        // SAFETY: We're using unsafe to extend the lifetime of references to backend and model.
        // This is safe because we're storing backend and model in the same struct as ctx,
        // and the struct fields are dropped in reverse order of declaration, ensuring
        // ctx is dropped before model and backend.
        let mut ctx = unsafe {
            let model_ref: &LlamaModel = std::mem::transmute(&*model);
            let backend_ref: &LlamaBackend = std::mem::transmute(&*backend);
            model_ref.new_context(backend_ref, context_params)?
        };

        let _chat_template = model.chat_template(None).unwrap();

        let formatted_system_prompt = model.apply_chat_template(&_chat_template, &[LlamaChatMessage::new("system".into(), system_prompt).unwrap()], false).unwrap();

        let system_tokens = model.str_to_token(&formatted_system_prompt, AddBos::Always).unwrap();
        let mut batch = LlamaBatch::new(4096, 1);

        for (i, token) in system_tokens.iter().enumerate() {
            let is_last = i == system_tokens.len() - 1;
            batch.add(*token, i as i32, &[0], is_last).unwrap();
        }

        ui::status_llm_context_init();

        ctx.decode(&mut batch).unwrap();

        let n_past = system_tokens.len() as i32;

        Ok(Self { _backend: backend, _model: model, _chat_template, ctx, n_past, batch, exchange_checkpoints: Vec::new() })
    }

    // who needs error handling, am I right?
    // Returns None if interrupted by user
    pub fn run_inference(&mut self, input: &str, interrupt_rx: &Receiver<InputEvent>) -> Option<String> {
        let n_past_before = self.n_past;
        self.exchange_checkpoints.push(n_past_before);

        let chat_message = self._model.apply_chat_template(&self._chat_template, &[LlamaChatMessage::new("user".into(), input.into()).unwrap()], true).unwrap();
        let user_tokens = self._model.str_to_token(&chat_message, AddBos::Never).unwrap();
        self.batch.clear();

        for (i, token) in user_tokens.iter().enumerate() {
            let is_last = i == user_tokens.len() - 1;
            self.batch.add(*token, self.n_past + i as i32, &[0], is_last).unwrap();
        }

        self.ctx.decode(&mut self.batch).unwrap();
        self.n_past += user_tokens.len() as i32;

        let mut rng = rand::rng();

        let mut reply = String::new();
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::dist(rng.next_u32()),
            LlamaSampler::greedy(),
        ]);

        let mut interrupted = false;

        loop {
            // Check for interrupt event
            if let Ok(InputEvent::Interrupt) = interrupt_rx.try_recv() {
                interrupted = true;
                break;
            }

            let token = sampler.sample(&self.ctx, self.batch.n_tokens() - 1);
            sampler.accept(token);

            if self._model.is_eog_token(token) { break; }

            if let Ok(s) = self._model.token_to_str(token, Special::Tokenize) {
                reply.push_str(&s);
            }

            self.batch.clear();
            self.batch.add(token, self.n_past, &[0], true).unwrap();
            self.ctx.decode(&mut self.batch).unwrap();
            self.n_past += 1;
        }

        if interrupted {
            // Roll back KV cache to state before this inference
            let _ = self.ctx.clear_kv_cache_seq(None, Some((n_past_before -1) as u32), Some(self.ctx.kv_cache_seq_pos_max(0) as u32));
            self.exchange_checkpoints.pop();
            return None;
        }

        Some(reply)
    }

    /// Rolls back the last user/assistant exchange from the KV cache.
    /// Returns true if there was an exchange to roll back.
    pub fn rollback_exchange(&mut self) -> bool {
        if let Some(checkpoint) = self.exchange_checkpoints.pop() {
            self.ctx.clear_kv_cache_seq(Some(0), Some((checkpoint - 1) as u32), Some(self.ctx.kv_cache_seq_pos_max(0) as u32)).unwrap_or(false)
        } else {
            false
        }
    }

    // who needs error handling, am I right?
    pub fn run_inference_once(&mut self, messages: &[LlamaChatMessage]) -> String {
        let size = self.ctx.get_state_size();

        let mut buffer = vec![0u8; size];

        unsafe {
            self.ctx.copy_state_data(buffer.as_mut_ptr());
        }

        self.ctx.clear_kv_cache();

        let chat_message = self._model.apply_chat_template(&self._chat_template, messages, true).unwrap();
        let user_tokens = self._model.str_to_token(&chat_message, AddBos::Never).unwrap();

        self.batch.clear();

        for (i, token) in user_tokens.iter().enumerate() {
            let is_last = i == user_tokens.len() - 1;
            self.batch.add(*token, i as i32, &[0], is_last).unwrap();
        }

        self.ctx.decode(&mut self.batch).unwrap();

        let mut rng = rand::rng();

        let mut reply = String::new();
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::dist(rng.next_u32()),
            LlamaSampler::greedy(),
        ]);

        let mut n_past: i32 = user_tokens.len() as i32;

        loop {
            let token = sampler.sample(&self.ctx, self.batch.n_tokens() - 1);
            sampler.accept(token);

            if self._model.is_eog_token(token) { break; }

            if let Ok(s) = self._model.token_to_str(token, Special::Tokenize) {
                reply.push_str(&s);
            }

            self.batch.clear();
            self.batch.add(token, n_past, &[0], true).unwrap();
            self.ctx.decode(&mut self.batch).unwrap();
            n_past += 1;
        }

        unsafe {
            self.ctx.set_state_data(&buffer);
        }

        reply
    }
}
