mod audio;
mod config;
mod input;
mod shutdown;
mod state;
mod stt;
mod tts;
// llm needs to be below stt
mod llm;
mod tools;
mod ui;
mod vad;

use std::{fs, time::Instant};

use stt::Stt;

use crate::{
    shutdown::save_conversation,
    state::{LlmCommand, LlmState, StateHandle},
};

fn load_previous_summary(conversation_file: &str) -> Option<String> {
    if let Ok(content) = fs::read_to_string(conversation_file)
        && !content.is_empty()
    {
        return Some(format!("\n\nPrevious conversation summary:\n{}\n", content));
    }
    None
}

fn main() -> anyhow::Result<()> {
    // Load assistant config and select
    let config = config::load_config()?;
    let selected = config::select_assistant(&config)?;
    let conversation_file = selected.conversation_file();

    let mut system_prompt = format!(
        "Your name is {}. {}. Here is a summary of previous interactions with the user: ",
        selected.name.clone(),
        selected.system_prompt.clone()
    );

    if let Some(summary) = load_previous_summary(&conversation_file) {
        system_prompt.push_str(&summary);
    }

    let whisper_model_path = config.global.whisper_model_path;
    let llm_model_path = selected.llm_model_path.clone().unwrap_or_else(|| {
        config
            .global
            .default_llm_model_path
            .expect("default_llm_model_path or llm_model_path must be set in assistant config")
    });
    let piper_model_path = selected.piper_model_path.clone().unwrap_or_else(|| {
        config
            .global
            .default_piper_model_path
            .expect("default_piper_model_path or piper_model_path must be set in assistant config")
    });
    let llm_threads: i32 = config.global.llm_threads;
    let llm_context_size: u32 = config.global.llm_context_size;

    let stt = Stt::new(&whisper_model_path)?;

    ui::status_stt_online();

    #[allow(clippy::arc_with_non_send_sync)]
    // Initialize global state
    let state = StateHandle::new();
    let state_for_audio = state.clone();
    let state_for_input = state.clone();
    let state_for_ui = state.clone();
    let state_for_llm = state.clone();
    let state_for_tts = state.clone();
    let state_for_vad = state.clone();

    let _ = input::spawn_input_thread(state_for_input);
    let _ = ui::spawn_ui_thread(state_for_ui, selected.name.clone());
    let _ = llm::spawn_llm_thread(
        state_for_llm,
        llm_model_path,
        llm_threads,
        llm_context_size,
        config.global.enable_word_by_word_response,
        system_prompt,
        config.global.tool_path,
    );
    let _ = tts::spawn_tts_thread(state_for_tts, piper_model_path);

    let (audio, stream, source_rate) = audio::start_mic(state_for_audio);

    vad::run_vad(state_for_vad, audio, source_rate, |utterance| {
        if let Ok(text) = stt.transcribe(&utterance) {
            if text.trim().is_empty() || text.trim() == "[BLANK_AUDIO]" {
                return;
            }

            let current_state = state.read();

            if current_state.is_only_responding_after_name
                && match current_state.time_since_name_was_said {
                    None => true,
                    Some(instant) => instant.elapsed().as_secs() > 5,
                }
            {
                let words = text.split(" ");

                if words.clone().count() <= 3
                    && words.clone().any(|word| {
                        selected.name.eq_ignore_ascii_case(
                            &word
                                .chars()
                                .filter(|c| c.is_alphabetic())
                                .collect::<String>(),
                        )
                    })
                {
                    state.update(|s| {
                        s.time_since_name_was_said = Some(Instant::now());
                        s.tts_commands.push("Yes?".into());
                    });

                    return;
                }

                if !words.take(5).any(|word| {
                    selected.name.eq_ignore_ascii_case(
                        &word
                            .chars()
                            .filter(|c| c.is_alphabetic())
                            .collect::<String>(),
                    )
                }) {
                    return;
                }
            }

            state.update(|s| {
                s.time_since_name_was_said = None;
                s.system_mute = true;
                s.conversation.push((text.trim().into(), "".into()));
                s.llm_state = LlmState::RunningInference;
                s.llm_command = Some(LlmCommand::ContinueConversation(text.trim().into()));
            });
        }
    });

    save_conversation(state, &conversation_file)?;

    ui::restore_cursor();

    #[allow(unused_must_use)]
    std::mem::ManuallyDrop::into_inner(stream);
    Ok(())
}
