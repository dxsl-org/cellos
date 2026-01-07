//! Cell metadata and lifecycle management.

use crate::*;

/// Cell metadata header (embedded in `.cell_info` section).
#[repr(C)]
pub struct CellHeader {
    /// Cell name.
    pub name: &'static str,
    /// Cell version.
    pub version: SemVer,
    /// Dependencies (name, version requirement).
    pub deps: &'static [(&'static str, &'static str)],
}

/// Cell node in the dependency graph.
pub struct CellNode {
    /// Unique identifier.
    pub id: CellId,
    /// Cells this one imports from.
    pub imports: alloc::vec::Vec<CellId>,
    /// Cells that import from this one.
    pub exported_to: alloc::vec::Vec<CellId>,
    /// Current state.
    pub state: CellState,
    /// Base address in memory.
    pub base_addr: VAddr,
    /// Size in bytes.
    pub size: usize,
}

extern crate alloc;
