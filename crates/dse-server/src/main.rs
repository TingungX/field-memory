mod routes;
mod llm;

use axum::Router;
use dse_core::{DseEngine, DseCoreParams};
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

pub struct AppState {
    pub engine: Arc<Mutex<DseEngine>>,
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
    });

    let app = Router::new()
        .route("/v1/chat/completions", axum::routing::post(routes::chat_completions))
        .route("/v1/messages", axum::routing::post(routes::messages))
        .fallback_service(ServeDir::new("crates/dse-server/src/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:4000").await.unwrap();
    println!("server listening on http://127.0.0.1:4000");
    axum::serve(listener, app).await.unwrap();
}

