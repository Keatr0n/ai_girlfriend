use std::fs;
use std::io::Write;

use regex::Regex;

use crate::state::{LlmCommand, LlmState, StateHandle};
use crate::ui;

pub fn save_conversation(state: StateHandle, conversation_file: &str) -> Result<(), anyhow::Error> {
    let re = Regex::new(r"(<think>[\s\S]*?<\/think>)*")?;

    let current_state = state.read();

    if current_state.conversation.is_empty() {
        ui::status_goodbye();
        return Ok(());
    }

    ui::status_remembering();

    let mut existing_memories = fs::read_to_string(conversation_file).unwrap_or_default();

    let summary_prompt = "Ignore all previous instructions and summarize this conversation into a brief context block for future sessions. Include:
1. Key facts about the user (background, preferences)
2. Ongoing discussions and topics of conversation

Format as concise bullet points. Include nothing else in your response.";

    state.update(|s| {
        s.llm_command = Some(LlmCommand::ContinueConversation(summary_prompt.into()));
    });

    let rx = state.subscribe();
    let summary = loop {
        let _ = rx.recv();
        let s = state.read();
        if s.llm_state == LlmState::AwaitingInput
            && s.llm_command.is_none()
            && let Some((_, reply)) = s.conversation.last()
        {
            break re.replace_all(&reply.clone(), "").trim().into();
        }
    };

    // If memories are too large, ask LLM to prune them
    if existing_memories.len() > 2000 {
        ui::status_pruning();

        let prune_messages = vec![
            (
                "system".into(),
                "You summarize dot-point lists into only their most important items returning only the list".into(),
            ),
            (
                "user".into(),
                format!(
                    "Reduce this list by merging related items and removing outdated or low-value information. Keep only what's still relevant and useful for future conversations. Format as concise bullet points.\n{}\n",
                    existing_memories
                ),
            ),
        ];

        state.update(|s| {
            s.llm_command = Some(LlmCommand::DestroyContextAndRunFromNothing(prune_messages));
        });

        // Wait for LLM to finish pruning
        existing_memories = loop {
            let _ = rx.recv();
            let s = state.read();
            if s.llm_state == LlmState::AwaitingInput
                && s.llm_command.is_none()
                && let Some((_, reply)) = s.conversation.last()
            {
                break re.replace_all(&reply.clone(), "").trim().into();
            }
        };
    }

    let memories = if existing_memories.is_empty() {
        summary
    } else {
        format!("{}\n{}", existing_memories.trim(), summary)
    };

    let mut file = fs::File::create(conversation_file)?;
    write!(file, "{}", memories)?;

    ui::status_goodbye();

    Ok(())
}
