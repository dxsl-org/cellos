#![no_std]
//! ViMte — cross-arch Memory Tagging trait.
//!
//! ⚠️  MTE is HARDENING, NOT a security boundary. It detects use-after-free
//! and buffer overflow probabilistically (1/16 tag collision chance).
//! TikTag (2024) demonstrates speculative side-channel bypass. Do NOT treat
//! MTE as an isolation primitive; it complements, not replaces, Rust safety.
//!
//! ## Current implementations
//! - `aarch64`: MTE2 (ARMv8.5-A), synchronous or async tag-fault mode.
//! - All other targets: `NoMte` stub (compile-time no-op).
//!
//! A second implementation (e.g., RISC-V Zimt, draft spec v0.2) would justify
//! the trait abstraction at the hardware interface layer; the stub keeps
//! non-aarch64 builds clean in the meantime.

/// Memory Tagging hardware abstraction.
///
/// Implementors must be zero-sized and stateless; tag operations act directly
/// on hardware registers and Normal-Tagged memory granules.
pub trait ViMte {
    /// Returns `true` when MTE2 (full checking) hardware is available.
    ///
    /// Checks `ID_AA64PFR1_EL1.MTE` ≥ 2 on AArch64.
    fn is_available() -> bool;

    /// Tag all 16-byte granules in `ptr..ptr+len` with `color` (4-bit, 0–15).
    ///
    /// # Safety
    /// - `ptr` must be 16-byte aligned.
    /// - `len` must be a non-zero multiple of 16.
    /// - The entire range `ptr..ptr+len` must be valid, allocated Normal-Tagged
    ///   memory owned exclusively by the caller.
    unsafe fn tag_region(ptr: *mut u8, len: usize, color: u8);

    /// Read the 4-bit allocation tag for the 16-byte granule containing `ptr`.
    ///
    /// # Safety
    /// - `ptr` must point into a valid Normal-Tagged memory region.
    unsafe fn get_tag(ptr: *const u8) -> u8;

    /// Switch tag-check mode for EL0 and EL1.
    ///
    /// `true`  → synchronous fault on mismatch (default, lowest latency for
    ///            deterministic error detection).
    /// `false` → asynchronous fault (lower overhead; fault delivered later).
    fn set_check_mode(synchronous: bool);
}

/// No-op stub for non-AArch64 targets.
///
/// Allows code that is generic over `ViMte` to compile everywhere without
/// `#[cfg(target_arch = "aarch64")]` at every call site.
#[cfg(not(target_arch = "aarch64"))]
pub struct NoMte;

#[cfg(not(target_arch = "aarch64"))]
impl ViMte for NoMte {
    fn is_available() -> bool {
        false
    }

    unsafe fn tag_region(_ptr: *mut u8, _len: usize, _color: u8) {}

    unsafe fn get_tag(_ptr: *const u8) -> u8 {
        0
    }

    fn set_check_mode(_synchronous: bool) {}
}
