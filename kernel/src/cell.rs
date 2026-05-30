//! Cell Metadata, Registry, and Lifecycle Management.
//!
//! Complies with Agent Manifest "LINH HỒN: Quản lý Metadata, Registry, Dependency".

pub mod cap_registry;
pub mod hotswap;
pub mod metadata;
pub mod registry;
pub mod state_stash;

// Re-export core types for convenience
pub use metadata::CellHeader;
pub use registry::{CellNode, CellState};
