use serde::Deserialize;
use std::fs;
use std::io;

use crate::ui;

const CONFIG_FILE: &str = "./config.toml";

#[derive(Debug, Deserialize, Clone)]
pub struct GlobalConfig {
    pub whisper_model_path: String,
    #[serde(default)]
    pub default_llm_model_path: Option<String>,
    #[serde(default)]
    pub default_piper_model_path: Option<String>,
    pub llm_threads: i32,
    pub llm_context_size: u32,
    #[serde(default)]
    pub default_assistant: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Assistant {
    pub name: String,
    pub system_prompt: String,
    #[serde(default)]
    pub llm_model_path: Option<String>,
    #[serde(default)]
    pub piper_model_path: Option<String>,
    #[serde(default)]
    pub conversation_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub global: GlobalConfig,
    pub assistant: Vec<Assistant>,
}

impl Assistant {
    pub fn conversation_file(&self) -> String {
        self.conversation_file
            .clone()
            .unwrap_or_else(|| format!("{}_history.txt", self.name.to_lowercase().replace(' ', "_")))
    }
}

pub fn load_config() -> anyhow::Result<Config> {
    let content = fs::read_to_string(CONFIG_FILE)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn select_assistant(config: &Config) -> anyhow::Result<Assistant> {
    if config.assistant.is_empty() {
        anyhow::bail!("No assistants defined in config");
    }

    if config.assistant.len() == 1 {
        ui::assistant_selected(&config.assistant[0].name);
        return Ok(config.assistant[0].clone());
    }

    // Check for default
    if let Some(default_name) = &config.global.default_assistant &&
        let Some(assistant) = config.assistant.iter().find(|a| &a.name == default_name) {
            ui::assistant_selected(&assistant.name);
            return Ok(assistant.clone());
    }

    // Interactive selection
    ui::assistant_selection_header();
    for (i, assistant) in config.assistant.iter().enumerate() {
        ui::assistant_option(i + 1, &assistant.name);
    }

    ui::assistant_prompt(config.assistant.len());

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let mut selection: usize = input.trim().parse().unwrap_or(0);
    if selection < 1 || selection > config.assistant.len() {
        ui::assistant_invalid_selection();
        selection = 1;
    }

    let selected = &config.assistant[selection - 1];
    ui::assistant_selected(&selected.name);

    Ok(selected.clone())
}
