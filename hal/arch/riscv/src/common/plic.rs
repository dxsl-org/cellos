//! PLIC (Platform-Level Interrupt Controller) Driver for RISC-V.
//! Reference: https://github.com/riscv/riscv-plic-spec/blob/master/riscv-plic.adoc

pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_PRIORITY_BASE: usize = 0x0;
pub const PLIC_PENDING_BASE: usize = 0x1000;
pub const PLIC_ENABLE_BASE: usize = 0x2000;
pub const PLIC_THRESHOLD_AND_CLAIM_BASE: usize = 0x20_0000;

// Context 0 is usually Hart 0 M-mode (often skipped in Linux/S-mode kernels if SBI handles M-mode)
// Context 1 is Hart 0 S-mode.
// For QEMU virt:
// Hart 0 M-mode: Context 0
// Hart 0 S-mode: Context 1
// Hart 1 M-mode: Context 2
// Hart 1 S-mode: Context 3
// ...
// We assume Single Core (Hart 0) S-mode for now -> Context 1.

pub struct Plic {
    base: usize,
}

impl Plic {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    /// Set priority for a specific IRQ.
    /// Priority: 0 (disabled) to 7 (highest).
    pub fn set_priority(&self, irq: u32, priority: u32) {
        let addr = self.base + PLIC_PRIORITY_BASE + (irq as usize) * 4;
        unsafe {
            let ptr = addr as *mut u32;
            ptr.write_volatile(priority);
        }
    }

    /// Enable interrupt for a specific Context.
    pub fn enable(&self, context: usize, irq: u32) {
        let addr = self.base + PLIC_ENABLE_BASE + (context * 0x80) + ((irq as usize / 32) * 4);
        let mask = 1 << (irq % 32);
        unsafe {
            let ptr = addr as *mut u32;
            let current = ptr.read_volatile();
            ptr.write_volatile(current | mask);
        }
    }

    /// Set priority threshold for a specific Context.
    /// Interrupts <= threshold are masked.
    pub fn set_threshold(&self, context: usize, threshold: u32) {
        let addr = self.base + PLIC_THRESHOLD_AND_CLAIM_BASE + (context * 0x1000);
        unsafe {
            let ptr = addr as *mut u32;
            ptr.write_volatile(threshold);
        }
    }

    /// Claim an interrupt for a specific Context.
    /// Returns the IRQ number, or 0 if none.
    pub fn claim(&self, context: usize) -> u32 {
        let addr = self.base + PLIC_THRESHOLD_AND_CLAIM_BASE + (context * 0x1000) + 4;
        unsafe {
            let ptr = addr as *mut u32;
            ptr.read_volatile()
        }
    }

    /// Complete an interrupt for a specific Context.
    pub fn complete(&self, context: usize, irq: u32) {
        let addr = self.base + PLIC_THRESHOLD_AND_CLAIM_BASE + (context * 0x1000) + 4;
        unsafe {
            let ptr = addr as *mut u32;
            ptr.write_volatile(irq);
        }
    }
}

// Global PLIC instance
pub static PLIC: Plic = Plic::new(PLIC_BASE);

/// Initialize PLIC for Hart 0 S-Mode (Context 1)
pub fn init() {
    // 1. Set threshold to 0 (accept all)
    PLIC.set_threshold(1, 0);

    // 2. Enable VirtIO interrupts (1..=8)
    // QEMU virt machine usually maps VirtIO 1-8 to IRQ 1-8.
    for irq in 1..=8 {
        PLIC.set_priority(irq, 1);
        PLIC.enable(1, irq);
    }
    // Enable UART0 (IRQ 10 on QEMU Virt)
    PLIC.set_priority(10, 1);
    PLIC.enable(1, 10);

    // log::info!("PLIC initialized for Hart 0 S-Mode (Context 1)");
}
