use axum::{
    Json,
    extract::State,
    response::sse::{Event, Sse},
};
use std::convert::Infallible;
use futures::stream::Stream;
use futures::StreamExt;
use std::sync::Arc;

use crate::AppState;
use crate::llm;
use crate::tools;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct OpenAIChatRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    pub model: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct AnthropicMessagesRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    pub model: Option<String>,
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OpenAIChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(handle_chat(state, req.messages).await)
}

pub async fn messages(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AnthropicMessagesRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(handle_chat(state, req.messages).await)
}

// ── Main handler ──

async fn handle_chat(
    state: Arc<AppState>,
    messages: Vec<Message>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let backend_url = std::env::var("LLM_BACKEND")
        .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".into());
    let backend_model = std::env::var("LLM_MODEL")
        .unwrap_or_else(|_| "qwen2.5:0.5b".into());

    // 1. Extract last user text for memory context
    let last_user_text = messages.iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_deref())
        .unwrap_or("")
        .to_string();
    let last_user_text_ref = if last_user_text.is_empty() { None } else { Some(last_user_text.as_str()) };

    let (memory_context, assoc_json, recall_json) = if let Some(query) = last_user_text_ref {
        let engine = state.engine.lock().unwrap();
        let recall = engine.recall(query, 5);
        let assoc = engine.associate(query);

        let assoc_json: Vec<serde_json::Value> = assoc.iter().take(5).map(|(a, imp)| {
            serde_json::json!({"label": a.label, "impact": format!("{:.2}", imp)})
        }).collect();

        let recall_json: Vec<serde_json::Value> = recall.events.iter().take(5).map(|(text, anchor, _imp)| {
            serde_json::json!({"text": text, "anchor": anchor})
        }).collect();

        let mut ctx = String::new();
        if !assoc.is_empty() {
            ctx.push_str("[关联概念] ");
            for (a, imp) in assoc.iter().take(5) {
                ctx.push_str(&format!("{} (关联度:{:.1}) ", a.label, imp));
            }
            ctx.push('\n');
        }
        if !recall.events.is_empty() {
            ctx.push_str("[相关记忆]\n");
            for (text, anchor, _imp) in recall.events.iter().take(5) {
                ctx.push_str(&format!("  [{}] {}\n", anchor, text));
            }
        }
        (ctx, assoc_json, recall_json)
    } else {
        (String::new(), vec![], vec![])
    };

    // 2. Build LLM messages with memory context injected
    let system_msg = if memory_context.is_empty() {
        String::new()
    } else {
        format!("[Memory Recall]\n{}\n---\n", memory_context.trim())
    };

    let mut llm_messages = messages.clone();
    if let Some(first_sys) = llm_messages.iter_mut().find(|m| m.role == "system") {
        let existing = first_sys.content.take().unwrap_or_default();
        first_sys.content = Some(format!("{}\n{}", system_msg, existing));
    } else if !system_msg.is_empty() {
        llm_messages.insert(0, Message {
            role: "system".into(),
            content: Some(system_msg),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // 3. Try tool calling phase (non-streaming)
    let tool_defs = tools::tool_definitions();
    let should_try_tools = !llm_messages.iter().any(|m| m.role == "tool");

    let (final_messages, tool_was_invoked) = if should_try_tools {
        match llm::chat_completion(&backend_url, &backend_model, &llm_messages, Some(&tool_defs)).await {
            Ok(resp) => {
                let finish = resp["choices"][0]["finish_reason"].as_str().unwrap_or("");
                if finish == "tool_calls" {
                    let tool_calls = &resp["choices"][0]["message"]["tool_calls"];
                    if let Some(calls) = tool_calls.as_array() {
                        if let Some(first_call) = calls.first() {
                            let fn_name = first_call["function"]["name"].as_str().unwrap_or("");
                            let fn_args: serde_json::Value = first_call["function"]["arguments"]
                                .as_str()
                                .and_then(|s| serde_json::from_str(s).ok())
                                .unwrap_or(serde_json::Value::Null);

                            // Execute tool
                            let tool_result = tools::execute_tool(&state.engine, fn_name, &fn_args);

                            // Build follow-up messages with tool_call + result
                            let mut followup = llm_messages.clone();

                            // Assistant message with tool_calls
                            let tool_call_entry = serde_json::json!({
                                "id": first_call["id"],
                                "type": "function",
                                "function": {
                                    "name": fn_name,
                                    "arguments": first_call["function"]["arguments"]
                                }
                            });
                            followup.push(Message {
                                role: "assistant".into(),
                                content: None,
                                tool_calls: Some(vec![tool_call_entry]),
                                tool_call_id: None,
                            });

                            // Tool result message
                            followup.push(Message {
                                role: "tool".into(),
                                content: Some(tool_result),
                                tool_calls: None,
                                tool_call_id: first_call["id"].as_str().map(|s| s.to_string()),
                            });

                            (followup, true)
                        } else {
                            (llm_messages.clone(), false)
                        }
                    } else {
                        (llm_messages.clone(), false)
                    }
                } else {
                    (llm_messages.clone(), false)
                }
            }
            Err(_e) => {
                // LLM doesn't support tools or errored; fall back to streaming directly
                (llm_messages.clone(), false)
            }
        }
    } else {
        (llm_messages.clone(), false)
    };

    // 4. Build __MEMORY__ event
    let memory_event_json = serde_json::json!({
        "associations": assoc_json,
        "recalled_events": recall_json,
        "system_prompt_line": memory_context.trim(),
        "tool_invoked": tool_was_invoked,
    });
    let memory_event_payload = format!("__MEMORY__{}", memory_event_json.to_string());
    let memory_event_stream = futures::stream::once(async move {
        Ok(Event::default().data(memory_event_payload))
    });

    // 5. Stream the final LLM response
    let llm_stream = llm::stream_chat(&backend_url, &backend_model, &final_messages).await;

    // 6. Spawn async memory write
    if let Some(query) = last_user_text_ref {
        let engine = state.engine.clone();
        let q = query.to_string();
        tokio::spawn(async move {
            let mut eng = engine.lock().unwrap();
            eng.on_user_input(&q);
            eng.relax();
        });
    }

    Box::pin(memory_event_stream.chain(llm_stream))
}

