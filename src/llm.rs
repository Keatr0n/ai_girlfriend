use std::num::NonZeroU32;
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaChatMessage, LlamaModel, params},
    sampling::LlamaSampler,
};
use regex::Regex;

use crate::state::StateHandle;
use crate::state::{LifeCycleState, LlmCommand, LlmState};
use crate::tools::{ToJson, parse_python_functions, run_tool, supports_tools, try_parse_tool_call};
use crate::ui;
use rand::RngCore;

pub struct LlmHandle {
    _handle: JoinHandle<()>,
}

pub fn spawn_llm_thread(
    state: StateHandle,
    path: String,
    llm_threads: i32,
    llm_context_size: u32,
    enable_word_by_word_response: bool,
    system_prompt: String,
    tool_directory: Option<String>,
) -> LlmHandle {
    let handle = thread::spawn(move || {
        let _ = run_llm_loop(
            state,
            path,
            llm_threads,
            llm_context_size,
            enable_word_by_word_response,
            system_prompt,
            tool_directory,
        );
    });

    LlmHandle { _handle: handle }
}

fn run_llm_loop(
    state: StateHandle,
    path: String,
    llm_threads: i32,
    llm_context_size: u32,
    enable_word_by_word_response: bool,
    system_prompt: String,
    tool_directory: Option<String>,
) -> anyhow::Result<()> {
    let mut backend = Box::new(LlamaBackend::init()?);
    let end_sentence = Regex::new(r"[.?;:]")?;
    backend.void_logs();

    ui::status_llm_loaded();

    let model = Box::new(LlamaModel::load_from_file(
        &backend,
        &path,
        &params::LlamaModelParams::default(),
    )?);

    let context_params = LlamaContextParams::default()
        .with_n_threads(llm_threads)
        .with_n_ctx(NonZeroU32::new(llm_context_size));

    let mut ctx = model.new_context(&backend, context_params)?;

    let _chat_template = model.chat_template(None).unwrap();

    let mut batch = LlamaBatch::new(4096, 1);

    let prompt = if supports_tools(_chat_template.to_str()?)
        && let Some(tool_directory) = tool_directory.clone()
    {
        let tools_str = parse_python_functions(tool_directory)
            .to_json()
            .unwrap_or_default();

        let proopt = model.apply_chat_template_with_tools_oaicompat(
            &_chat_template,
            &[LlamaChatMessage::new("system".into(), system_prompt.clone()).unwrap()],
            Some(&tools_str),
            None,
            false,
        );

        match proopt {
            Ok(data) => data,
            Err(_) => model.apply_chat_template_with_tools_oaicompat(
                &_chat_template,
                &[LlamaChatMessage::new("system".into(), system_prompt).unwrap()],
                None,
                None,
                false,
            )?,
        }
    } else {
        model.apply_chat_template_with_tools_oaicompat(
            &_chat_template,
            &[LlamaChatMessage::new("system".into(), system_prompt).unwrap()],
            None,
            None,
            false,
        )?
    };

    let system_tokens = model.str_to_token(&prompt.prompt, AddBos::Always).unwrap();

    for (i, token) in system_tokens.iter().enumerate() {
        let is_last = i == system_tokens.len() - 1;
        batch.add(*token, i as i32, &[0], is_last).unwrap();
    }

    ui::status_llm_context_init();

    ctx.decode(&mut batch).unwrap();

    let system_token_len = system_tokens.len() as i32;
    let mut n_past = system_token_len;
    let mut exchange_checkpoints: Vec<i32> = vec![];

    state.update(|s| {
        s.llm_state = LlmState::AwaitingInput;
        s.system_mute = false;
        s.life_cycle_state = LifeCycleState::Running;
    });

    while state.subscribe().recv().is_ok() {
        let current_state = state.read();

        let messages: Vec<LlamaChatMessage> = if let Some(command) = current_state.llm_command {
            match command {
                LlmCommand::CancelInference => continue,
                LlmCommand::ContinueConversation(message) => {
                    vec![LlamaChatMessage::new("user".into(), message).unwrap()]
                }
                LlmCommand::DestroyContextAndRunFromNothing(llama_chat_messages) => {
                    ctx.clear_kv_cache();
                    llama_chat_messages
                        .iter()
                        .map(|(role, message)| {
                            LlamaChatMessage::new(role.into(), message.into()).unwrap()
                        })
                        .collect()
                }
                LlmCommand::EditLastMessage(message) => {
                    if let Some(checkpoint) = exchange_checkpoints.pop() {
                        ctx.clear_kv_cache_seq(
                            Some(0),
                            Some((checkpoint - 1) as u32),
                            Some(ctx.kv_cache_seq_pos_max(0) as u32),
                        )
                        .unwrap_or(false);
                    }

                    vec![LlamaChatMessage::new("user".into(), message).unwrap()]
                }
            }
        } else {
            continue;
        };

        state.update(|s| {
            s.llm_command = None;
            s.llm_state = LlmState::RunningInference;
        });

        let n_past_before = n_past;
        exchange_checkpoints.push(n_past_before);

        let chat_message = model
            .apply_chat_template(&_chat_template, messages.as_slice(), true)
            .unwrap();
        let user_tokens = model.str_to_token(&chat_message, AddBos::Never).unwrap();
        batch.clear();

        for (i, token) in user_tokens.iter().enumerate() {
            let is_last = i == user_tokens.len() - 1;
            batch.add(*token, n_past + i as i32, &[0], is_last).unwrap();
        }

        ctx.decode(&mut batch).unwrap();
        n_past += user_tokens.len() as i32;

        let mut rng = rand::rng();

        let mut reply = String::new();
        let mut last_message_chunk_index = 0;
        let mut sampler = LlamaSampler::chain_simple([LlamaSampler::dist(rng.next_u32())]);
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        let mut interrupted = false;

        loop {
            // Check for interrupt event
            if state.read().llm_command == Some(LlmCommand::CancelInference) {
                interrupted = true;
                break;
            }

            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if model.is_eog_token(token) {
                break;
            }

            if let Ok(t) = model.token_to_piece(token, &mut decoder, true, None) {
                reply.push_str(&t);
                if enable_word_by_word_response {
                    state.update(|s| {
                        if let Some((user, _)) = s.conversation.pop() {
                            s.conversation.push((user, reply.clone()));
                        }
                        if end_sentence.is_match(&t) {
                            let sentence = &reply[last_message_chunk_index..];
                            last_message_chunk_index = reply.len();
                            s.tts_commands.push(sentence.into());
                        }
                    });
                }
            }

            batch.clear();
            batch.add(token, n_past, &[0], true).unwrap();
            ctx.decode(&mut batch).unwrap();
            n_past += 1;
        }

        if interrupted {
            // Roll back KV cache to state before this inference
            let _ = ctx.clear_kv_cache_seq(
                None,
                Some((n_past_before - 1) as u32),
                Some(ctx.kv_cache_seq_pos_max(0) as u32),
            );
            exchange_checkpoints.pop();

            state.update(|s| {
                if let Some((_, _)) = s.conversation.pop() {
                    if s.is_only_responding_after_name {
                        s.time_since_name_was_said = Some(Instant::now());
                    }

                    s.llm_state = LlmState::AwaitingInput;
                }
            });

            continue;
        }

        // Check for tool calls and execute them
        if let Some(ref tool_dir) = tool_directory
            && let Some((_format, tool_command)) = try_parse_tool_call(&reply)
        {
            match run_tool(tool_dir, &tool_command) {
                Ok(tool_result) => {
                    // Add tool result as a message and continue inference
                    let tool_response_messages = vec![
                        LlamaChatMessage::new("assistant".into(), reply.clone()).unwrap(),
                        LlamaChatMessage::new("tool".into(), tool_result.clone()).unwrap(),
                    ];

                    let tool_chat = model
                        .apply_chat_template(
                            &_chat_template,
                            tool_response_messages.as_slice(),
                            true,
                        )
                        .unwrap();
                    let tool_tokens = model.str_to_token(&tool_chat, AddBos::Never).unwrap();
                    batch.clear();

                    for (i, token) in tool_tokens.iter().enumerate() {
                        let is_last = i == tool_tokens.len() - 1;
                        batch.add(*token, n_past + i as i32, &[0], is_last).unwrap();
                    }

                    ctx.decode(&mut batch).unwrap();
                    n_past += tool_tokens.len() as i32;

                    // Generate follow-up response
                    let mut follow_up = String::new();
                    let mut decoder = encoding_rs::UTF_8.new_decoder();
                    let mut sampler =
                        LlamaSampler::chain_simple([LlamaSampler::dist(rng.next_u32())]);

                    loop {
                        if state.read().llm_command == Some(LlmCommand::CancelInference) {
                            break;
                        }

                        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                        sampler.accept(token);

                        if model.is_eog_token(token) {
                            break;
                        }

                        if let Ok(t) = model.token_to_piece(token, &mut decoder, true, None) {
                            follow_up.push_str(&t);
                            if enable_word_by_word_response {
                                state.update(|s| {
                                    if let Some((user, _)) = s.conversation.pop() {
                                        s.conversation.push((user, follow_up.clone()));
                                    }
                                    if end_sentence.is_match(&t) {
                                        let sentence = &follow_up[last_message_chunk_index..];
                                        last_message_chunk_index = follow_up.len();
                                        s.tts_commands.push(sentence.into());
                                    }
                                });
                            }
                        }

                        batch.clear();
                        batch.add(token, n_past, &[0], true).unwrap();
                        ctx.decode(&mut batch).unwrap();
                        n_past += 1;
                    }

                    // Use follow-up as the final reply
                    reply = follow_up;
                }
                Err(e) => {
                    println!("Tool execution failed: {:?}\r", e);
                }
            }
        }

        state.update(|s| {
            if let Some((user, _)) = s.conversation.pop() {
                s.conversation.push((user, reply.clone()));

                if s.is_only_responding_after_name {
                    s.time_since_name_was_said = Some(Instant::now());
                }

                if s.life_cycle_state != LifeCycleState::ShuttingDown {
                    s.llm_state = LlmState::RunningTts;

                    if !enable_word_by_word_response {
                        s.tts_commands.push(reply);
                    }
                } else {
                    s.llm_state = LlmState::AwaitingInput;
                }
            }
        });
    }

    Ok(())
}
