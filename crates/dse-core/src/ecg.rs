use chrono::{DateTime, Utc, Duration};
use crate::types::{AnchorKey, ImpactTrace, Anisotropy};
use crate::math::cosine_distance;

/// Cognitive ECG — pure observation, no decisions.
#[derive(Debug, Clone)]
pub struct CognitiveEcg {
    pub snapshots: Vec<FieldSnapshot>,
}

#[derive(Debug, Clone)]
pub struct FieldSnapshot {
    pub timestamp: DateTime<Utc>,
    pub tension: f32,
    pub convergence_rate: f32,
    pub anisotropies: Vec<Anisotropy>,
    pub event_inflow: usize,
    pub anchor_count: usize,
}

#[derive(Debug, Clone)]
pub struct EcgReport {
    pub current: FieldSnapshot,
    pub trend: Vec<(DateTime<Utc>, f32)>, // tension history
}

impl CognitiveEcg {
    pub fn new() -> Self {
        Self { snapshots: vec![] }
    }

    /// Take a snapshot of the field state.
    pub fn snapshot(
        &mut self,
        anchors: &[AnchorKey],
        traces: &[ImpactTrace],
        event_inflow: usize,
        window: Duration,
    ) {
        let now = Utc::now();

        // Field tension: average impact per anchor in recent window
        let recent_traces: Vec<&ImpactTrace> = traces.iter()
            .filter(|t| {
                let elapsed = now.signed_duration_since(t.timestamp);
                elapsed < window
            })
            .collect();

        let tension = if anchors.is_empty() || recent_traces.is_empty() {
            0.0
        } else {
            recent_traces.iter().map(|t| t.impact).sum::<f32>() / anchors.len() as f32
        };

        // Convergence rate: average direction change vs previous snapshot
        let convergence_rate = if let Some(prev) = self.snapshots.last() {
            // Recompute from previous state — simplified: use tension change as proxy
            (tension - prev.tension).abs()
        } else {
            0.0
        };

        // Directional bias: detect anchor clusters drifting together
        let anisotropies = detect_anisotropies(anchors);

        let snap = FieldSnapshot {
            timestamp: now,
            tension,
            convergence_rate,
            anisotropies,
            event_inflow,
            anchor_count: anchors.len(),
        };

        self.snapshots.push(snap);
        if self.snapshots.len() > 1000 {
            self.snapshots.remove(0);
        }
    }

    pub fn report(&self) -> Option<EcgReport> {
        self.snapshots.last().map(|current| {
            let trend: Vec<(DateTime<Utc>, f32)> = self.snapshots.iter()
                .rev()
                .take(100)
                .map(|s| (s.timestamp, s.tension))
                .collect();
            EcgReport {
                current: current.clone(),
                trend,
            }
        })
    }
}

/// Detect directional bias: clusters of anchors drifting in similar directions.
fn detect_anisotropies(anchors: &[AnchorKey]) -> Vec<Anisotropy> {
    if anchors.len() < 3 {
        return vec![];
    }

    // Simple approach: find the anchor that has drifted most from its origin
    let mut drifts: Vec<(&AnchorKey, f32)> = anchors.iter()
        .map(|a| {
            let drift = cosine_distance(&a.direction, &a.origin_direction);
            (a, drift)
        })
        .collect();
    drifts.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    // Take top-3 as potential anisotropies
    drifts.iter().take(3).map(|(a, drift)| {
        Anisotropy {
            direction: a.direction.clone(),
            magnitude: *drift,
            anchor_count: 1, // simplified
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnchorId, EventId};

    fn a(id: u64, dir: Vec<f32>, density: u32) -> AnchorKey {
        let mut anchor = AnchorKey::new(AnchorId(id), format!("a{}", id), dir.clone(), density);
        anchor.origin_direction = dir;
        anchor
    }

    #[test]
    fn test_empty_snapshot_has_zero_tension() {
        let mut ecg = CognitiveEcg::new();
        ecg.snapshot(&[], &[], 0, Duration::seconds(3600));
        let report = ecg.report().unwrap();
        assert_eq!(report.current.tension, 0.0);
    }

    #[test]
    fn test_tension_increases_with_impacts() {
        let mut ecg = CognitiveEcg::new();
        let anchors = vec![a(1, vec![1.0, 0.0], 10)];
        let traces = vec![ImpactTrace {
            event_id: EventId(1),
            anchor_id: AnchorId(1),
            impact: 3.0,
            timestamp: Utc::now(),
        }];
        ecg.snapshot(&anchors, &traces, 1, Duration::seconds(3600));
        let report = ecg.report().unwrap();
        assert!(report.current.tension > 0.0);
    }
}

