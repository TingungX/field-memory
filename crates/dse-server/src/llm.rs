use axum::response::sse::Event;
use futures::stream::Stream;
use std::convert::Infallible;

use crate::routes::Message;

/// Proxy streaming chat request to backend LLM, returning SSE events.
/// Backend must support OpenAI-compatible chat completions with streaming.
pub async fn stream_chat(
    backend_url: &str,
    model: &str,
    messages: &[Message],
) -> impl Stream<Item = Result<Event, Infallible>> {
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
            return Box::pin(err_stream) as Box<dyn Stream<Item = Result<Event, Infallible>> + Send + Unpin>;
        }
    };

    let mut stream = response.bytes_stream();

    let sse_stream = async_stream::stream! {
        while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);

                    // Parse SSE lines from OpenAI streaming format
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() { continue; }

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                yield Ok(Event::default().data("[DONE]"));
                                return;
                            }

                            // Try to extract content delta
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

    Box::pin(sse_stream) as Box<dyn Stream<Item = Result<Event, Infallible>> + Send + Unpin>
}

