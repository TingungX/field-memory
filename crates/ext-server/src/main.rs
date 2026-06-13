mod routes;
mod llm;
mod memory_routes;
mod tools;

use axum::Router;
use field_mem_core::{DseEngine, DseCoreParams};
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub engine: Arc<Mutex<DseEngine>>,
    /// Known libraries: name -> (anchors_count, events_count) snapshot
    pub libraries: Arc<Mutex<std::collections::HashMap<String, (usize, usize)>>>,
    /// Currently active library name
    pub active_library: Arc<Mutex<String>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let params = DseCoreParams {
        vector_dim: 32,
        event_window_secs: 86400,
        ..Default::default()
    };

    let mut engine = DseEngine::new(params);

    engine.init(&[
        ("代码质量和长期语义一致性", 12),
        ("偏好简洁直接的方案", 10),
        ("对重复犯错敏感", 8),
    ]);

    let state = Arc::new(AppState {
        engine: Arc::new(Mutex::new(engine)),
        libraries: Arc::new(Mutex::new(std::collections::HashMap::new())),
        active_library: Arc::new(Mutex::new("default".to_string())),
    });

    let app = Router::new()
        .route("/v1/chat/completions", axum::routing::post(routes::chat_completions))
        .route("/v1/messages", axum::routing::post(routes::messages))
        .route("/api/memory/status", axum::routing::get(memory_routes::status))
        .route("/api/memory/save", axum::routing::post(memory_routes::save))
        .route("/api/memory/load", axum::routing::post(memory_routes::load))
        .route("/api/memory/ping", axum::routing::get(memory_routes::ping))
        .route("/api/memory/init", axum::routing::post(memory_routes::init))
        .route("/api/memory/seed", axum::routing::post(memory_routes::seed))
        .route("/api/memory/query", axum::routing::post(memory_routes::query))
        .route("/api/memory/libraries", axum::routing::get(memory_routes::list_libraries))
        .route("/api/memory/library/save", axum::routing::post(memory_routes::library_save))
        .route("/api/memory/library/load", axum::routing::post(memory_routes::library_load))
        .route("/api/memory/library/delete", axum::routing::post(memory_routes::library_delete))
        // Dedicated 3D field visualization page
        .route("/field", axum::routing::get_service(ServeFile::new("crates/ext-server/src/static/field.html")))
        .fallback_service(ServeDir::new("crates/ext-server/src/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "5000".into());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("server listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
