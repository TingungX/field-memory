mod routes;
mod llm;
mod memory_routes;
mod sessions;
mod tools;
mod ollama_embed;

use axum::Router;
use field_mem_core::{DseEngine, DseCoreParams};
use field_mem_core::embed::EmbedProvider;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

pub struct AppState {
    pub engine: Arc<Mutex<DseEngine>>,
    /// Known libraries: name -> (anchors_count, events_count) snapshot
    pub libraries: Arc<Mutex<std::collections::HashMap<String, (usize, usize)>>>,
    /// Currently active library name
    pub active_library: Arc<Mutex<String>>,
    /// Server-side source of truth for sessions.
    /// Loaded once at startup; every mutation goes through `sessions::mutate()` so
    /// each change is atomically persisted before returning.
    pub sessions: Arc<Mutex<sessions::SessionsData>>,
    /// Embedding provider configuration — used when creating new engines
    /// (e.g. library_create) so the new engine uses the same embed model.
    pub embed_config: EmbedConfig,
}

#[derive(Clone)]
pub struct EmbedConfig {
    pub url: String,
    pub model: String,
    pub dim: usize,
}

impl EmbedConfig {
    pub fn new_boxed(&self) -> Box<dyn EmbedProvider> {
        Box::new(ollama_embed::OllamaEmbedProvider::new(
            &self.url, &self.model, self.dim,
        ))
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let embed_url = std::env::var("EMBED_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434".into());
    let embed_model = std::env::var("EMBED_MODEL")
        .unwrap_or_else(|_| "bge-m3".into());
    let embed_dim: usize = std::env::var("EMBED_DIM")
        .unwrap_or_else(|_| "1024".into())
        .parse()
        .unwrap_or(1024);

    let embed_config = EmbedConfig {
        url: embed_url.clone(),
        model: embed_model.clone(),
        dim: embed_dim,
    };

    let params = DseCoreParams {
        vector_dim: embed_dim,
        event_window_secs: 86400,
        ..Default::default()
    };

    let engine = DseEngine::with_embed(params, embed_config.new_boxed());

    let state = Arc::new(AppState {
        engine: Arc::new(Mutex::new(engine)),
        libraries: Arc::new(Mutex::new(std::collections::HashMap::new())),
        active_library: Arc::new(Mutex::new("default".to_string())),
        sessions: sessions::shared(),
        embed_config: embed_config,
    });

    // Register the default library so it appears in the library list
    {
        let mut libs = state.libraries.lock().unwrap();
        libs.insert("default".to_string(), (0, 0)); // fresh engine, no anchors yet
    }

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
        .route("/api/memory/library/create", axum::routing::post(memory_routes::library_create))
        .route("/api/memory/library/delete", axum::routing::post(memory_routes::library_delete))
// Sessions persistence — server-side source of truth.
        // Each mutation atomically saves to disk before returning, so a partial
        // client request can never lose server state.
        .route("/api/sessions", axum::routing::get(sessions::list).post(sessions::create))
.route("/api/sessions/{id}", axum::routing::patch(sessions::patch).delete(sessions::delete))
        .route("/api/sessions/{id}/messages", axum::routing::post(sessions::append_message))
.route("/api/sessions/{id}/messages/{idx}", axum::routing::patch(sessions::update_message).delete(sessions::delete_message))
        // Dedicated 3D field visualization page
        .route("/field", axum::routing::get_service(ServeFile::new("crates/ext-server/src/static/field.html")))
        .fallback_service(
            ServeDir::new("crates/ext-server/src/static")
                .fallback(ServeFile::new("crates/ext-server/src/static/index.html"))
        )
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::CACHE_CONTROL,
            "no-cache".parse::<axum::http::HeaderValue>().unwrap(),
        ))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "5000".into());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("server listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
