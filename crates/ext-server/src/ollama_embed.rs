use std::time::Duration;
use field_mem_core::embed::EmbedProvider;
use field_mem_core::types::Vector;

/// Embedding provider backed by Ollama's local embedding API.
///
/// Calls `POST /api/embeddings` on a local Ollama instance (default
/// http://127.0.0.1:11434).  Uses `ureq` for blocking HTTP with no
/// tokio dependency, safe to call from `spawn_blocking` or sync contexts.
pub struct OllamaEmbedProvider {
    agent: ureq::Agent,
    url: String,
    model: String,
    dim: usize,
}

impl OllamaEmbedProvider {
    pub fn new(url: &str, model: &str, dim: usize) -> Self {
        let config = ureq::config::Config::builder()
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_send_body(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(30)))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            url: format!("{}/api/embeddings", url.trim_end_matches('/')),
            model: model.to_string(),
            dim,
        }
    }

    fn do_embed(&self, text: &str) -> Result<Vector, String> {
        let body = serde_json::json!({
            "model": self.model,
            "prompt": text,
        });

        let http_resp = self.agent
            .post(&self.url)
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("HTTP: {}", e))?;

        let mut resp_body = http_resp.into_body();
        let body_str = resp_body
            .read_to_string()
            .map_err(|e| format!("read: {}", e))?;

        let data: serde_json::Value =
            serde_json::from_str(&body_str).map_err(|e| format!("JSON: {}", e))?;

        let emb = data["embedding"]
            .as_array()
            .ok_or_else(|| "no 'embedding' field".to_string())?;

        let v: Vector = emb.iter()
            .filter_map(|x| x.as_f64().map(|f| f as f32))
            .collect();

        if v.len() != self.dim {
            return Err(format!("dim mismatch: expected {} got {}", self.dim, v.len()));
        }
        Ok(v)
    }
}

impl EmbedProvider for OllamaEmbedProvider {
    fn embed(&self, text: &str) -> Vector {
        self.do_embed(text).unwrap_or_else(|e| {
            eprintln!("[ollama_embed] {} text={:?}", e, text.chars().take(60).collect::<String>());
            vec![0.0f32; self.dim]
        })
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

