use std::fs;
use std::io::Write;

use llama_cpp_2::model::LlamaChatMessage;

use crate::state::{LlmCommand, LlmState, StateHandle};
use crate::ui;

pub fn save_conversation(state: &StateHandle, conversation_file: &str) -> Result<(), anyhow::Error> {
    let current_state = state.read();

    if current_state.conversation.is_empty() {
        ui::status_goodbye();
        return Ok(());
    }

    ui::status_remembering();
    state.update(|s| {
        s.tts_command = Some("Give me a moment, I am just filing this conversation away so I can remember it later.".into());
    });

    let existing_data = fs::read_to_string(conversation_file).unwrap_or_default();

    let summary_prompt = "Summarize this conversation into a brief context block for future sessions. Include:
1. Key facts about the user (background, preferences)
2. Ongoing discussions and topics of conversation

Format as concise bullet points suitable for a system prompt. Focus on actionable context, not play-by-play.";

    state.update(|s| {
        s.llm_command = Some(LlmCommand::ContinueConversation(summary_prompt.into()));
    });

    let rx = state.subscribe();
    let summary = loop {
        let _ = rx.recv();
        let s = state.read();
        if s.llm_state == LlmState::AwaitingInput
            && let Some((_, reply)) = s.conversation.last() {
                break reply.clone();
            }
    };

    let mut memories = if existing_data.is_empty() {
        summary
    } else {
        format!("{}\n{}", existing_data.trim(), summary)
    };

    // If memories are too large, ask LLM to prune them
    if memories.len() > 2000 {
        ui::status_pruning();

        let prune_messages = vec![
            LlamaChatMessage::new(
                "system".into(),
                "You summarize dot-point lists into only their most important items".into(),
            )?,
            LlamaChatMessage::new(
                "user".into(),
                format!(
                    "Reduce this context list by merging related items and removing outdated or low-value information. Keep only what's still relevant and useful for future conversations.\n{}",
                    memories
                ),
            )?,
        ];

        state.update(|s| {
            s.llm_command = Some(LlmCommand::DestroyContextAndRunFromNothing(prune_messages));
            s.tts_command = Some("It seems we've talked a lot in the past. So I'll need to prune some of these memories so I don't consume your whole drive.".into());
        });

        // Wait for LLM to finish pruning
        let rx = state.subscribe();
        memories = loop {
            let _ = rx.recv();
            let s = state.read();
            if s.llm_state == LlmState::AwaitingInput
                && let Some((_, reply)) = s.conversation.last() {
                    break reply.clone();
                }
        };
    }

    let mut file = fs::File::create(conversation_file)?;
    write!(file, "{}", memories)?;

    ui::status_goodbye();

    Ok(())
}
