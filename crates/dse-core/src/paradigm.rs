#[cfg(test)]
use crate::math::cosine_similarity;
use crate::math::{cosine_distance, normalize, dot};
use crate::types::{AnchorKey, SeedConcept, AnchorId, SeedId};

/// Detect paradigm shift: two opposing anchors where pressure is heavily lopsided.
pub fn detect_paradigm_shift(
    a: &AnchorKey,
    b: &AnchorKey,
    a_pressure: f32,
    b_pressure: f32,
) -> bool {
    let divergence = cosine_distance(&a.direction, &b.direction);
    divergence > 0.8 && a_pressure > 3.0 * b_pressure && a.density > 5
}

/// Orthogonalize loser anchor relative to winner.
/// Gram-Schmidt: v_orth = v_loser - proj_winner(v_loser)
pub fn orthogonalize(loser: &AnchorKey, winner: &AnchorKey) -> SeedConcept {
    let dot_lw = dot(&loser.direction, &winner.direction);
    let dot_ww = dot(&winner.direction, &winner.direction);
    let proj_scale = if dot_ww.abs() < 1e-8 { 0.0 } else { dot_lw / dot_ww };

    let mut ortho = loser.direction.clone();
    for i in 0..ortho.len() {
        ortho[i] -= proj_scale * winner.direction[i];
    }
    let ortho_normalized = normalize(&ortho);

    SeedConcept {
        id: SeedId::new(),
        orthogonal_direction: ortho_normalized,
        shadow_anchor: loser.clone(),
        defeated_by: winner.id,
        pressure_accumulated: 0.0,
    }
}

/// Revive a seed back to full anchor (paradigm shift execution).
pub fn revive(seed: &SeedConcept) -> AnchorKey {
    let mut anchor = seed.shadow_anchor.clone();
    anchor.id = AnchorId::new(); // new identity
    anchor.update_mechanics();
    anchor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AnchorId;

    fn a(id: u64, dir: Vec<f32>, density: u32) -> AnchorKey {
        AnchorKey::new(AnchorId(id), format!("a{}", id), dir, density)
    }

    #[test]
    fn test_detect_false_for_same_direction() {
        let a1 = a(1, vec![1.0, 0.0], 10);
        let a2 = a(2, vec![0.9, 0.1], 10);
        assert!(!detect_paradigm_shift(&a1, &a2, 10.0, 2.0));
    }

    #[test]
    fn test_detect_true_for_opposite_with_pressure() {
        let a1 = a(1, vec![1.0, 0.0], 10); // diverging, high pressure, sufficient density
        let a2 = a(2, vec![-1.0, 0.0], 3);
        assert!(detect_paradigm_shift(&a1, &a2, 10.0, 2.0));
    }

    #[test]
    fn test_orthogonalize_is_orthogonal() {
        let winner = a(1, vec![1.0, 0.0], 10);
        let loser = a(2, vec![0.0, 1.0], 3);
        let seed = orthogonalize(&loser, &winner);
        // The orthogonal direction should be close to loser's original direction
        // since winner and loser are orthogonal to begin with
        let sim = cosine_similarity(&seed.orthogonal_direction, &vec![0.0, 1.0]);
        assert!(sim > 0.9);
    }

    #[test]
    fn test_revive_restores_anchor() {
        let loser = a(2, vec![0.5, 0.5], 10);
        let winner = a(1, vec![1.0, 0.0], 20);
        let seed = orthogonalize(&loser, &winner);
        let revived = revive(&seed);
        assert_eq!(revived.label, loser.label);
        assert_eq!(revived.density, loser.density);
    }
}
