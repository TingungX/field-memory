use dse_core::{DseEngine, DseCoreParams};

fn main() {
    println!("=== DSE-Memory CLI Demo ===\n");

    // 1. Create engine
    let params = DseCoreParams {
        vector_dim: 32,
        event_window_secs: 86400, // 24 hours for demo
        ..Default::default()
    };
    let mut engine = DseEngine::new(params);

    // 2. Initialize anchors
    println!("[1] Initializing anchors...");
    engine.init(&[
        ("偏好 Rust 方案，对 Python 方案天然持怀疑态度", 15),
        ("极其看重代码的长期语义一致性，宁可牺牲短期开发速度", 15),
        ("对 apply_patch 文件操作的可靠性高度关注", 12),
        ("注重项目结构化组织，反对随意散放文件", 12),
        ("偏好纯文本配置，避免复杂框架配置", 10),
        ("注重编译期错误检测，不喜欢运行时意外", 10),
        ("重视记忆和经验的积累，厌恶重复犯错", 8),
    ]);
    println!("   Created {} anchors\n", engine.anchors.len());

    // 3. Simulate conversation
    println!("[2] Simulating conversation...");

    engine.on_user_input("帮我在 proxy 项目里加一个 webfetch 功能");
    engine.on_user_input("不对，用 Rust 重写那个模块");
    engine.on_user_input("上次 apply_patch 替换把 tool 搞丢了，这次要小心");

    // 4. Passive 1: value init
    println!("\n[3] Passive 1 — Value Init:");
    let values = engine.value_init();
    for (a, _density) in values.iter().take(5) {
        println!("  {} (density: {})", a.label, a.density);
    }

    // 5. Passive 2: associative recall
    println!("\n[4] Passive 2 — Associative Recall for 'proxy webfetch':");
    let hits = engine.associate("proxy webfetch");
    for (a, impact) in hits.iter().take(5) {
        println!("  {} (impact: {:.2})", a.label, impact);
    }

    // 6. Relaxation (digest the conversation)
    println!("\n[5] Running relaxation cycle...");
    engine.relax();

    // 7. Active recall
    println!("\n[6] Active — Recall 'apply_patch 的安全问题':");
    let result = engine.recall("apply_patch 的安全问题", 10);
    println!("  Top anchors:");
    for (label, impact) in &result.anchors {
        println!("    {} (impact: {:.2})", label, impact);
    }
    println!("  Retrieved events:");
    for (text, anchor, impact) in &result.events {
        println!("    [{}] {} (impact: {:.2})", anchor, text, impact);
    }

    // 8. ECG report
    println!("\n[7] ECG Report:");
    if let Some(report) = engine.ecg_report() {
        println!("  Field tension: {:.4}", report.current.tension);
        println!("  Convergence rate: {:.4}", report.current.convergence_rate);
        println!("  Anchors: {} | Events: {} | Anisotropies: {}",
            report.current.anchor_count,
            report.current.event_inflow,
            report.current.anisotropies.len(),
        );
    }

    // 9. Persist
    let store_path = "/tmp/dse-demo-data";
    println!("\n[8] Saving to {}...", store_path);
    engine.save(std::path::Path::new(store_path)).unwrap();
    println!("  Done.\n");

    // 10. Load and verify
    println!("[9] Loading from {} for verification...", store_path);
    let mut engine2 = DseEngine::new(DseCoreParams::default());
    engine2.load(std::path::Path::new(store_path)).unwrap();
    println!("  Loaded: {} anchors, {} events, {} traces, {} seeds\n",
        engine2.anchors.len(),
        engine2.events.len(),
        engine2.traces.len(),
        engine2.seeds.len(),
    );

    println!("=== Demo Complete ===");
}

