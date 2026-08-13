//! field-memory v2's independent, contract-driven core.

pub mod error;
pub mod event;
pub mod geometry;
pub mod kernel;
pub mod numeric;
pub mod persist;
pub mod resolution;
pub mod response;
pub mod sample;
pub mod step;
pub mod transport;
pub mod version;

pub use error::{Result, V2Error};
pub use event::{
    CoordinateLogEntry, CoordinatePhase, Event, EventId, FieldState, Lifecycle, StepId,
};
pub use geometry::Direction;
pub use version::{BackendKind, EmbeddingIdentity, FieldVersion, SpaceKind};
