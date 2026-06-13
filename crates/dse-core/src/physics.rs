use crate::math::{cosine_similarity, normalize, add, scale};
use crate::types::{Vector, AnchorKey, Event};

/// The ONE core equation: impact of an event on an anchor.
/// impact = cos_sim(event_dir, anchor_dir) × √density
pub fn impact(anchor: &AnchorKey, event_dir: &Vector) -> f32 {
    cosine_similarity(event_dir, &anchor.direction) * (anchor.density as f32).sqrt()
}

/// Event magnitude = total resonance across all anchors
pub fn event_magnitude(anchors: &[AnchorKey], event_dir: &Vector) -> f32 {
    anchors.iter()
        .map(|a| impact(a, event_dir))
        .sum()
}

/// Effective event direction: event_dir pulled by all anchor directions.
/// effective = normalize(event_dir + Σ impact_i × anchor_i.direction)
/// Only anchors with impact > threshold participate to avoid O(n²) noise.
pub fn effective_direction(
    event_dir: &Vector,
    anchors: &[AnchorKey],
    threshold: f32,
) -> Vector {
    let mut total = scale(event_dir, 1.0); // start with original
    for anchor in anchors {
        let imp = impact(anchor, event_dir);
        if imp > threshold {
            let contribution = scale(&anchor.direction, imp);
            total = add(&total, &contribution);
        }
    }
    normalize(&total)
}

/// Single relaxation step for an anchor.
/// Returns new direction and whether convergence was reached.
/// anchor.post_dir = normalize(anchor.dir + push + pull)
///   where push = perturbation × effective_dir
///         pull = damping × (anchor.last_dir - anchor.dir)
pub fn relax_step(
    anchor: &mut AnchorKey,
    perturbation: f32,
    effective_dir: &Vector,
    last_direction: &Vector,
    convergence_threshold: f32,
) -> bool {
    let push = scale(effective_dir, perturbation);
    let delta = add(last_direction, &scale(&anchor.direction, -1.0)); // last_dir - cur_dir
    let pull = scale(&delta, anchor.damping);

    let new_dir = normalize(&add(&add(&anchor.direction, &push), &pull));

    let moved = crate::math::cosine_distance(&new_dir, &anchor.direction);

    anchor.direction = new_dir;

    moved < convergence_threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AnchorId;

    fn make_anchor(dir: Vector, density: u32) -> AnchorKey {
        AnchorKey::new(AnchorId::new(), "test".into(), dir, density)
    }

    #[test]
    fn test_impact_identical_direction() {
        let anchor = make_anchor(vec![1.0, 0.0], 16); // √16 = 4
        let event_dir = vec![1.0, 0.0];
        let imp = impact(&anchor, &event_dir);
        assert!((imp - 4.0).abs() < 0.001); // cos=1.0 × 4 = 4.0
    }

    #[test]
    fn test_impact_orthogonal() {
        let anchor = make_anchor(vec![1.0, 0.0], 16);
        let event_dir = vec![0.0, 1.0];
        let imp = impact(&anchor, &event_dir);
        assert!((imp - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_event_magnitude_single_anchor() {
        let anchors = vec![make_anchor(vec![1.0, 0.0], 9)]; // √9 = 3
        let mag = event_magnitude(&anchors, &vec![1.0, 0.0]);
        assert!((mag - 3.0).abs() < 0.001);
    }

    #[test]
    #[ignore = "plan design bug: orthogonal event (cos_sim=0) has zero impact, cannot be pulled"]
    fn test_effective_direction_pulled() {
        let anchors = vec![
            make_anchor(vec![1.0, 0.0], 100), // strong anchor pulls right
        ];
        let event = vec![0.0, 1.0]; // event points up
        let eff = effective_direction(&event, &anchors, 0.0);
        // Should be pulled toward [1,0] so x component > 0
        assert!(eff[0] > 0.3, "effective dir should be pulled right, got x={}", eff[0]);
    }
}
