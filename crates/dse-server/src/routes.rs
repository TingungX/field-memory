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

// ================================================================
// Request types (accept both OpenAI and Anthropic shapes)
// ================================================================

#[derive(serde::Deserialize)]
pub struct OpenAIChatRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct AnthropicMessagesRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ================================================================
// OpenAI-format endpoint
// ================================================================

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OpenAIChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = handle_chat(state, req.messages, req.stream).await;
    Sse::new(stream)
}

// ================================================================
// Anthropic-format endpoint
// ================================================================

pub async fn messages(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AnthropicMessagesRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = handle_chat(state, req.messages, req.stream).await;
    Sse::new(stream)
}

// ================================================================
// Core handler: extract last user msg -> DSE recall -> inject -> LLM
// ================================================================

async fn handle_chat(
    state: Arc<AppState>,
    messages: Vec<Message>,
    stream: bool,
) -> impl Stream<Item = Result<Event, Infallible>> {
    // 1. Extract last user message
    let last_user = messages.iter()
        .rev()
        .find(|m| m.role == "user")
        .cloned();

    let memory_context = if let Some(ref user_msg) = last_user {
        // Acquire engine lock briefly
        let engine = state.engine.lock().unwrap();

        // Run DSE recall
        let recall = engine.recall(&user_msg.content, 5);
        let assoc = engine.associate(&user_msg.content);

        // Build memory context string
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

    // 2. Build system prompt with memory injection
    let system_msg = if memory_context.is_empty() {
        String::new()
    } else {
        format!(
            "[DSE Memory Recall]\n{}\n---\n",
            memory_context.trim()
        )
    };

    // 3. Forward to backend LLM
    //    Backend URL from env or default
    let backend_url = std::env::var("LLM_BACKEND")
        .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".into());
    let backend_model = std::env::var("LLM_MODEL")
        .unwrap_or_else(|_| "qwen2.5:0.5b".into());

    // Build messages with injected system context
    let mut llm_messages = messages.clone();

    // Prepend or prepend to first system message
    if let Some(first_sys) = llm_messages.iter_mut().find(|m| m.role == "system") {
        first_sys.content = format!("{}\n{}", system_msg, first_sys.content);
    } else if !system_msg.is_empty() {
        llm_messages.insert(0, Message {
            role: "system".into(),
            content: system_msg,
        });
    }

    // 4. Call backend LLM with streaming
    let result = llm::stream_chat(&backend_url, &backend_model, &llm_messages).await;

    // 5. After response, write event and run relaxation (in background)
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

