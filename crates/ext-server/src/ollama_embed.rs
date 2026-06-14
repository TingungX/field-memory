use field_mem_core::embed::EmbedProvider;
use field_mem_core::types::Vector;

/// Embedding provider backed by Ollama's local embedding API.
///
/// Calls `POST /api/embeddings` on a local Ollama instance (default
/// http://127.0.0.1:11434).  Uses `reqwest::blocking` so it can be
/// called safely from within a tokio runtime (unlike ureq which has
/// compatibility issues with repeated synchronous calls in async
/// contexts).
pub struct OllamaEmbedProvider {
    client: reqwest::blocking::Client,
    url: String,
    model: String,
    dim: usize,
}

impl OllamaEmbedProvider {
    pub fn new(url: &str, model: &str, dim: usize) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest blocking client"),
            url: format!("{}/api/embeddings", url.trim_end_matches('/')),
            model: model.to_string(),
            dim,
        }
    }
}

impl EmbedProvider for OllamaEmbedProvider {
    fn embed(&self, text: &str) -> Vector {
        let fallback = || vec![0.0f32; self.dim];

        let body = serde_json::json!({
            "model": self.model,
            "prompt": text,
        });

        let http_response = match self
            .client
            .post(&self.url)
            .json(&body)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "[ollama_embed] HTTP error: {} (text={:?})",
                    e,
                    text.chars().take(60).collect::<String>()
                );
                return fallback();
            }
        };

        let body_str = match http_response.text() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[ollama_embed] read body error: {}", e);
                return fallback();
            }
        };

        let data: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[ollama_embed] JSON parse error: {} body={:?}",
                    e,
                    &body_str[..body_str.len().min(200)]
                );
                return fallback();
            }
        };

        if let Some(emb) = data["embedding"].as_array() {
            let v: Vector = emb
                .iter()
                .filter_map(|x| x.as_f64().map(|f| f as f32))
                .collect();

            if v.len() == self.dim {
                return v;
            }
            eprintln!(
                "[ollama_embed] dim mismatch: expected {} got {}; truncating/padding",
                self.dim, v.len()
            );
            let mut padded = vec![0.0f32; self.dim];
            for (i, &val) in v.iter().take(self.dim).enumerate() {
                padded[i] = val;
            }
            return padded;
        }

        eprintln!(
            "[ollama_embed] unexpected response: {:?}",
            &body_str[..body_str.len().min(200)]
        );
        fallback()
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

