//! PLIC (Platform-Level Interrupt Controller) Driver for RISC-V.
//! Reference: https://github.com/riscv/riscv-plic-spec/blob/master/riscv-plic.adoc

use core::sync::atomic::{AtomicUsize, Ordering};

pub const PLIC_PRIORITY_BASE: usize = 0x0;
pub const PLIC_PENDING_BASE: usize = 0x1000;
pub const PLIC_ENABLE_BASE: usize = 0x2000;
pub const PLIC_THRESHOLD_AND_CLAIM_BASE: usize = 0x20_0000;

// PLIC context numbering is SoC policy. The kernel resolves the current
// physical hart through `hal/soc/riscv` and passes the selected context into
// this shared register-access mechanism.

/// Runtime PLIC base address. Updated before `init()` via `set_plic_base()`.
static PLIC_RUNTIME_BASE: AtomicUsize = AtomicUsize::new(0);

/// Override the PLIC base address before `init()` is called (called from kernel
/// after DTB parsing populates `platform::PlatformInfo`).
pub fn set_plic_base(base: usize) {
    PLIC_RUNTIME_BASE.store(base, Ordering::Relaxed);
}

pub struct Plic;

impl Plic {
    pub const fn new() -> Self {
        Self
    }

    fn base() -> usize {
        let base = PLIC_RUNTIME_BASE.load(Ordering::Relaxed);
        assert!(
            base != 0,
            "PLIC base must be selected before register access"
        );
        base
    }

    /// Set priority for a specific IRQ.
    /// Priority: 0 (disabled) to 7 (highest).
    pub fn set_priority(&self, irq: u32, priority: u32) {
        let addr = Self::base() + PLIC_PRIORITY_BASE + (irq as usize) * 4;
        // SAFETY: `addr` points into the identity-mapped PLIC MMIO aperture
        // selected by `set_plic_base()`. The caller provides a device IRQ id.
        unsafe {
            (addr as *mut u32).write_volatile(priority);
        }
    }

    /// Enable interrupt for a specific Context.
    pub fn enable(&self, context: usize, irq: u32) {
        let addr = Self::base() + PLIC_ENABLE_BASE + (context * 0x80) + ((irq as usize / 32) * 4);
        let mask = 1 << (irq % 32);
        // SAFETY: same MMIO aperture contract as `set_priority`; read-modify-write
        // is required by the PLIC enable register layout.
        unsafe {
            let ptr = addr as *mut u32;
            ptr.write_volatile(ptr.read_volatile() | mask);
        }
    }

    /// Set priority threshold for a specific Context.
    /// Interrupts <= threshold are masked.
    pub fn set_threshold(&self, context: usize, threshold: u32) {
        let addr = Self::base() + PLIC_THRESHOLD_AND_CLAIM_BASE + (context * 0x1000);
        // SAFETY: `addr` points at this context's threshold register inside the
        // configured PLIC MMIO region.
        unsafe {
            (addr as *mut u32).write_volatile(threshold);
        }
    }

    /// Claim an interrupt for a specific Context.
    /// Returns the IRQ number, or 0 if none.
    pub fn claim(&self, context: usize) -> u32 {
        let addr = Self::base() + PLIC_THRESHOLD_AND_CLAIM_BASE + (context * 0x1000) + 4;
        // SAFETY: `addr` points at this context's claim register inside the
        // configured PLIC MMIO region.
        unsafe { (addr as *mut u32).read_volatile() }
    }

    /// Complete an interrupt for a specific Context.
    pub fn complete(&self, context: usize, irq: u32) {
        let addr = Self::base() + PLIC_THRESHOLD_AND_CLAIM_BASE + (context * 0x1000) + 4;
        // SAFETY: `addr` points at this context's completion register inside the
        // configured PLIC MMIO region.
        unsafe {
            (addr as *mut u32).write_volatile(irq);
        }
    }
}

// Global PLIC instance (zero-size; all state in PLIC_RUNTIME_BASE).
pub static PLIC: Plic = Plic::new();

/// Initialize the active S-mode PLIC context and enable the provided IRQs.
///
/// Uses the base address set by `set_plic_base()` — call that first from the
/// kernel after platform discovery. Missing or empty runtime IRQ lists fail
/// closed by enabling no device IRQs.
pub fn init(context: usize, irqs: &[u32]) {
    PLIC.set_threshold(context, 0);
    for &irq in irqs {
        if irq == 0 {
            continue;
        }
        PLIC.set_priority(irq, 1);
        PLIC.enable(context, irq);
    }
}

/// Claim the highest-priority pending IRQ from one S-mode context.
pub fn claim(context: usize) -> Option<u32> {
    let irq = PLIC.claim(context);
    (irq != 0).then_some(irq)
}

/// Notify the PLIC that `context` has finished handling `irq`.
pub fn complete(context: usize, irq: u32) {
    if irq == 0 {
        return;
    }
    PLIC.complete(context, irq);
}
