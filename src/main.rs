mod assistant;
mod audio;
mod input;
mod shutdown;
mod state;
mod ui;
mod vad;
mod stt;
mod tts;
mod llm;

use std::sync::Arc;
use std::fs;

use stt::Stt;

use crate::state::{LifeCycleState, LlmCommand, StateHandle};

fn load_previous_summary(conversation_file: &str) -> Option<String> {
    if let Ok(content) = fs::read_to_string(conversation_file)
        && !content.is_empty() {
            return Some(format!("\n\nPrevious conversation summary:\n{}\n", content));
        }
    None
}

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Load assistant config and select
    let config = assistant::load_config()?;
    let selected = assistant::select_assistant(&config)?;
    let conversation_file = selected.conversation_file();

    let mut system_prompt = format!("Your name is {}. {}", selected.name.clone(), selected.system_prompt.clone());

    if let Some(summary) = load_previous_summary(&conversation_file) {
        system_prompt.push_str(&summary);
    }

    let whisper_model_path = std::env::var("WHISPER_MODEL_PATH")
        .expect("WHISPER_MODEL_PATH must be set in .env file");
    let llm_model_path = selected.llm_model_path.clone()
        .unwrap_or_else(|| std::env::var("LLM_MODEL_PATH")
            .expect("LLM_MODEL_PATH must be set in .env file or assistant config"));
    let piper_model_path = selected.piper_model_path.clone()
        .unwrap_or_else(|| std::env::var("PIPER_MODEL_PATH")
        .expect("PIPER_MODEL_PATH must be set in .env file or assistant config"));
    let llm_threads: i32 = std::env::var("LLM_THREADS")
        .expect("LLM_THREADS must be set in .env file")
        .parse()
        .expect("LLM_THREADS must be a valid number");
    let llm_context_size: u32 = std::env::var("LLM_CONTEXT_SIZE")
        .expect("LLM_CONTEXT_SIZE must be set in .env file")
        .parse()
        .expect("LLM_CONTEXT_SIZE must be a valid number");


    let stt = Stt::new(&whisper_model_path)?;

    ui::status_stt_online();

    #[allow(clippy::arc_with_non_send_sync)]

    ui::status_llm_loaded();

    // Initialize global state
    let state = StateHandle::new();
    let state_for_audio = state.clone();
    let state_for_input = state.clone();
    let state_for_ui = state.clone();
    let state_for_llm = state.clone();
    let state_for_tts = state.clone();

    let conversation_file = Arc::new(conversation_file);
    let conversation_file_clone = conversation_file.clone();

    let _ = input::spawn_input_thread(state_for_input);
    let _ = ui::spawn_ui_thread(state_for_ui);
    let _ = llm::spawn_llm_thread(state_for_llm, llm_model_path, llm_threads, llm_context_size, system_prompt);
    let _ = tts::spawn_tts_thread(state_for_tts, piper_model_path);

    let (audio, stream, source_rate) = audio::start_mic(state_for_audio);

    vad::run_vad(audio, source_rate, |utterance| {
        if let Ok(text) = stt.transcribe(&utterance) {
            if text.trim().is_empty() || text.trim() == "[BLANK_AUDIO]" { return; }

            state.update(|s| {
                s.system_mute = true;
                s.conversation.push((text.trim().into(), "".into()));
                s.llm_command = Some(LlmCommand::ContinueConversation(text.trim().into()));
            });
        }
    });

    if state.read().life_cycle_state == LifeCycleState::ShuttingDown {
        let _ = shutdown::save_conversation(&state, &conversation_file_clone);
    }

    ui::restore_cursor();
    #[allow(unused_must_use)]
    std::mem::ManuallyDrop::into_inner(stream);
    Ok(())
}
