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
