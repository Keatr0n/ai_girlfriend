use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::state::{LifeCycleState, LlmCommand, LlmState, StateHandle};

pub struct InputHandle {
    _handle: JoinHandle<()>,
}

pub fn spawn_input_thread(state: StateHandle) -> InputHandle {

    let handle = thread::spawn(move || {
        run_input_loop(state);
    });

    InputHandle { _handle: handle }
}

fn run_input_loop(state: StateHandle) {
    let _ = enable_raw_mode();

    let mut pre_edit_mute_state = false;

    loop {
        let current_state = state.read();
        if current_state.life_cycle_state == LifeCycleState::ShuttingDown {
            break;
        }

        if !poll(Duration::from_millis(10)).unwrap_or(false) {
            continue;
        }

        let Ok(Event::Key(key)) = read() else {
            continue;
        };

        // Ctrl+C - shutdown
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            let _ = disable_raw_mode();
            state.update(|s| {
                s.life_cycle_state = LifeCycleState::ShuttingDown
            });
            break;
        }

        if let Some((edit_buffer, cursor_pos)) = current_state.text_input {
            match key.code {
                KeyCode::Down | KeyCode::Esc => {
                    state.update(|s| {
                        s.text_input = None;
                        s.is_editing = false;
                        s.user_mute = pre_edit_mute_state;
                    });
                }
                KeyCode::Backspace => {
                    if cursor_pos == 0 || edit_buffer.chars().count() == 0 {
                        continue;
                    }

                    let mut new_buffer = edit_buffer.clone();

                    if cursor_pos < edit_buffer.chars().count() {
                        new_buffer.remove(cursor_pos-1);
                    } else {
                        new_buffer.pop();
                    }

                    state.update(|s| {
                        s.text_input = Some((new_buffer,cursor_pos-1));
                    });
                }
                KeyCode::Enter => {
                    let text = edit_buffer.clone();
                    state.update(|s| {
                        s.system_mute = true;
                        if s.is_editing {
                            s.user_mute = pre_edit_mute_state;
                            s.text_input = None;
                            s.conversation.pop();
                            s.conversation.push((text.clone(), "".into()));
                            s.llm_command = Some(LlmCommand::EditLastMessage(text));
                            s.is_editing = false;
                        } else {
                            s.text_input = Some(("".into(), 0));
                            s.conversation.push((text.clone(), "".into()));
                            s.llm_command = Some(LlmCommand::ContinueConversation(text));
                        }
                    });
                }
                KeyCode::Left => {
                    if cursor_pos > 0 {
                        state.update(|s| {
                            s.text_input = Some((edit_buffer, cursor_pos-1));
                        });
                    }
                }
                KeyCode::Right => {
                    if cursor_pos < edit_buffer.chars().count() {
                        state.update(|s| {
                            s.text_input = Some((edit_buffer, cursor_pos+1));
                        });
                    }
                }
                _ => {
                    if current_state.llm_state == LlmState::RunningInference { continue; }

                    if let Some(c) = key.code.as_char() {
                        let mut new_buffer = edit_buffer.clone();
                        if cursor_pos >= edit_buffer.chars().count() {
                            new_buffer.push(c);
                        } else {
                            new_buffer.insert(cursor_pos, c);
                        }

                        state.update(|s| {
                            s.text_input = Some((new_buffer, cursor_pos+1));
                        });
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Up => {
                    let current = state.read();
                    let mut edit_buffer = String::new();
                    if let Some((user, _)) = current.conversation.last() {
                        edit_buffer = user.clone();
                    }

                    let cursor_pos = edit_buffer.chars().count();
                    state.update(|s| {
                        s.text_input = Some((edit_buffer, cursor_pos));
                        s.is_editing = true;
                        pre_edit_mute_state = s.user_mute;
                        s.user_mute = true;
                    });
                }
                KeyCode::Esc => {
                    state.update(|s| {
                        s.llm_command = Some(LlmCommand::CancelInference);
                    });
                }
                KeyCode::Char('m') => {
                    state.update(|s| {
                        s.user_mute = !s.user_mute;
                    });
                }
                KeyCode::Char('t') => {
                    state.update(|s| {
                        s.text_input = Some(("".into(), 0));
                        s.is_editing = false;
                        pre_edit_mute_state = s.user_mute;
                        s.user_mute = true;
                    });
                }
                _ => {}
            }
        }
    }
}
