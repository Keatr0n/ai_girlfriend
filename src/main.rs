mod audio;
mod vad;
mod stt;
mod tts;

use std::num::NonZeroU32;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use llama_cpp_2::{
    context::params::LlamaContextParams, llama_backend::LlamaBackend, llama_batch::LlamaBatch,
    model::{LlamaModel, params, AddBos, Special}, sampling::LlamaSampler
};

use rand::RngCore;
use stt::Stt;

enum ListeningState {
    Listening,
    Thinking,
    None,
}

fn print_conversation(history: &[(String, String)], listening_state: ListeningState) {
    print!("\x1B[2J\x1B[1;1H"); // Clear screen and move cursor to top
    println!("=== Conversation ===\n");
    for (user, ai) in history {
        println!("You: {}\n", user);
        println!("AI: {}\n", ai);
    }

    match listening_state {
       ListeningState::Listening => println!("---\nListening..."),
       ListeningState::Thinking => println!("---\nThinking..."),
       ListeningState::None => println!("---"),
    }
}

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let system_prompt = std::env::var("SYSTEM_PROMPT")
        .expect("SYSTEM_PROMPT must be set in .env file");
    let whisper_model_path = std::env::var("WHISPER_MODEL_PATH")
        .expect("WHISPER_MODEL_PATH must be set in .env file");
    let llm_model_path = std::env::var("LLM_MODEL_PATH")
        .expect("LLM_MODEL_PATH must be set in .env file");
    let piper_model_path = std::env::var("PIPER_MODEL_PATH")
        .expect("PIPER_MODEL_PATH must be set in .env file");
    let llm_threads: i32 = std::env::var("LLM_THREADS")
        .expect("LLM_THREADS must be set in .env file")
        .parse()
        .expect("LLM_THREADS must be a valid number");
    let llm_context_size: u32 = std::env::var("LLM_CONTEXT_SIZE")
        .expect("LLM_CONTEXT_SIZE must be set in .env file")
        .parse()
        .expect("LLM_CONTEXT_SIZE must be a valid number");

    let (audio, stream, source_rate) = audio::start_mic();
    let stt = Stt::new(&whisper_model_path)?;

    println!("STT online");

    let mut backend = LlamaBackend::init()?;
    backend.void_logs();

    let model = LlamaModel::load_from_file(
        &backend,
        &llm_model_path,
        &params::LlamaModelParams::default()
    )?;

    println!("LLM loaded");

    let context_params = LlamaContextParams::default()
        .with_n_threads(llm_threads)
        .with_n_ctx(NonZeroU32::new(llm_context_size));

    let mut ctx = model.new_context(&backend, context_params)?;

    // Initialize system prompt once
    let system_tokens = model.str_to_token(&system_prompt, AddBos::Always).unwrap();
    let mut batch = LlamaBatch::new(512, 1);

    for (i, token) in system_tokens.iter().enumerate() {
        let is_last = i == system_tokens.len() - 1;
        batch.add(*token, i as i32, &[0], is_last).unwrap();
    }

    println!("LLM context initializing...");

    ctx.decode(&mut batch).unwrap();
    let mut n_past = system_tokens.len() as i32;

    let muted = Arc::new(AtomicBool::new(false));
    let muted_clone = muted.clone();

    let mut conversation_history: Vec<(String, String)> = Vec::new();

    print_conversation(&conversation_history, ListeningState::Listening);
    vad::run_vad(audio, source_rate, muted, |utterance| {
        if let Ok(text) = stt.transcribe(&utterance) {
            if text.trim().is_empty() || text.trim() == "[BLANK_AUDIO]" { return; }

            muted_clone.store(true, Ordering::Relaxed);
            print_conversation(&conversation_history, ListeningState::Thinking);

            // Add user message to context
            let user_tokens = model.str_to_token(&format!("\n### User: {}\n### Assistant:", text), AddBos::Never).unwrap();
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

            loop {
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(token);

                if model.is_eog_token(token) { break; }

                if let Ok(s) = model.token_to_str(token, Special::Tokenize) {
                    // Stop if the model starts generating the next turn
                    if reply.contains("User:") || reply.contains("###") {
                        break;
                    }
                    reply.push_str(&s);
                }

                batch.clear();
                batch.add(token, n_past, &[0], true).unwrap();
                ctx.decode(&mut batch).unwrap();
                n_past += 1;
            }

            // Clean up any formatting that leaked through
            let cleaned_reply = reply
                .split("User:")
                .next()
                .unwrap_or(&reply)
                .split("###")
                .next()
                .unwrap_or(&reply)
                .trim()
                .to_string();

            conversation_history.push((text.clone(), cleaned_reply.clone()));
            print_conversation(&conversation_history, ListeningState::None);

            tts::speak(&cleaned_reply, &piper_model_path).unwrap();
            muted_clone.store(false, Ordering::Relaxed);
            print_conversation(&conversation_history, ListeningState::Listening);
        }
    });

    #[allow(unused_must_use)]
    std::mem::ManuallyDrop::into_inner(stream);
    Ok(())
}
