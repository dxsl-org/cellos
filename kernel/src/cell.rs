//! Cell Metadata, Registry, and Lifecycle Management.
//!
//! Complies with Agent Manifest "LINH HỒN: Quản lý Metadata, Registry, Dependency".

pub mod cap_registry;
pub mod cspace;
pub mod hotswap;
pub mod metadata;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub mod provenance_selftest;
pub mod registry;
pub mod retirement_verdict;
pub mod service_registry;
pub mod state_stash;

// Re-export core types for convenience
pub use metadata::CellHeader;
pub use registry::{CellNode, CellState};
