//! Platform peripheral discovery via DTB.
//!
//! The architecture's `crate::arch::boot::platform::init(dtb_ptr)` runs in
//! `kmain` before any driver init (riscv64 parses the DTB, aarch64 takes the
//! board backend's facts). All drivers read MMIO addresses via
//! `platform::with(|p| p.xxx)`. riscv64 falls back to the SoC profile's
//! defaults when the DTB is absent or a compatible node is not found.
//!
//! Call ordering invariant: `init` → `uart::init` → `hal::ARCH.init` (which calls
//! `plic::init` internally). Any call to `with` before `init` panics.

mod boot_once;

use boot_once::BootOnce;

// ── Public types ───────────────────────────────────────────────────────────────

/// A VirtIO MMIO device found in the DTB.
#[derive(Clone, Copy)]
pub struct VirtioEntry {
    pub base: usize,
    pub irq: u32,
}

/// Platform peripheral layout populated once from the DTB at early boot.
#[derive(Clone)]
pub struct PlatformInfo {
    pub uart_base: usize,
    pub uart_irq: u32,
    pub plic_base: usize,
    /// Mapped region size for identity-map range covering all PLIC registers.
    pub plic_size: usize,
    pub clint_base: usize,
    /// VirtIO MMIO slots from DTB (up to 8). `None` = slot unused.
    pub virtio_mmio: [Option<VirtioEntry>; 8],
    /// Goldfish RTC MMIO base (0 = not found in DTB, using default).
    pub rtc_base: usize,
}

// ── Storage ────────────────────────────────────────────────────────────────────

static PLATFORM: BootOnce<PlatformInfo> = BootOnce::new();

/// Publish immutable platform data while the boot CPU is the only running core.
///
/// Spinlocks are forbidden on this path: ARM64 reaches it before the MMU has
/// assigned Normal memory attributes, so LL/SC may abort on real hardware.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) unsafe fn publish_boot(info: PlatformInfo) {
    // SAFETY: every architecture calls `platform::init` once from `kmain`
    // before interrupts, paging, or secondary-core startup.
    unsafe { PLATFORM.initialize(info) };
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Borrow the platform info. Panics if `init` was not called.
pub fn with<R>(f: impl FnOnce(&PlatformInfo) -> R) -> R {
    f(PLATFORM
        .get()
        .expect("[platform] platform::init not called before platform::with"))
}
