#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod budget_client;
pub mod placement;
pub mod policy;
pub mod reconcile;
pub mod scaling;

pub use budget_client::{BudgetAuthorityClient, BudgetClientError, ResourceVector};
pub use placement::{rank_nodes, CandidateNode};
pub use policy::AutoscaleDomainPolicy;
pub use reconcile::{MemberLifecycleState, ReconcileTarget, ReconciliationTracker};
pub use scaling::{ScaleDecision, ScalingEngine};
