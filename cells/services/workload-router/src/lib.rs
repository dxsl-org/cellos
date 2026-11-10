//! Spec 26 design skeleton — NOT wired and fail-open (owner decision D1, 2026-11-10).
//! Excluded from the workspace and every image; no enforcement may rely on it. Kept as
//! source for controlling-plan P08 (`.agents/261011-1200-cell-contract-authority`).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod admission;
pub mod endpoint;
pub mod queue;
pub mod routing;
pub mod telemetry;

pub use admission::{check_admission, AdmissionError, OriginProof};
pub use endpoint::{EndpointKind, EndpointRegistry, ReadyEvidence, WorkloadMember};
pub use queue::{BoundedQueue, QueuedItem};
pub use routing::{RoutingError, WeightedRouter};
pub use telemetry::ChallengeRound;
