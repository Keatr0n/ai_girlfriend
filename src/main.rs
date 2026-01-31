mod audio;
mod input;
mod ui;
mod vad;
mod stt;
mod tts;
mod llm;

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::fs;
use std::io::Write;

use llama_cpp_2::{llama_batch::LlamaBatch, model::LlamaChatMessage};
use stt::Stt;

use crate::llm::Llm;
use crate::ui::ListeningState;

const CONVERSATION_FILE: &str = "conversation_history.txt";

fn load_previous_summary() -> Option<String> {
    if let Ok(content) = fs::read_to_string(CONVERSATION_FILE)
        && !content.is_empty() {
            return Some(format!("\n\nPrevious conversation summary:\n{}\n", content));
        }
    None
}

fn save_conversation(history: &[(String, String)], llm: &mut Llm) -> Result<(), anyhow::Error> {
    let existing_data = fs::read_to_string(CONVERSATION_FILE).unwrap_or_default();
    ui::status_remembering();

    let mut file = fs::File::create(CONVERSATION_FILE)?;
    let mut llm_input = vec![LlamaChatMessage::new("system".into(), "You summarize conversations into their key facts".into())?];
    for (user, ai) in history {
         llm_input.push(LlamaChatMessage::new("user".into(), user.into())?);
         llm_input.push(LlamaChatMessage::new("assistant".into(), ai.into())?);
    }
    llm_input.push(LlamaChatMessage::new("user".into(), String::from("
    Summarize this conversation into a brief context block for future sessions. Include:
    1. Key facts about the user (background, preferences)
    2. Ongoing discussions and topics of conversation

    Format as concise bullet points suitable for a system prompt. Focus on actionable context, not play-by-play.
        "))?);

    let reply = llm.run_inference_once(&llm_input, LlamaBatch::new(65536, 1));
    let mut memories = format!("{}\n{}", existing_data, reply);

    if memories.len() > 2000 {
        ui::status_pruning();
        llm_input.clear();
        llm_input.push(LlamaChatMessage::new("system".into(), "You summarize dot-point lists into only their most important items".into())?);
        llm_input.push(LlamaChatMessage::new("user".into(), format!("Reduce this context list by merging related items and removing outdated or low-value information. Keep only what's still relevant and useful for future conversations.\n{}",memories))?);

        memories = llm.run_inference_once(&llm_input, LlamaBatch::new(4096, 1));
    }

    ui::status_goodbye();

    write!(file, "{}", memories)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let mut system_prompt = std::env::var("SYSTEM_PROMPT")
        .expect("SYSTEM_PROMPT must be set in .env file");

    if let Some(summary) = load_previous_summary() {
        system_prompt.push_str(&summary);
    }

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

    ui::status_stt_online();

    #[allow(clippy::arc_with_non_send_sync)]
    let llm = Arc::new(Mutex::new(Llm::new(llm_model_path, llm_threads, llm_context_size, system_prompt)?));

    ui::status_llm_loaded();

    let muted = Arc::new(AtomicBool::new(false));
    let muted_clone = muted.clone();

    let shutdown = Arc::new(AtomicBool::new(false));


    let conversation_history = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let conversation_history_edit = conversation_history.clone();
    let conversation_history_edit_for_input = conversation_history.clone();
    let llm_edit = llm.clone();

    let input_handle = input::spawn_input_thread(conversation_history_edit_for_input);
    let input_rx = input_handle.rx;

    let on_text = |text: String, interrupt_rx: &std::sync::mpsc::Receiver<input::InputEvent>| {
        conversation_history.lock().unwrap().push((text.clone(), "".into()));
        muted_clone.store(true, Ordering::Relaxed);
        ui::print_conversation(&conversation_history.lock().unwrap(), ListeningState::Thinking);

        let reply = match llm.lock().unwrap().run_inference(&text, interrupt_rx) {
            Some(r) => r,
            None => {
                conversation_history.lock().unwrap().pop();
                // Inference was interrupted
                ui::print_conversation(&conversation_history.lock().unwrap(), ListeningState::Listening);
                muted_clone.store(false, Ordering::Relaxed);
                return;
            }
        };
        conversation_history.lock().unwrap().pop();
        conversation_history.lock().unwrap().push((text.clone(), reply.clone()));
        ui::print_conversation(&conversation_history.lock().unwrap(), ListeningState::None);

        tts::speak(&reply, &piper_model_path).unwrap();

        // make it seem a little more natural
        std::thread::sleep(std::time::Duration::from_millis(1000));
        muted_clone.store(false, Ordering::Relaxed);
        ui::print_conversation(&conversation_history.lock().unwrap(), ListeningState::Listening);
    };

    ui::print_conversation(&conversation_history.lock().unwrap(), ListeningState::Listening);
    vad::run_vad(audio, source_rate, muted, shutdown.clone(), input_rx, |utterance, interrupt_rx| {
        if let Ok(text) = stt.transcribe(&utterance) {
            if text.trim().is_empty() || text.trim() == "[BLANK_AUDIO]" { return; }

            on_text(text, interrupt_rx);
        }
    }, |value, interrupt_rx| {
        if llm_edit.lock().unwrap().rollback_exchange() {
            conversation_history_edit.lock().unwrap().pop();
        }

        on_text(value, interrupt_rx);
    });

    if shutdown.load(Ordering::Relaxed)
        && let Ok(history) = conversation_history.lock() {
            let _ = save_conversation(&history, &mut llm.lock().unwrap());
        }

    ui::restore_cursor();
    #[allow(unused_must_use)]
    std::mem::ManuallyDrop::into_inner(stream);
    Ok(())
}
