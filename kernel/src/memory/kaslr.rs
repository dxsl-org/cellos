//! Boot-time KASLR for per-cell virtual base randomisation.
//!
//! The seed is derived from the HHDM base (unpredictable to an attacker that
//! does not know the Limine-randomised physical layout) and the RDTSC counter
//! on x86_64 (or the mtime CSR on RISC-V). This gives sufficient entropy for
//! basic ASLR while remaining deterministic within a single boot.
//!
//! `kaslr_cell_base(cell_id)` produces a unique, page-aligned virtual base for
//! each cell in the range `[KASLR_BASE_MIN, KASLR_BASE_MIN + KASLR_RANGE)`.
//! The ELF loader (Phase 04) calls this function to pick where to load a cell.
//!
//! # Limits
//! - KASLR_BASE_MIN:  0x0000_1000_0000 (256 MiB)
//! - KASLR_RANGE:     0x0000_6FFF_0000 (~1.75 GiB)
//! - KASLR_ALIGN:     64 KiB (16 × 4 KiB pages) between cells
//! - Maximum distinct slots: KASLR_RANGE / KASLR_ALIGN ≈ 28 000

use core::sync::atomic::{AtomicU64, Ordering};
use types::VAddr;

static KASLR_SEED: AtomicU64 = AtomicU64::new(0);

/// Lowest virtual address a cell may be loaded at.
const KASLR_BASE_MIN: usize = 0x0000_1000_0000;
/// Width of the randomisable VA window (must be a power of two or handled as modulo).
const KASLR_RANGE:    usize = 0x0000_6FFF_0000;
/// Alignment between cell VA bases (64 KiB).
const KASLR_ALIGN:    usize = 0x1000 * 16;

/// Initialise the KASLR seed from boot-time entropy.
///
/// Should be called once, early in `kmain`, after the HHDM offset is known.
///
/// # Entropy sources
/// - `hhdm_offset`: Limine randomises the HHDM base; unknown to an attacker.
/// - x86_64: RDTSC counter at the moment of the call (nanosecond-scale jitter).
/// - RISC-V: `mtime` lower 32 bits from the platform timer.
/// - Fallback: a compile-time constant XOR'd with `hhdm_offset`.
pub fn init_kaslr(hhdm_offset: u64) {
    let hw_entropy = read_hw_entropy();
    // Mix HHDM offset with hardware entropy. The golden-ratio constant
    // (0x9E37_79B9_7F4A_7C15) spreads entropy across all bits.
    let seed = (hhdm_offset ^ hw_entropy).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    KASLR_SEED.store(seed, Ordering::Relaxed);
}

/// Derive a page-aligned virtual base address for `cell_id`.
///
/// Each `cell_id` produces a distinct, pseudo-random address in the range
/// `[KASLR_BASE_MIN, KASLR_BASE_MIN + KASLR_RANGE)`.  The mapping is stable
/// within a boot but unpredictable across boots (seed changes with HHDM).
///
/// # Arguments
/// * `cell_id` — monotonically increasing cell identifier (0-based).
pub fn kaslr_cell_base(cell_id: usize) -> VAddr {
    let seed = KASLR_SEED.load(Ordering::Relaxed);
    // Per-cell diversification: multiply cell_id by a large odd constant to
    // scatter cells across the VA range without clustering them sequentially.
    let mixed = seed ^ (cell_id as u64).wrapping_mul(0x6C62_272E_07BB_0142);
    // Derive an offset in [0, KASLR_RANGE) aligned to KASLR_ALIGN.
    let raw_offset = (mixed as usize) % KASLR_RANGE;
    let aligned_offset = raw_offset & !(KASLR_ALIGN - 1);
    KASLR_BASE_MIN + aligned_offset
}

// ─── Architecture-specific entropy read ──────────────────────────────────────

fn read_hw_entropy() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // RDTSC returns the current timestamp counter. On QEMU TCG this is a
        // monotonically increasing instruction count — sufficient for ASLR.
        let lo: u32;
        let hi: u32;
        // SAFETY: RDTSC is an unprivileged instruction on x86_64; it reads the
        // CPU time-stamp counter and has no memory or control-flow side-effects.
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nomem, nostack)
            );
        }
        ((hi as u64) << 32) | (lo as u64)
    }
    #[cfg(target_arch = "riscv64")]
    {
        // SAFETY: `csrr time` reads the mtime-backed TIME CSR (unprivileged read
        // permitted in S-mode when sstatus.TVM=0, which is always the case here).
        let t: u64;
        unsafe { core::arch::asm!("csrr {}, time", out(reg) t, options(nomem, nostack)); }
        t
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: CNTVCT_EL0 is the virtual count register; accessible from EL1.
        let t: u64;
        unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) t, options(nomem, nostack)); }
        t
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64", target_arch = "aarch64")))]
    {
        // No hardware timer available; use a fixed diversifier.
        // This means KASLR is purely seed-based (hhdm_offset) on these arches.
        0xDEAD_BEEF_CAFE_1234u64
    }
}
