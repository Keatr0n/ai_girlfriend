use std::process::{Command, Stdio};
use std::io::Write;
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
        let current_state = state.read();

        if current_state.life_cycle_state == LifeCycleState::ShuttingDown {
            break;
        }

        if let Some(text) = current_state.tts_command {
            state.update(|s| {
                s.tts_command = None;
            });

            let mut child = Command::new("piper")
                .args(["--model", &model_path, "--output_file", "out.wav"])
                .stdin(Stdio::piped())
                .spawn()?;

            child.stdin.take().unwrap().write_all(re.replace_all(&text, "").as_bytes())?;
            child.wait()?;

            // Play the audio
            Command::new("ffplay")
                .args(["-nodisp", "-autoexit", "out.wav"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
                .wait()?;

            state.update(|s| {
                s.llm_state = LlmState::AwaitingInput;
                s.system_mute = false;
            });
        }
    }

    Ok(())
}
