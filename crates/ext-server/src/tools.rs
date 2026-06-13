use std::sync::{Arc, Mutex};
use field_mem_core::DseEngine;

/// All available tool definitions sent to the LLM.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        // ── seed_memory ──
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "seed_memory",
                "description": "根据一组种子概念创建全新的记忆知识库。每个概念包含标签和初始密度值（密度越高代表越重要）。系统会为每个概念生成合成事件并执行松弛周期。通常在对话初期调用一次。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "concepts": {
                            "type": "array",
                            "description": "种子概念列表",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": {
                                        "type": "string",
                                        "description": "概念名称/标签，如 '编程习惯'、'代码规范'"
                                    },
                                    "density": {
                                        "type": "integer",
                                        "description": "初始密度 (1-100)，越高越重要",
                                        "minimum": 1,
                                        "maximum": 100
                                    }
                                },
                                "required": ["label", "density"]
                            }
                        }
                    },
                    "required": ["concepts"]
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
        // ── associate_memory ──
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "associate_memory",
                "description": "从记忆库中查找与查询文本语义相关的概念锚点。比 recall 更轻量，只返回概念级关联，不返回具体事件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "查询文本"
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
        "seed_memory" => execute_seed_memory(engine, arguments),
        "recall_memory" => execute_recall_memory(engine, arguments),
        "associate_memory" => execute_associate_memory(engine, arguments),
        _ => serde_json::json!({"error": format!("unknown tool: {}", name)}).to_string(),
    }
}

// ════════════════════════════════════════════
// seed_memory
// ════════════════════════════════════════════

fn execute_seed_memory(
    engine: &Arc<Mutex<DseEngine>>,
    arguments: &serde_json::Value,
) -> String {
    let concepts_raw = match arguments.get("concepts") {
        Some(c) => c.as_array().map(|a| a.clone()).unwrap_or_default(),
        None => return r#"{"error": "missing 'concepts' parameter"}"#.into(),
    };
    if concepts_raw.is_empty() {
        return r#"{"error": "concepts list is empty"}"#.into();
    }

    let concepts: Vec<(String, u32)> = concepts_raw
        .iter()
        .filter_map(|c| {
            let label = c.get("label")?.as_str()?.to_string();
            let density = c.get("density")?.as_u64().unwrap_or(5) as u32;
            Some((label, density))
        })
        .collect();
    if concepts.is_empty() {
        return r#"{"error": "no valid concepts after parsing"}"#.into();
    }

    let concepts_refs: Vec<(&str, u32)> = concepts.iter().map(|(l, d)| (l.as_str(), *d)).collect();
    let events_per = 10usize;
    let cycles = 5usize;
    let modifiers = ["擅长", "不喜欢", "需要改进", "重点关注", "积累经验",
                      "讨论过", "遇到的问题", "学到的教训"];

    {
        let mut eng = engine.lock().unwrap();
        for c in &concepts_refs { eng.init(&[*c]); }
    }
    {
        let mut eng = engine.lock().unwrap();
        for (label, _) in &concepts {
            for i in 0..events_per {
                let mod_idx = i.min(modifiers.len() - 1);
                let event_text = if mod_idx == 0 {
                    format!("{}: 这是最核心的原则", label)
                } else {
                    format!("{}: {} 相关的讨论和记录", modifiers[mod_idx], label)
                };
                eng.on_user_input(&event_text);
                if i % 3 == 0 {
                    eng.on_user_input(&format!("关于{}的补充思考第{}条", label, i + 1));
                }
            }
        }
    }
    {
        let mut eng = engine.lock().unwrap();
        for _ in 0..cycles { eng.relax(); }
    }

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

    serde_json::json!({
        "status": "ok",
        "anchors_count": anchors_count,
        "events_count": events_count,
        "traces_count": traces_count,
        "field_tension": tension,
        "anchors": anchor_details,
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

// ════════════════════════════════════════════
// associate_memory
// ════════════════════════════════════════════

fn execute_associate_memory(
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

    let engine = engine.lock().unwrap();
    let assoc = engine.associate(query);

    let anchors: Vec<serde_json::Value> = assoc.iter().take(10).map(|(a, imp)| {
        serde_json::json!({
            "label": a.label,
            "density": a.density,
            "impact": format!("{:.3}", imp),
        })
    }).collect();

    serde_json::json!({
        "query": query,
        "associated_anchors": anchors,
        "count": anchors.len(),
    }).to_string()
}

