pub mod types;
pub mod math;
pub mod embed;
pub mod physics;
pub mod cycle;
pub mod recall;
pub mod init;
pub mod paradigm;
pub mod ecg;
pub mod engine;
pub mod persist;

pub use engine::DseEngine;
pub use types::*;

/// Core configuration — 5 parameters only
pub struct DseCoreParams {
    pub vector_dim: usize,
    pub event_window_secs: u64,
    pub damping_base: f32,
    pub stiffness_base: f32,
    pub convergence_threshold: f32,
}

impl Default for DseCoreParams {
    fn default() -> Self {
        Self {
            vector_dim: 32,
            event_window_secs: 3600,
            damping_base: 0.5,
            stiffness_base: 1.0,
            convergence_threshold: 0.001,
        }
    }
}

