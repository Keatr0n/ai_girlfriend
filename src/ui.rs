use std::io::{self, Write};

pub enum ListeningState {
    Listening,
    Thinking,
    None,
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

pub fn status(msg: &str) {
    print!("{}\n\r", msg);
    flush();
}

pub fn status_llm_loaded() {
    status("LLM loaded");
}

pub fn status_llm_context_init() {
    status("LLM context initializing...");
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

pub fn print_conversation(history: &[(String, String)], state: ListeningState) {
    hide_cursor();
    clear_screen();
    print!("=== Conversation ===\n\r");

    for (user, ai) in history {
        print!("\nYou: {}\n\n\r", user);
        if !ai.is_empty() {
            print!("AI: {}\n\r", ai);
        }
    }

    match state {
        ListeningState::Listening => print!("---\n\rListening...\n\r"),
        ListeningState::Thinking => print!("---\n\rThinking...\n\r"),
        ListeningState::None => print!("---\n\r"),
    }

    flush();
}

pub fn muted() {
    print!("\x1B[1A\r\x1B[2K");
    flush();
}

pub fn unmuted() {
    print!("Listening...\n\r");
    flush();
}

// === Input Editing ===

pub fn edit_cancel() {
    print!("\r\x1B[2K\x1b[?25l\x1b[1A");
    flush();
}

pub fn edit_show_buffer(buffer: &str, courser_pos: usize) {
    print!("\r\x1B[K> {}\x1b[{}G", buffer, courser_pos+2);
    flush();
}

pub fn edit_submit() {
    print!("\r\x1B[2K");
    hide_cursor();
    flush();
}

pub fn edit_start(buffer: &str) {
    show_cursor();
    print!("\n\r\x1B[K> {}", buffer);
    flush();
}

pub fn move_courser_left() {
    print!("\x1b[1D");
    flush();
}

pub fn move_courser_right() {
    print!("\x1b[1C");
    flush();
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
