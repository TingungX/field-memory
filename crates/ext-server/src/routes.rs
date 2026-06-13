use axum::{
    Json,
    extract::State,
    response::sse::{Event, Sse},
};
use std::convert::Infallible;
use futures::stream::Stream;
use std::sync::Arc;

use crate::AppState;
use crate::llm;

#[allow(dead_code)]
#[derive(serde::Deserialize)]
pub struct OpenAIChatRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    pub model: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
pub struct AnthropicMessagesRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    pub model: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OpenAIChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = handle_chat(state, req.messages, req.stream).await;
    Sse::new(stream)
}

pub async fn messages(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AnthropicMessagesRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = handle_chat(state, req.messages, req.stream).await;
    Sse::new(stream)
}

async fn handle_chat(
    state: Arc<AppState>,
    messages: Vec<Message>,
    _stream: bool,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let last_user = messages.iter()
        .rev()
        .find(|m| m.role == "user")
        .cloned();

    let memory_context = if let Some(ref user_msg) = last_user {
        let engine = state.engine.lock().unwrap();
        let recall = engine.recall(&user_msg.content, 5);
        let assoc = engine.associate(&user_msg.content);

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

        ctx
    } else {
        String::new()
    };

    let system_msg = if memory_context.is_empty() {
        String::new()
    } else {
        format!("[Memory Recall]\n{}\n---\n", memory_context.trim())
    };

    let backend_url = std::env::var("LLM_BACKEND")
        .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".into());
    let backend_model = std::env::var("LLM_MODEL")
        .unwrap_or_else(|_| "qwen2.5:0.5b".into());

    let mut llm_messages = messages.clone();

    if let Some(first_sys) = llm_messages.iter_mut().find(|m| m.role == "system") {
        first_sys.content = format!("{}\n{}", system_msg, first_sys.content);
    } else if !system_msg.is_empty() {
        llm_messages.insert(0, Message {
            role: "system".into(),
            content: system_msg,
        });
    }

    let result = llm::stream_chat(&backend_url, &backend_model, &llm_messages).await;

    if let Some(user_msg) = last_user {
        let engine = state.engine.clone();
        tokio::spawn(async move {
            let mut eng = engine.lock().unwrap();
            eng.on_user_input(&user_msg.content);
            eng.relax();
        });
    }

    result
}

