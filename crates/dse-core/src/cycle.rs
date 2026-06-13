use chrono::{Utc, Duration};
use crate::types::{AnchorKey, Event, ImpactTrace, AnchorId, Vector};
use crate::physics::{impact, relax_step};
use crate::DseCoreParams;

/// Relaxation cycle: the field's only evolution mechanism.
pub struct RelaxationCycle {
    pub window: Duration,
    pub convergence_threshold: f32,
    pub impact_trace_threshold: f32,
}

impl RelaxationCycle {
    pub fn new(params: &DseCoreParams) -> Self {
        Self {
            window: Duration::seconds(params.event_window_secs as i64),
            convergence_threshold: params.convergence_threshold,
            impact_trace_threshold: 0.01,
        }
    }

    /// Run one complete relaxation cycle.
    pub fn run(
        &self,
        events: &[Event],
        anchors: &mut [AnchorKey],
        traces: &mut Vec<ImpactTrace>,
    ) {
        let recent: Vec<&Event> = events.iter()
            .filter(|e| {
                let elapsed = Utc::now().signed_duration_since(e.timestamp);
                elapsed < self.window
            })
            .collect();

        if recent.is_empty() {
            return;
        }

        // Collect event directions for the window
        let dirs: Vec<&Vector> = recent.iter().map(|e| &e.direction).collect();

        // Snapshot anchors for borrow-check-friendly access to other anchors'
        // directions during the mutating loop below.
        let anchor_snapshot: Vec<AnchorKey> = anchors.to_vec();

        for anchor in anchors.iter_mut() {
            // 1. Cumulative perturbation
            let perturbation: f32 = dirs.iter()
                .map(|d| impact(anchor, d))
                .sum();

            if perturbation < 0.01 {
                continue;
            }

            // 2. Effective event direction
            //    Use only the anchor itself (self-influence) and nearby anchors
            let eff_dir = self.compute_effective_dir(&dirs, &anchor_snapshot, anchor.id);

            // 3. Relaxation steps based on perturbation intensity
            let steps = match perturbation {
                p if p > 10.0 => 50,
                p if p > 3.0 => 15,
                p if p > 1.0 => 5,
                _ => 2,
            };

            let last_dir = anchor.direction.clone();

            for _ in 0..steps {
                let converged = relax_step(
                    anchor,
                    perturbation / steps as f32, // divide perturbation across steps
                    &eff_dir,
                    &last_dir,
                    self.convergence_threshold,
                );
                if converged {
                    break;
                }
            }

            // 4. Density increment
            anchor.density += (perturbation * 0.1).ceil() as u32;
            anchor.update_mechanics();

            // 5. Record traces for recall
            for event in &recent {
                let imp = impact(anchor, &event.direction);
                if imp > self.impact_trace_threshold {
                    traces.push(ImpactTrace {
                        event_id: event.id,
                        anchor_id: anchor.id,
                        impact: imp,
                        timestamp: Utc::now(),
                    });
                }
            }
        }
    }

    fn compute_effective_dir(
        &self,
        dirs: &[&Vector],
        anchors: &[AnchorKey],
        _self_id: AnchorId,
    ) -> Vector {
        // Average of event directions, pulled by high-density anchors
        if dirs.is_empty() {
            return vec![0.0; anchors.first().map(|a| a.direction.len()).unwrap_or(0)];
        }
        let n = dirs[0].len();
        let mut total = vec![0.0f32; n];
        let mut count = 0usize;

        for dir in dirs {
            let mut d = (*dir).clone();
            for anchor in anchors {
                let imp = impact(anchor, dir);
                if imp > 0.1 {
                    for i in 0..n {
                        d[i] += imp * anchor.direction[i];
                    }
                }
            }
            // Normalize after pull
            let norm: f32 = d.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut d { *x /= norm; }
            }
            for i in 0..n { total[i] += d[i]; }
            count += 1;
        }

        if count > 0 {
            let norm: f32 = total.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut total { *x /= norm; }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnchorId, EventId};

    fn make_anchor(id: u64, dir: Vec<f32>, density: u32) -> AnchorKey {
        AnchorKey::new(AnchorId(id), format!("a{}", id), dir, density)
    }

    fn make_event(id: u64, dir: Vec<f32>) -> Event {
        Event::new(EventId(id), dir, format!("event{}", id))
    }

    #[test]
    fn test_relaxation_increases_density() {
        let params = DseCoreParams::default();
        let cycle = RelaxationCycle::new(&params);

        let mut anchors = vec![make_anchor(1, vec![1.0, 0.0], 10)];
        let events = vec![make_event(1, vec![1.0, 0.0])];
        let mut traces = vec![];

        let d_before = anchors[0].density;
        cycle.run(&events, &mut anchors, &mut traces);

        assert!(anchors[0].density > d_before, "density should increase after relaxation");
        assert!(!traces.is_empty(), "impact traces should be recorded");
    }

    #[test]
    fn test_no_density_change_for_irrelevant_events() {
        let params = DseCoreParams::default();
        let cycle = RelaxationCycle::new(&params);

        let mut anchors = vec![make_anchor(1, vec![1.0, 0.0], 10)];
        let events = vec![make_event(1, vec![0.0, 1.0])]; // orthogonal
        let mut traces = vec![];

        let d_before = anchors[0].density;
        cycle.run(&events, &mut anchors, &mut traces);

        // Orthogonal event has zero impact, density unchanged
        assert_eq!(anchors[0].density, d_before);
    }
}
