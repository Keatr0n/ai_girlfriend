use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};

use regex::Regex;

use crate::state::{LifeCycleState, LlmState, StateHandle};

pub struct TtsHandle {
    _handle: JoinHandle<()>,
}

pub fn spawn_tts_thread(state: StateHandle, model_path: String) -> TtsHandle {
    let handle = thread::spawn(move || {
        let _ = run_tts_loop(state, model_path);
    });

    TtsHandle { _handle: handle }
}

fn run_tts_loop(state: StateHandle, model_path: String) -> anyhow::Result<()> {
    let re = Regex::new(r"(<think>[\s\S]*?<\/think>)*(\**)*")?;

    while state.subscribe().recv().is_ok() {
        let mut current_state = state.read();

        if current_state.life_cycle_state == LifeCycleState::ShuttingDown {
            break;
        }

        while !current_state.tts_commands.is_empty() {
            if let Some(text) = current_state.tts_commands.first() {
                let text = text.clone();

                state.update(|s| {
                    s.tts_commands.remove(0);
                });

                if text.is_empty() {
                    continue;
                }

                Command::new("piper")
                    .args([
                        "--model",
                        &model_path,
                        "--output_file",
                        "out.wav",
                        "--",
                        &format!("{}", re.replace_all(&text, "")),
                    ])
                    .spawn()?
                    .wait()?;

                // Play the audio
                Command::new("ffplay")
                    .args(["-nodisp", "-autoexit", "out.wav"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?
                    .wait()?;
            }

            current_state = state.read();
            if current_state.llm_state == LlmState::RunningTts {
                state.update(|s| {
                    s.llm_state = LlmState::AwaitingInput;
                    s.system_mute = false;
                });
            }
        }
    }

    Ok(())
}
