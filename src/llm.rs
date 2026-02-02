use std::thread::JoinHandle;
use std::{num::NonZeroU32, thread};

use llama_cpp_2::{context::{params::LlamaContextParams}, llama_backend::LlamaBackend, llama_batch::LlamaBatch, model::{AddBos, LlamaChatMessage, LlamaModel, Special, params}, sampling::LlamaSampler};

use crate::state::{LifeCycleState, LlmCommand, LlmState};
use crate::{state::StateHandle};
use crate::ui;
use rand::RngCore;

pub struct LlmHandle {
    _handle: JoinHandle<()>,
}

pub fn spawn_llm_thread(state: StateHandle, path: String, llm_threads: i32, llm_context_size: u32, system_prompt: String) -> LlmHandle {
    let handle = thread::spawn(move || {
        let _ = run_llm_loop(state, path, llm_threads, llm_context_size, system_prompt);
    });

    LlmHandle { _handle: handle }
}

fn run_llm_loop(state: StateHandle, path: String, llm_threads: i32, llm_context_size: u32, system_prompt: String) -> anyhow::Result<()> {
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

    let mut ctx = model.new_context(&backend, context_params)?;

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

    let mut n_past = system_tokens.len() as i32;
    let mut exchange_checkpoints: Vec<i32> = vec![];

    state.update(|s|{
        s.llm_state = LlmState::AwaitingInput;
        s.system_mute = false;
        s.life_cycle_state = LifeCycleState::Running;
    });

    while state.subscribe().recv().is_ok() {
        let current_state = state.read();

        if current_state.llm_state != LlmState::AwaitingInput {
            continue;
        }

        let messages: Vec<LlamaChatMessage> = if let Some(command) = current_state.llm_command {
            match command {
                crate::state::LlmCommand::CancelInference => continue,
                crate::state::LlmCommand::ContinueConversation(message) => vec![LlamaChatMessage::new("user".into(), message).unwrap()],
                crate::state::LlmCommand::DestroyContextAndRunFromNothing(llama_chat_messages) => {
                    ctx.clear_kv_cache();
                    llama_chat_messages.iter().map(|(role, message)| {
                        LlamaChatMessage::new(role.into(), message.into()).unwrap()
                    }).collect()
                },
                crate::state::LlmCommand::EditLastMessage(message) => {
                    if let Some(checkpoint) = exchange_checkpoints.pop() {
                        ctx.clear_kv_cache_seq(Some(0), Some((checkpoint - 1) as u32), Some(ctx.kv_cache_seq_pos_max(0) as u32)).unwrap_or(false);
                    }

                    vec![LlamaChatMessage::new("user".into(), message).unwrap()]
                },
            }
        } else {
            continue;
        };

        state.update(|s|{
            s.llm_state = LlmState::RunningInference;
            s.llm_command = None;
        });

        let n_past_before = n_past;
        exchange_checkpoints.push(n_past_before);

        let chat_message = model.apply_chat_template(&_chat_template, messages.as_slice(), true).unwrap();
        let user_tokens = model.str_to_token(&chat_message, AddBos::Never).unwrap();
        batch.clear();

        for (i, token) in user_tokens.iter().enumerate() {
            let is_last = i == user_tokens.len() - 1;
            batch.add(*token, n_past + i as i32, &[0], is_last).unwrap();
        }

        ctx.decode(&mut batch).unwrap();
        n_past += user_tokens.len() as i32;

        let mut rng = rand::rng();

        let mut reply = String::new();
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::dist(rng.next_u32()),
            LlamaSampler::greedy(),
        ]);

        let mut interrupted = false;

        loop {
            // Check for interrupt event
            if state.read().llm_command == Some(LlmCommand::CancelInference) {
                interrupted = true;
                break;
            }

            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if model.is_eog_token(token) { break; }

            if let Ok(s) = model.token_to_str(token, Special::Tokenize) {
                reply.push_str(&s);
            }

            batch.clear();
            batch.add(token, n_past, &[0], true).unwrap();
            ctx.decode(&mut batch).unwrap();
            n_past += 1;
        }

        if interrupted {
            // Roll back KV cache to state before this inference
            let _ = ctx.clear_kv_cache_seq(None, Some((n_past_before -1) as u32), Some(ctx.kv_cache_seq_pos_max(0) as u32));
            exchange_checkpoints.pop();
            continue;
        }

        state.update(|s|{
            if let Some((user, _)) = s.conversation.pop() {
                s.conversation.push((user, reply.clone()));

                if s.life_cycle_state != LifeCycleState::ShuttingDown {
                    s.llm_state = LlmState::RunningTts;
                    s.tts_command = Some(reply);
                } else {
                    s.llm_state = LlmState::AwaitingInput;
                }
            }
        });
    }

    Ok(())
}
