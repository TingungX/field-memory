use crate::physics::impact;
use crate::math::moving_avg_direction;
use crate::types::{AnchorKey, Event, ImpactTrace, Vector};

/// Passive 1: value initialization.
/// Return top anchors by density (core concepts for session init).
pub fn value_init(anchors: &[AnchorKey], top_n: usize) -> Vec<&AnchorKey> {
    let mut sorted: Vec<&AnchorKey> = anchors.iter().collect();
    sorted.sort_by(|a, b| b.density.cmp(&a.density));
    sorted.truncate(top_n);
    sorted
}

/// Passive 2: associative recall (concept-level, no events).
/// Query broadcast → return anchors sorted by impact.
pub fn associate<'a>(anchors: &'a [AnchorKey], query_dir: &Vector, threshold: f32) -> Vec<(&'a AnchorKey, f32)> {
    let mut hits: Vec<(&AnchorKey, f32)> = anchors.iter()
        .map(|a| (a, impact(a, query_dir)))
        .filter(|(_, imp)| *imp > threshold)
        .collect();
    hits.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

/// Result of active recall
#[derive(Debug, Clone)]
pub struct RecallResult {
    /// Top anchors by impact
    pub anchors: Vec<(String, f32)>,
    /// Retrieved events from impact traces
    pub events: Vec<(String, String, f32)>, // (event_text, anchor_label, impact)
}

/// Active: recall_memory.
/// Query broadcast → top anchors → trace lookup → return events.
pub fn recall(
    anchors: &[AnchorKey],
    traces: &[ImpactTrace],
    events: &[Event],
    query_dir: &Vector,
    top_k: usize,
    threshold: f32,
) -> RecallResult {
    // 1. Broadcast: rank anchors by impact
    let mut hits: Vec<(&AnchorKey, f32)> = anchors.iter()
        .map(|a| (a, impact(a, query_dir)))
        .filter(|(_, imp)| *imp > threshold)
        .collect();
    hits.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(top_k);

    let anchor_labels: Vec<(String, f32)> = hits.iter()
        .map(|(a, imp)| (a.label.clone(), *imp))
        .collect();

    // 2. Trace lookup: for each hit anchor, find associated events
    let mut event_results: Vec<(String, String, f32)> = vec![];

    for (anchor, _anchor_impact) in &hits {
        let mut anchor_traces: Vec<&ImpactTrace> = traces.iter()
            .filter(|t| t.anchor_id == anchor.id)
            .collect();
        anchor_traces.sort_by(|a, b| b.impact.partial_cmp(&a.impact).unwrap_or(std::cmp::Ordering::Equal));

        for trace in anchor_traces.iter().take(10) {
            if let Some(event) = events.iter().find(|e| e.id == trace.event_id) {
                event_results.push((
                    event.text.clone(),
                    anchor.label.clone(),
                    trace.impact,
                ));
            }
        }
    }

    // 3. Sort all events by impact, dedup
    event_results.sort_by(|(_, _, a), (_, _, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    event_results.dedup_by(|(t1, _, _), (t2, _, _)| t1 == t2);

    RecallResult {
        anchors: anchor_labels,
        events: event_results,
    }
}

/// Memory consolidation: light relaxation from query (recall as a lightweight event).
/// Increases density of strongly-hit anchors.
pub fn consolidate_from_recall(anchors: &mut [AnchorKey], query_dir: &Vector, threshold: f32) {
    for anchor in anchors.iter_mut() {
        let imp = impact(anchor, query_dir);
        if imp > threshold {
            anchor.density += 1;
            anchor.update_mechanics();
            // Slight direction pull toward query
            let rate = 0.01 / (anchor.density as f32);
            anchor.direction = moving_avg_direction(
                &anchor.direction, query_dir, rate,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnchorId, EventId};

    fn a(id: u64, label: &str, dir: Vec<f32>, density: u32) -> AnchorKey {
        AnchorKey::new(AnchorId(id), label.into(), dir, density)
    }

    fn e(id: u64, dir: Vec<f32>, text: &str) -> Event {
        Event::new(EventId(id), dir, text.into())
    }

    #[test]
    fn test_value_init_returns_top_density() {
        let anchors = vec![
            a(1, "core", vec![1.0, 0.0], 100),
            a(2, "edge", vec![0.0, 1.0], 3),
            a(3, "mid", vec![1.0, 1.0], 50),
        ];
        let result = value_init(&anchors, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].label, "core");
        assert_eq!(result[1].label, "mid");
    }

    #[test]
    fn test_associate_returns_relevant() {
        let anchors = vec![
            a(1, "rust", vec![1.0, 0.0], 10),
            a(2, "python", vec![-1.0, 0.0], 10),
        ];
        let query = vec![1.0, 0.1]; // close to rust
        let hits = associate(&anchors, &query, 0.1);
        assert!(hits.len() >= 1);
        assert_eq!(hits[0].0.label, "rust");
    }

    #[test]
    fn test_recall_returns_events_from_traces() {
        let anchors = vec![a(1, "rust", vec![1.0, 0.0], 10)];
        let events = vec![
            e(1, vec![1.0, 0.0], "Rust is great"),
            e(2, vec![0.0, 1.0], "unrelated"),
        ];
        let traces = vec![
            ImpactTrace { event_id: EventId(1), anchor_id: AnchorId(1), impact: 3.0, timestamp: chrono::Utc::now() },
        ];
        let query = vec![1.0, 0.0];
        let result = recall(&anchors, &traces, &events, &query, 10, 0.1);
        assert!(result.events.len() >= 1);
        assert!(result.events.iter().any(|(text, _, _)| text == "Rust is great"));
    }
}
