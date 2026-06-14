use std::sync::{Arc, Mutex};
use field_mem_core::DseEngine;
use field_mem_core::EventSource;

/// All available tool definitions sent to the LLM.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        // ── init_field ──
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "init_field",
                "description": "根据用户意图描述构建认知地形。LLM 会从意图中自动提取 40-60 个概念维度，每个概念通过真实 embedding 获得方向，密度由基础性决定。可多次调用，每次追加锚点到已有场上。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "intent": {
                            "type": "string",
                            "description": "用户意图描述，如'我希望你是一个注重长期方案的系统程序员'"
                        }
                    },
                    "required": ["intent"]
                }
            }
        }),
        // ── recall_memory ──
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "recall_memory",
                "description": "从记忆库中召回与查询文本相关的事件和锚点。返回关联锚点（按 impact 排序）和相关历史事件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "查询文本，记忆系统会将其语义嵌入后在场中广播"
                        },
                        "top_k": {
                            "type": "integer",
                            "description": "返回 top 结果数 (1-10，默认 5)",
                            "minimum": 1,
                            "maximum": 10
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
    ]
}

/// Dispatch a tool call to the appropriate handler.
/// Returns a JSON string containing the tool result.
pub fn execute_tool(
    engine: &Arc<Mutex<DseEngine>>,
    name: &str,
    arguments: &serde_json::Value,
) -> String {
    match name {
        "init_field" => execute_init_field(engine, arguments),
        "recall_memory" => execute_recall_memory(engine, arguments),
        _ => serde_json::json!({"error": format!("unknown tool: {}", name)}).to_string(),
    }
}

// ════════════════════════════════════════════
// init_field
// ════════════════════════════════════════════

fn execute_init_field(
    engine: &Arc<Mutex<DseEngine>>,
    arguments: &serde_json::Value,
) -> String {
    let intent = match arguments.get("intent") {
        Some(i) => i.as_str().unwrap_or("").to_string(),
        None => return r#"{"error": "missing 'intent' parameter"}"#.into(),
    };
    if intent.is_empty() {
        return r#"{"error": "intent is empty"}"#.into();
    }

    // 1. Extract concepts via LLM
    let backend_url = std::env::var("LLM_BACKEND")
        .unwrap_or_else(|_| "http://127.0.0.1:4000/v1/chat/completions".into());
    let env_model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "test-model-1".into());
    let api_key = std::env::var("LLM_API_KEY").ok();

    let concepts = match crate::llm::extract_concepts(&backend_url, &env_model, &intent, api_key.as_deref()) {
        Ok(c) => c,
        Err(e) => return serde_json::json!({"error": format!("concept extraction failed: {e}")}).to_string(),
    };

    // 2. Dedup against existing anchors
    let existing_labels: Vec<String> = {
        let eng = engine.lock().unwrap();
        eng.anchors.iter().map(|a| a.label.clone()).collect()
    };

    let (new_concepts, skipped): (Vec<_>, Vec<_>) = concepts
        .into_iter()
        .partition(|(label, _)| !existing_labels.iter().any(|el| el == label));

    if new_concepts.is_empty() {
        return serde_json::json!({
            "status": "skipped",
            "reason": "all concepts already exist as anchors",
            "skipped_concepts": skipped.iter().map(|(l, f)| serde_json::json!({
                "concept": l, "fundamentality": f, "existing": true
            })).collect::<Vec<_>>(),
        }).to_string();
    }

    // 3. Create anchors + inject seed events + relax
    let cycles = 3usize;
    let new_count = new_concepts.len();

    {
        let mut eng = engine.lock().unwrap();
        eng.init_from_descriptions(&new_concepts);
    }

    {
        let mut eng = engine.lock().unwrap();
        for (label, fundamentality) in &new_concepts {
            let repeats = (*fundamentality * 5.0).ceil().max(1.0).min(5.0) as usize;
            for _ in 0..repeats {
                eng.on_input_with_source(label, EventSource::Seed);
            }
        }
    }

    {
        let mut eng = engine.lock().unwrap();
        for _ in 0..cycles { eng.relax(); }
    }

    // 4. Build response
    let (anchors_count, events_count, traces_count, tension) = {
        let eng = engine.lock().unwrap();
        (eng.anchors.len(), eng.events.len(), eng.traces.len(),
         eng.ecg_report().map(|r| r.current.tension).unwrap_or(0.0))
    };
    let anchor_details: Vec<serde_json::Value> = {
        let eng = engine.lock().unwrap();
        eng.anchors.iter().map(|a| serde_json::json!({
            "label": a.label, "density": a.density,
            "stiffness": a.stiffness, "damping": a.damping,
        })).collect()
    };

    let new_concepts_json: Vec<serde_json::Value> = new_concepts.iter().map(|(l, f)| {
        serde_json::json!({ "concept": l, "fundamentality": f })
    }).collect();
    let skipped_json: Vec<serde_json::Value> = skipped.iter().map(|(l, f)| {
        serde_json::json!({ "concept": l, "fundamentality": f, "existing": true })
    }).collect();

    serde_json::json!({
        "status": "ok",
        "intent": intent,
        "new_concepts_count": new_count,
        "skipped_count": skipped.len(),
        "anchors_count": anchors_count,
        "events_count": events_count,
        "traces_count": traces_count,
        "field_tension": tension,
        "anchors": anchor_details,
        "new_concepts": new_concepts_json,
        "skipped_concepts": skipped_json,
    }).to_string()
}

// ════════════════════════════════════════════
// recall_memory
// ════════════════════════════════════════════

fn execute_recall_memory(
    engine: &Arc<Mutex<DseEngine>>,
    arguments: &serde_json::Value,
) -> String {
    let query = match arguments.get("query") {
        Some(q) => q.as_str().unwrap_or(""),
        None => return r#"{"error": "missing 'query' parameter"}"#.into(),
    };
    if query.is_empty() {
        return r#"{"error": "query is empty"}"#.into();
    }
    let top_k = arguments.get("top_k")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .min(10)
        .max(1) as usize;

    let engine = engine.lock().unwrap();
    let recall = engine.recall(query, top_k);
    let assoc = engine.associate(query);

    let anchors: Vec<serde_json::Value> = assoc.iter().take(top_k).map(|(a, imp)| {
        serde_json::json!({
            "label": a.label,
            "density": a.density,
            "impact": format!("{:.3}", imp),
        })
    }).collect();

    let events: Vec<serde_json::Value> = recall.events.iter().take(top_k).map(|(text, anchor, imp)| {
        serde_json::json!({
            "text": text,
            "anchor": anchor,
            "impact": format!("{:.3}", imp),
        })
    }).collect();

    serde_json::json!({
        "query": query,
        "associated_anchors": anchors,
        "recalled_events": events,
        "anchors_count": anchors.len(),
        "events_count": events.len(),
    }).to_string()
}

