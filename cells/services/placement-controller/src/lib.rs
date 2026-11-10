//! Spec 26 design skeleton — NOT wired and fail-open (owner decision D1, 2026-11-10).
//! Excluded from the workspace and every image; no enforcement may rely on it. Kept as
//! source for controlling-plan P08 (`.agents/261011-1200-cell-contract-authority`).

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
