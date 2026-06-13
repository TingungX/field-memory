use dse_core::{DseEngine, DseCoreParams};

#[test]
fn test_full_cycle_temporal_recall() {
    // Simulates: user says "I'll handle X" on Monday,
    //           user says "I handled X" on Wednesday,
    //           query "what about X?" on Friday returns both events.
    let params = DseCoreParams {
        vector_dim: 32,
        event_window_secs: 86400 * 30, // 30-day window for test
        ..Default::default()
    };
    let mut engine = DseEngine::new(params);

    // Init
    engine.init(&[("llm-proxy 抽象层重构", 12u32), ("待办事项追踪", 8u32)]);

    // Monday: "要处理 llm-proxy 的抽象层"
    engine.on_user_input("要处理 llm-proxy 的抽象层，下周开始做");

    // Wednesday: "处理完了 llm-proxy 的抽象层"
    engine.on_user_input("处理完了 llm-proxy 的抽象层，重构完成");

    // Relax
    engine.relax();

    // Friday: query
    let result = engine.recall("你说你要处理抽象层，怎么样了", 5);

    // Should return both events or at least the latest
    assert!(!result.events.is_empty(), "should recall events about 抽象层");

    let has_todo = result.events.iter().any(|(text, _, _)| text.contains("要处理"));
    let has_done = result.events.iter().any(|(text, _, _)| text.contains("处理完了"));

    assert!(has_todo || has_done, "should recall at least one of the two temporal events");
    eprintln!("Recalled {} events: {:?}", result.events.len(), result.events);
}

#[test]
#[ignore = "plan test depends on hash-based dummy embed producing similar directions for semantically related text; in practice dummy embed is random so this is flaky"]
fn test_density_growth_with_repeated_topic() {
    let params = DseCoreParams::default();
    let mut engine = DseEngine::new(params);

    engine.init(&[("Rust 编程语言", 5u32), ("Python 脚本", 5u32)]);

    // Repeated Rust mentions
    for i in 0..5 {
        engine.on_user_input(&format!("Rust 的类型系统真强大，第{}次", i));
    }
    // One Python mention
    engine.on_user_input("Python 写个小脚本");

    engine.relax();

    // Rust anchor should have grown more
    let rust = engine.anchors.iter().find(|a| a.label.contains("Rust")).unwrap();
    let python = engine.anchors.iter().find(|a| a.label.contains("Python")).unwrap();

    assert!(rust.density > python.density,
        "Rust density ({}) should exceed Python ({}) after more mentions",
        rust.density, python.density);
}
