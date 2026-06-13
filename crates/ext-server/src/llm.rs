use axum::response::sse::Event;
use futures::stream::Stream;
use std::pin::Pin;
use std::convert::Infallible;

use crate::routes::Message;

/// Proxy streaming chat request to backend LLM, returning SSE events.
/// Backend must support OpenAI-compatible chat completions with streaming.
type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

pub async fn stream_chat(
    backend_url: &str,
    model: &str,
    messages: &[Message],
) -> SseStream {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    let response = match client
        .post(backend_url)
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let err_stream = futures::stream::once(async move {
                Ok(Event::default().data(format!("LLM backend error: {}", e)))
            });
            return Box::pin(err_stream);
        }
    };

    let mut byte_stream = response.bytes_stream();
    let sse_stream = async_stream::stream! {
        use futures::StreamExt;
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() { continue; }
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                yield Ok(Event::default().data("[DONE]"));
                                return;
                            }
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                                    yield Ok(Event::default().data(content));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Ok(Event::default().data(format!("stream error: {}", e)));
                }
            }
        }
    };
    Box::pin(sse_stream)
}
