use crate::types::Vector;

/// Embedding provider trait — the ONE external dependency of DSE
pub trait EmbedProvider: Send + Sync {
    /// Convert text to a direction vector
    fn embed(&self, text: &str) -> Vector;
    /// Expected vector dimension
    fn dim(&self) -> usize;
}

/// Dummy provider for testing — deterministic pseudo-random vectors
/// based on a hash of the input text. No external API calls.
pub struct DummyEmbedProvider {
    dim: usize,
}

impl DummyEmbedProvider {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    /// Simple hash-based pseudo-embedding.
    /// Different inputs produce different but deterministic directions.
    fn hash_to_vector(&self, text: &str) -> Vector {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut v = vec![0.0f32; self.dim];
        let bytes = text.as_bytes();

        for i in 0..self.dim {
            let mut hasher = DefaultHasher::new();
            (i as u64).hash(&mut hasher);
            for &b in bytes {
                b.hash(&mut hasher);
            }
            let h = hasher.finish();
            // Map u64 to f32 in [-1, 1]
            v[i] = ((h as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32;
        }

        // Normalize
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v { *x /= norm; }
        }
        v
    }
}

impl EmbedProvider for DummyEmbedProvider {
    fn embed(&self, text: &str) -> Vector {
        self.hash_to_vector(text)
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::cosine_similarity;

    #[test]
    fn test_same_text_same_direction() {
        let p = DummyEmbedProvider::new(32);
        let a = p.embed("hello");
        let b = p.embed("hello");
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_different_text_different_direction() {
        let p = DummyEmbedProvider::new(32);
        let a = p.embed("Rust programming");
        let b = p.embed("Python scripting");
        let sim = cosine_similarity(&a, &b);
        // Very different texts should have low similarity (not identical)
        assert!(sim < 0.99);
    }

    #[test]
    fn test_unit_norm() {
        let p = DummyEmbedProvider::new(64);
        let v = p.embed("test text");
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.001);
    }
}

