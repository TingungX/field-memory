use crate::types::{AnchorKey, AnchorId};
use crate::embed::EmbedProvider;

/// Initialize anchors from concept descriptions with fundamentality scores.
/// Each concept → embedded → AnchorKey with density derived from fundamentality.
/// fundamentality (0.0–1.0) maps to density via `(f * 20.0).ceil()`, range 1–20.
pub fn init_from_concepts(embed: &dyn EmbedProvider, concepts: &[(String, f32)]) -> Vec<AnchorKey> {
    concepts.iter()
        .map(|(desc, fundamentality)| {
            let dir = embed.embed(desc);
            let density = (fundamentality * 20.0).ceil().max(1.0).min(20.0) as u32;
            AnchorKey::new(
                AnchorId::new(),
                extract_label(desc).to_string(),
                dir,
                density,
            )
        })
        .collect()
}

/// Legacy init: create anchors from (label, raw density) pairs.
/// Prefer `init_from_concepts` which uses fundamentality scoring.
#[deprecated(note = "use init_from_concepts with fundamentality (0.0-1.0) instead")]
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
    fn test_init_from_concepts_density_mapping() {
        let embed = DummyEmbedProvider::new(32);
        let concepts = vec![
            ("核心原则".to_string(), 1.0),  // density = 20
            ("辅助概念".to_string(), 0.5),  // density = 10
            ("边缘概念".to_string(), 0.05), // density = 1
        ];
        let anchors = init_from_concepts(&embed, &concepts);
        assert_eq!(anchors.len(), 3);
        assert_eq!(anchors[0].density, 20); // 1.0 * 20 = 20
        assert_eq!(anchors[1].density, 10); // 0.5 * 20 = 10
        assert_eq!(anchors[2].density, 1);  // 0.05 * 20 = 1.0 → ceil = 1
    }

    #[test]
    fn test_init_from_concepts_high_fundamentality_high_stiffness() {
        let embed = DummyEmbedProvider::new(32);
        let concepts = vec![
            ("高基础性".to_string(), 0.95),
            ("低基础性".to_string(), 0.1),
        ];
        let anchors = init_from_concepts(&embed, &concepts);
        assert!(anchors[0].stiffness > anchors[1].stiffness, "higher density = higher stiffness");
        assert!(anchors[0].damping > anchors[1].damping, "higher density = higher damping (more stable)");
    }

    #[test]
    #[allow(deprecated)]
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
