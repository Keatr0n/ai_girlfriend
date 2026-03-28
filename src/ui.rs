use std::{
    io::{self, Write},
    thread::{self, JoinHandle},
};

use crossterm::terminal;
use regex::Regex;

use crate::state::{ConversationSnippet, LifeCycleState, LlmRole, LlmState, State, StateHandle};

pub fn run_ui_loop(state: StateHandle, model_name: String, enable_word_by_word_response: bool) {
    let re = Regex::new(r"(<think>[\s\S]*?<\/think>)*").ok();
    let mut previous_state = state.read();

    loop {
        let s = state.read();
        if s == previous_state
            || s.life_cycle_state == LifeCycleState::Initializing
            || (s.llm_state == LlmState::RunningInference && !enable_word_by_word_response)
        {
            continue;
        }

        previous_state = s.clone();

        if s.life_cycle_state == LifeCycleState::ShuttingDown {
            break;
        }

        let _ = print_conversation(s, &re, &model_name);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

pub struct UiHandle {
    _handle: JoinHandle<()>,
}

pub fn spawn_ui_thread(
    state: StateHandle,
    model_name: String,
    enable_word_by_word_response: bool,
) -> UiHandle {
    let handle = thread::spawn(move || {
        run_ui_loop(state, model_name, enable_word_by_word_response);
    });

    UiHandle { _handle: handle }
}

/// Clears the screen and moves cursor to top
fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
}

/// Hides the cursor
fn hide_cursor() {
    print!("\x1b[?25l");
}

/// Shows the cursor
fn show_cursor() {
    print!("\x1b[?25h");
}

/// Flushes stdout
pub fn flush() {
    let _ = io::stdout().flush();
}

// === Status Messages ===

fn status(msg: &str) {
    print!("{}\n\r", msg);
    flush();
}

pub fn status_llm_loaded() {
    status("LLM loaded");
}

pub fn status_llm_context_init() {
    status("\x1b[1A\r\x1b[2KLLM context initializing...");
}

pub fn status_stt_online() {
    status("STT online");
}

// === Saving/Memory Messages ===

pub fn status_remembering() {
    clear_screen();
    print!("Remembering conversation...");
    flush();
}

pub fn status_pruning() {
    clear_screen();
    print!("Pruning memories...");
    flush();
}

pub fn status_goodbye() {
    clear_screen();
    show_cursor();
    print!("See ya next time!");
    flush();
}

// === Conversation Display ===

fn print_conversation(state: State, re: &Option<Regex>, model_name: &String) -> anyhow::Result<()> {
    clear_screen();
    print!("=== Conversation ===\n\r");

    let history = state.conversation;

    for ConversationSnippet {
        message,
        role,
        is_tool_call,
    } in history
    {
        if is_tool_call {
            continue;
        }

        match role {
            LlmRole::Assistant => {
                if let Some(reg) = &re
                    && state.is_hiding_think_tags
                {
                    print!(
                        "{}: {}\n\r",
                        model_name,
                        reg.replace_all(&message.replace("\n", "\n\r"), "").trim()
                    );
                } else {
                    print!("{}: {}\n\r", model_name, message.replace("\n", "\n\r"));
                }
            }
            LlmRole::User => print!("\nYou: {}\n\n\r", message),
            _ => (),
        }
    }

    match state.llm_state {
        LlmState::RunningInference => print!("---\n\rThinking...\n\r"),
        LlmState::RunningTts | LlmState::InitializingTts => print!("---\n\r"),
        LlmState::AwaitingInput => {
            if state.user_mute {
                print!("---\n\r");
            } else if state.is_only_responding_after_name
                && match state.time_since_name_was_said {
                    None => true,
                    Some(instant) => instant.elapsed().as_secs() > 5,
                }
            {
                print!("---\n\rListening for {}...\r\n", model_name);
            } else {
                print!("---\n\rListening...\n\r");
            }
        }
    }

    if let Some((buffer, cursor_pos)) = state.text_input {
        if state.llm_state != LlmState::RunningInference {
            show_cursor();
        } else {
            hide_cursor();
        }

        let (width, _height) = terminal::size()?;

        if state.is_editing {
            print!("[Editing last message]\n\r");
        }

        print!("> {}", buffer);

        let prompt_len = 2;
        let buffer_len = buffer.chars().count() + prompt_len;
        let total_pos = prompt_len + cursor_pos;
        let term_width = width as usize;

        let num_lines = (buffer_len - (buffer_len % term_width)) / term_width;

        let mut line = num_lines - ((total_pos - (total_pos % term_width)) / term_width);

        // brute force
        if (buffer_len % term_width) == 0 && line != 0 {
            line -= 1;
        }

        let col = (total_pos % term_width) + 1;

        print!("\r\x1b[{}G", col);
        if line > 0 {
            print!("\x1b[{}A", line);
        } else if (total_pos % term_width) == 0 {
            print!("\x1b[1B");
        }
    } else {
        hide_cursor();
    }

    flush();
    Ok(())
}

// === Cleanup ===

pub fn restore_cursor() {
    show_cursor();
    flush();
}

// === Errors ===

pub fn error_stream(e: impl std::fmt::Debug) {
    println!("GOT STREAM ERROR {:?}", e);
}

#[allow(dead_code)]
pub fn debug_audio_captured() {
    println!("AUDIO CAPTURED");
}

// === Assistant Selection ===

pub fn assistant_selection_header() {
    clear_screen();
    print!("=== Select Assistant ===\n\r\n\r");
    flush();
}

pub fn assistant_option(index: usize, name: &str) {
    print!("  [{}] {}\n\r", index, name);
    flush();
}

pub fn assistant_prompt(max: usize) {
    show_cursor();
    print!("\n\rSelect (1-{}): ", max);
    flush();
}

pub fn assistant_selected(name: &str) {
    clear_screen();
    print!("Using assistant: {}\n\r", name);
    flush();
}

pub fn assistant_invalid_selection() {
    print!("\n\rInvalid selection.\n\rDefaulting to first.");
    flush();
}
