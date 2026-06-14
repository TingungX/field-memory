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
    reasoning_effort: Option<&str>,
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
    // Only inject the field if the caller supplied one. Models that don't
    // understand it (legacy / non-reasoning) should see no change.
    if let Some(r) = reasoning_effort {
        if !r.is_empty() {
            body["reasoning_effort"] = serde_json::Value::String(r.to_string());
        }
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
    reasoning_effort: Option<&str>,
) -> SseStream {
    let client = build_client();
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });
    if let Some(r) = reasoning_effort {
        if !r.is_empty() {
            body["reasoning_effort"] = serde_json::Value::String(r.to_string());
        }
    }

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
        // Accumulator for tool_calls: openai streams them as incremental deltas
        // keyed by `index`. We coalesce by index and re-emit the full array
        // on every delta so the client can overwrite-and-render without
        // doing the merge itself.
        let mut tool_calls_buf: Vec<serde_json::Value> = Vec::new();
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
                                let delta = &parsed["choices"][0]["delta"];
                                // Main content — emitted as plain delta text.
                                if let Some(content) = delta["content"].as_str() {
                                    yield Ok(Event::default().data(content));
                                }
                                // Reasoning — try both common field names.
                                // Emitted per-delta so the UI can stream-think
                                // like the main content.
                                let reasoning_text = delta["reasoning_content"]
                                    .as_str()
                                    .or_else(|| delta["reasoning"].as_str());
                                if let Some(r) = reasoning_text {
                                    if !r.is_empty() {
                                        let payload = serde_json::json!({"delta": r});
                                        yield Ok(Event::default().data(
                                            format!("__REASONING__{}", payload)
                                        ));
                                    }
                                }
                                // Tool calls — coalesce by index, re-emit whole array.
                                if let Some(calls) = delta["tool_calls"].as_array() {
                                    if !calls.is_empty() {
                                        for call in calls {
                                            let idx = call["index"]
                                                .as_u64()
                                                .unwrap_or(0) as usize;
                                            while tool_calls_buf.len() <= idx {
                                                tool_calls_buf.push(serde_json::json!({}));
                                            }
                                            let entry = tool_calls_buf[idx]
                                                .as_object_mut()
                                                .expect("just initialized as object");
                                            if let Some(id) = call["id"].as_str() {
                                                entry.insert(
                                                    "id".into(),
                                                    serde_json::Value::String(id.to_string()),
                                                );
                                            }
                                            if let Some(t) = call["type"].as_str() {
                                                entry.insert(
                                                    "type".into(),
                                                    serde_json::Value::String(t.to_string()),
                                                );
                                            }
                                            // function.{name,arguments} may arrive
                                            // as nested object or be absent — both fine.
                                            if let Some(fn_obj) =
                                                call.get("function").and_then(|v| v.as_object())
                                            {
                                                let target = entry
                                                    .entry("function".to_string())
                                                    .or_insert_with(|| serde_json::json!({}))
                                                    .as_object_mut()
                                                    .expect("function must be object");
                                                if let Some(name) = fn_obj.get("name").and_then(|v| v.as_str()) {
                                                    target.insert(
                                                        "name".into(),
                                                        serde_json::Value::String(name.to_string()),
                                                    );
                                                }
                                                if let Some(args) = fn_obj.get("arguments").and_then(|v| v.as_str()) {
                                                    let existing = target
                                                        .get("arguments")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("")
                                                        .to_string();
                                                    target.insert(
                                                        "arguments".into(),
                                                        serde_json::Value::String(
                                                            format!("{}{}", existing, args)
                                                        ),
                                                    );
                                                }
                                            }
                                        }
                                        yield Ok(Event::default().data(
                                            format!("__TOOL_CALLS__{}",
                                                serde_json::to_string(&tool_calls_buf)
                                                    .unwrap_or_else(|_| "[]".into()))
                                        ));
                                    }
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


// ════════════════════════════════════════════
// Concept extraction for init_field
// ════════════════════════════════════════════

/// Extract concepts from user intent description using LLM (synchronous, ureq-based).
/// Returns Vec<(concept_label, fundamentality)> where fundamentality is 0.0–1.0.
pub fn extract_concepts(
    backend_url: &str,
    model: &str,
    user_intent: &str,
    api_key: Option<&str>,
) -> Result<Vec<(String, f32)>, String> {
    let prompt = format!(
        "你正在为一个势能场记忆系统构建初始认知地形。\
从以下用户意图描述中提取 40-60 个核心概念维度。\n\n\
规则：\n\
- 概念应该是该领域的基础维度，不是具体事实\n\
- fundamentality 越高表示该概念越核心、越不可绕过\n\
- 概念之间应该有足够的语义差异（不要列出近义词）\n\
- 不要包含用户偏好本身（如\"喜欢Rust\"），而是偏好背后的维度（如\"类型安全直觉\"）\n\
- 输出必须是纯 JSON 数组，不要有其他文字\n\n\
用户意图描述：{}\n\n\
输出格式（严格 JSON 数组）：\n\
[{{\"concept\": \"概念名\", \"fundamentality\": 0.8}}, ...]",
        user_intent
    );

    let messages = serde_json::json!([
        {"role": "user", "content": prompt}
    ]);

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });

    let env_key = std::env::var("LLM_API_KEY").ok();
    let key = api_key
        .filter(|k| !k.is_empty())
        .or_else(|| env_key.as_deref())
        .unwrap_or_default();

    let config = ureq::config::Config::builder().build();
    let agent = ureq::Agent::new_with_config(config);

    let mut req = agent.post(backend_url);
    if !key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", key));
    }
    req = req.header("Content-Type", "application/json");

    let resp = req.send_json(&body).map_err(|e| format!("LLM request failed: {e}"))?;

    let mut resp_body = resp.into_body();
    let resp_text = resp_body.read_to_string().map_err(|e| format!("read response failed: {e}"))?;

    // Parse the OpenAI-compatible response
    let resp_json: serde_json::Value =
        serde_json::from_str(&resp_text).map_err(|e| format!("parse response failed: {e}"))?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "no content in LLM response".to_string())?;

    // The LLM may wrap the JSON in markdown code blocks — strip them
    let content_clean = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let concepts_array: serde_json::Value =
        serde_json::from_str(content_clean).map_err(|e| {
            format!("parse concepts JSON failed: {e}\nraw content: {content}")
        })?;

    let arr = concepts_array
        .as_array()
        .ok_or_else(|| "concepts response is not a JSON array".to_string())?;

    let mut concepts = Vec::new();
    for item in arr {
        let concept = item["concept"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let fundamentality = item["fundamentality"].as_f64().unwrap_or(0.5) as f32;
        if !concept.is_empty() && fundamentality > 0.0 {
            concepts.push((concept, fundamentality.clamp(0.01, 1.0)));
        }
    }

    if concepts.is_empty() {
        return Err("no valid concepts extracted from LLM response".to_string());
    }

    Ok(concepts)
}
