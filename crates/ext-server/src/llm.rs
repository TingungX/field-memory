use axum::response::sse::Event;
use futures::stream::Stream;
use std::pin::Pin;
use std::convert::Infallible;

use crate::routes::Message;

/// Proxy streaming chat request to backend LLM, returning SSE events.
/// Backend must support OpenAI-compatible chat completions with streaming.
type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

fn build_client() -> reqwest::Client {
    reqwest::Client::new()
}

fn build_request(
    client: &reqwest::Client,
    backend_url: &str,
    api_key: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = client.post(backend_url);
    let env_key = std::env::var("LLM_API_KEY").ok();
    let key = api_key
        .filter(|k| !k.is_empty())
        .or_else(|| env_key.as_deref())
        .unwrap_or_default();
    if !key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", key));
    }
    req
}

/// Non-streaming chat completion — used for tool call detection.
pub async fn chat_completion(
    backend_url: &str,
    model: &str,
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    api_key: Option<&str>,
) -> Result<serde_json::Value, String> {
    let client = build_client();
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });
    if let Some(t) = tools {
        body["tools"] = serde_json::json!(t);
    }

    let req = build_request(&client, backend_url, api_key).json(&body);
    let resp = req.send().await.map_err(|e| format!("LLM request failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("read response failed: {e}"))?;

    if !status.is_success() {
        return Err(format!("LLM returned HTTP {status}: {text}"));
    }

    serde_json::from_str(&text).map_err(|e| format!("parse response failed: {e}"))
}

pub async fn stream_chat(
    backend_url: &str,
    model: &str,
    messages: &[Message],
    api_key: Option<&str>,
) -> SseStream {
    let client = build_client();
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    let req = build_request(&client, backend_url, api_key).json(&body);

    let response = match req.send().await
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
