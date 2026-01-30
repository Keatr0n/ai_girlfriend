use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::ui;

#[derive(Debug, Clone)]
pub enum InputEvent {
    Shutdown,
    Interrupt,
    EditStart,
    EditCancel,
    EditSubmit(String),
    Muted,
}

pub struct InputHandle {
    pub rx: Receiver<InputEvent>,
    _handle: JoinHandle<()>,
}

pub fn spawn_input_thread(history: Arc<Mutex<Vec<(String, String)>>>) -> InputHandle {
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        run_input_loop(tx, history);
    });

    InputHandle { rx, _handle: handle }
}

fn run_input_loop(tx: Sender<InputEvent>, history: Arc<Mutex<Vec<(String, String)>>>) {
    let _ = enable_raw_mode();

    let mut is_editing = false;
    let mut edit_buffer = String::new();
    let mut courser_pos: usize = 0;

    loop {
        if !poll(Duration::from_millis(10)).unwrap_or(false) {
            continue;
        }

        let Ok(Event::Key(key)) = read() else {
            continue;
        };

        // Ctrl+C - shutdown
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            let _ = disable_raw_mode();
            let _ = tx.send(InputEvent::Shutdown);
            break;
        }

        if is_editing {
            match key.code {
                KeyCode::Down | KeyCode::Esc => {
                    is_editing = false;
                    edit_buffer.clear();
                    ui::edit_cancel();
                    let _ = tx.send(InputEvent::EditCancel);
                }
                KeyCode::Backspace => {
                    if courser_pos <= 1 || edit_buffer.chars().count() == 0 {
                        continue;
                    }

                    if courser_pos - 1 < edit_buffer.chars().count() {
                        edit_buffer.remove(courser_pos-2);
                    } else {
                        edit_buffer.pop();
                    }
                    courser_pos -= 1;
                    ui::edit_show_buffer(&edit_buffer, courser_pos);
                }
                KeyCode::Enter => {
                    is_editing = false;
                    let text = std::mem::take(&mut edit_buffer);
                    ui::edit_submit();
                    let _ = tx.send(InputEvent::EditSubmit(text));
                }
                KeyCode::Left => {
                    if courser_pos > 1 {
                        ui::move_courser_left();
                        courser_pos -=1;
                    }
                }
                KeyCode::Right => {
                    if courser_pos <= edit_buffer.chars().count() {
                        ui::move_courser_right();
                        courser_pos +=1;
                    }
                }
                _ => {
                    if let Some(c) = key.code.as_char() {
                        if courser_pos >= edit_buffer.chars().count() {
                            edit_buffer.push(c);
                        } else {
                            edit_buffer.insert(courser_pos-1, c);
                        }
                        courser_pos += 1;
                        ui::edit_show_buffer(&edit_buffer, courser_pos);
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Up => {
                    is_editing = true;

                    if let Ok(history) = history.lock() && let Some((user, _)) = history.last() {
                        edit_buffer = user.clone();
                    }

                    courser_pos = edit_buffer.chars().count() +1;
                    ui::edit_start(&edit_buffer);
                    let _ = tx.send(InputEvent::EditStart);
                }
                KeyCode::Esc => {
                    let _ = tx.send(InputEvent::Interrupt);
                }
                KeyCode::Char('m') => {
                    let _ = tx.send(InputEvent::Muted);
                }
                _ => {}
            }
        }
    }
}
