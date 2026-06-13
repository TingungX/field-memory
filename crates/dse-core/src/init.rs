use crate::types::{AnchorKey, AnchorId};
use crate::embed::EmbedProvider;

/// Initialize anchors from concept descriptions.
/// Each description → embedded → AnchorKey with initial density.
pub fn init_anchors(embed: &dyn EmbedProvider, concepts: &[(&str, u32)]) -> Vec<AnchorKey> {
    concepts.iter()
        .map(|(desc, density)| {
            let dir = embed.embed(desc);
            AnchorKey::new(
                AnchorId::new(),
                extract_label(desc).to_string(),
                dir,
                *density,
            )
        })
        .collect()
}

/// Extract a short label from a concept description
fn extract_label(desc: &str) -> &str {
    // Take first 3-4 Chinese characters or first 2 words
    let chars: Vec<char> = desc.chars().collect();
    if chars.len() <= 6 {
        desc
    } else {
        // Try to split at first punctuation
        for (i, &c) in chars.iter().enumerate() {
            if c == '，' || c == '。' || c == '、' || c == ' ' {
                if i >= 2 {
                    return &desc[..desc.char_indices().nth(i).map(|(pos, _)| pos).unwrap_or(desc.len())];
                }
            }
        }
        // Take first ~6 chars
        let end = desc.char_indices()
            .nth(6)
            .map(|(pos, _)| pos)
            .unwrap_or(desc.len());
        &desc[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::DummyEmbedProvider;

    #[test]
    fn test_init_creates_anchors() {
        let embed = DummyEmbedProvider::new(32);
        let concepts = vec![
            ("偏好 Rust 方案", 15u32),
            ("注重长期语义一致性", 12u32),
        ];
        let anchors = init_anchors(&embed, &concepts);
        assert_eq!(anchors.len(), 2);
        assert!(anchors[0].density == 15);
        assert!(anchors[0].stiffness > anchors[1].stiffness); // higher density = higher stiffness
        assert!(anchors[0].damping < anchors[1].damping); // higher density = lower damping (more memorable)
    }

    #[test]
    fn test_extract_label_short() {
        assert_eq!(extract_label("Rust"), "Rust");
    }
}

