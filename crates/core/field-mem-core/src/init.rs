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

/// Use the full concept description as the anchor label.
/// Previous implementation truncated to ≤6 chars, which destroyed semantic
/// integrity ("高二学生住在成都" → "高二学生住在") and made recall results
/// meaningless. The label is the anchor's identity — it must be complete.
fn extract_label(desc: &str) -> &str {
    desc
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
        assert!(anchors[0].damping > anchors[1].damping); // higher density = higher damping (more stable)
    }

    #[test]
    fn test_extract_label_short() {
        assert_eq!(extract_label("Rust"), "Rust");
    }
}
