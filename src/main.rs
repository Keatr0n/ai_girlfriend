mod audio;
mod vad;
mod stt;
mod tts;
mod llm;

use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use std::fs;
use std::io::Write;

use llama_cpp_2::model::LlamaChatMessage;
use stt::Stt;

use crate::llm::Llm;

enum ListeningState {
    Listening,
    Thinking,
    None,
}

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
    print!("\x1B[2J\x1B[1;1H");
    print!("Remembering conversation...");

    let mut file = fs::File::create(CONVERSATION_FILE)?;
    let mut llm_input = vec![LlamaChatMessage::new("system".into(), "You summarize conversations into their key facts".into())?];
    for (user, ai) in history {
         llm_input.push(LlamaChatMessage::new("user".into(), user.into())?);
         llm_input.push(LlamaChatMessage::new("assistant".into(), ai.into())?);
    }
    llm_input.push(LlamaChatMessage::new("user".into(), String::from("
    Summarize this conversation into a brief context block for future sessions. Include:
    1. Key facts about the user (background, preferences)
    2. Decisions made or conclusions reached
    3. Ongoing tasks or issues to remember

    Format as concise bullet points suitable for a system prompt. Focus on actionable context, not play-by-play.
        "))?);

    let reply = llm.run_inference_once(&llm_input);
    let mut memories = format!("{}\n{}", existing_data, reply);

    if memories.len() > 2000 {
        print!("\x1B[2J\x1B[1;1H");
        print!("Pruning memories...");
        llm_input.clear();
        llm_input.push(LlamaChatMessage::new("system".into(), "You summarize dot-point lists into only their most important items".into())?);
        llm_input.push(LlamaChatMessage::new("user".into(), format!("Reduce this context list by merging related items and removing outdated or low-value information. Keep only what's still relevant and useful for future conversations.\n{}",memories))?);

        memories = llm.run_inference_once(&llm_input);
    }

    write!(file, "{}", memories)?;
    Ok(())
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

    println!("STT online");

   let mut llm = Llm::new(llm_model_path, llm_threads, llm_context_size, system_prompt)?;

    println!("LLM loaded");

    let muted = Arc::new(AtomicBool::new(false));
    let muted_clone = muted.clone();

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let conversation_history = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

    ctrlc::set_handler(move || {
        shutdown_clone.store(true, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    print_conversation(&conversation_history.lock().unwrap(), ListeningState::Listening);
    vad::run_vad(audio, source_rate, muted, shutdown.clone(), |utterance| {
        if let Ok(text) = stt.transcribe(&utterance) {
            if text.trim().is_empty() || text.trim() == "[BLANK_AUDIO]" { return; }

            muted_clone.store(true, Ordering::Relaxed);
            print_conversation(&conversation_history.lock().unwrap(), ListeningState::Thinking);

            let reply = llm.run_inference(&text);

            conversation_history.lock().unwrap().push((text.clone(), reply.clone()));
            print_conversation(&conversation_history.lock().unwrap(), ListeningState::None);

            tts::speak(&reply, &piper_model_path).unwrap();

            // make it seem a little more natural
            std::thread::sleep(std::time::Duration::from_millis(1000));
            muted_clone.store(false, Ordering::Relaxed);
            print_conversation(&conversation_history.lock().unwrap(), ListeningState::Listening);
        }
    });

    if shutdown.load(Ordering::Relaxed)
        && let Ok(history) = conversation_history.lock() {
            let _ = save_conversation(&history, &mut llm);
        }

    #[allow(unused_must_use)]
    std::mem::ManuallyDrop::into_inner(stream);
    Ok(())
}
