use std::process::{Command, Stdio};
use std::io::Write;

pub fn speak(text: &str, model_path: &str) -> anyhow::Result<()> {
    let mut child = Command::new("piper")
        .args(["--model", model_path, "--output_file", "out.wav"])
        .stdin(Stdio::piped())
        .spawn()?;

    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    child.wait()?;

    // Play the audio
    Command::new("ffplay")
        .args(["-nodisp", "-autoexit", "out.wav"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?
        .wait()?;

    Ok(())
}
