//! RISC-V PMP (Physical Memory Protection) region definitions.
//!
//! # Architecture Constraint
//!
//! PMP CSRs (`pmpaddr*`, `pmpcfg*`) are **M-mode-only**.  Writing them from
//! S-mode raises an Illegal Instruction trap.  ViCell runs in S-mode under
//! OpenSBI, so this module **cannot write PMP registers at runtime**.
//!
//! A future M-mode firmware shim supplies its selected SoC/board regions. This
//! architecture module exposes only encoding mechanics and permission bits.
//!
//! # NAPOT encoding
//! `pmpaddr = (base >> 2) | (size/8 - 1)`
//! Requirements: `base` aligned to `size`; `size` must be a power of two ≥ 8.

/// Permission bits for PMP config entries (`pmpcfg` byte per region).
pub mod perm {
    /// Read permission.
    pub const R: u8 = 0b001;
    /// Write permission.
    pub const W: u8 = 0b010;
    /// Execute permission.
    pub const X: u8 = 0b100;
    /// Read + Write.
    pub const RW: u8 = R | W;
    /// Read + Execute.
    pub const RX: u8 = R | X;
    /// Read + Write + Execute.
    pub const RWX: u8 = R | W | X;
    /// Addressing mode: NAPOT (naturally aligned power-of-two).
    pub const A_NAPOT: u8 = 0b11 << 3;
    /// Lock bit: entry enforced on M-mode too; cannot be modified until reset.
    /// Under Smepmp with MML=1, locked entries become M-mode-only rules.
    pub const L: u8 = 1 << 7;
}

/// Compute the NAPOT `pmpaddr` value for a region.
///
/// `base` must be aligned to `size`; `size` must be a power of two ≥ 8.
pub const fn napot_addr(base: usize, size: usize) -> usize {
    (base >> 2) | (size / 8 - 1)
}
