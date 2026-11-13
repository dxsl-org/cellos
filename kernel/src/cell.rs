//! Cell Metadata, Registry, and Lifecycle Management.
//!
//! Complies with Agent Manifest "LINH HỒN: Quản lý Metadata, Registry, Dependency".

pub mod cap_registry;
pub mod cspace;
pub mod hotswap;
pub mod init_death;
pub mod metadata;
pub mod retirement_verdict;
pub mod service_registry;
pub mod state_stash;

// Re-export core types for convenience
pub use metadata::CellHeader;
