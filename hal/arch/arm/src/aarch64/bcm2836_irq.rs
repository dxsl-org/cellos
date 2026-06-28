//! BCM2836 ARM-local interrupt controller — Raspberry Pi 3.
//!
//! Manages per-core ARM-local IRQs: ARM Generic Timer PPIs (26/27/30) and
//! Mailboxes. Peripheral IRQs (GPIO, UART, SPI, I2C) arrive via the BCM2835
//! legacy controller and appear as bit 8 (GPU IRQ) in Core0 IRQ Source.
//!
//! Only Core 0 is managed (G1 single-core). Multi-core support is G3.
//!
//! Reference: BCM2836 ARM-local peripherals datasheet §4 (Broadcom).

const LOCAL_CTRL_BASE: usize = 0x4000_0000;

// BCM2836 QA7_rev3.4 datasheet §4 register map (confirmed by Linux + QEMU sources):
//   0x40 = Core 0 Timers Interrupt Control  (bits[3:0]=IRQ, bits[7:4]=FIQ routing per timer)
//   0x60 = Core 0 IRQ Source                (read-only: which source fired)
//   0x70 = Core 0 FIQ Source               (read-only)
// NOTE: offset 0x24 is "Local Interrupt 1 routing" — NOT the core timer control.
//       Old code used 0x24/0x40/0x60 which were all wrong (off by 0x1C).
const CORE0_TIMERS_IRQ: usize = LOCAL_CTRL_BASE + 0x40; // enable timer IRQs → Core 0 IRQ line
const CORE0_IRQ_SOURCE: usize = LOCAL_CTRL_BASE + 0x60; // read: which IRQ fired on Core 0
const CORE0_FIQ_SOURCE: usize = LOCAL_CTRL_BASE + 0x70; // FIQ source (should be 0 — we use IRQ)

// Bits in CORE0_TIMERS_IRQ
const TIMER_NS_PHYS_IRQ: u32 = 1 << 1; // nCNTPNSIRQ (EL1 Non-secure physical, PPI 30)
const TIMER_HP_IRQ:      u32 = 1 << 2; // nCNTHPIRQ  (EL2 Hypervisor physical,  PPI 26)

// Bits in CORE0_IRQ_SOURCE (exported for trap.rs dispatch)
pub const IRQ_SRC_TIMER_NS:  u32 = 1 << 1; // Non-secure physical timer fired
pub const IRQ_SRC_TIMER_HP:  u32 = 1 << 2; // Hypervisor physical timer fired
/// GPU (peripheral) IRQ: routes BCM2835 legacy controller pending IRQs.
pub const IRQ_SRC_GPU:        u32 = 1 << 8;

#[inline(always)]
fn wr(addr: usize, val: u32) {
    // SAFETY: bare-metal MMIO; no concurrent writes during single-core boot.
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}
#[inline(always)]
fn rd(addr: usize) -> u32 {
    // SAFETY: same.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Initialize BCM2836 local IRQ controller for Core 0.
///
/// Enables the ARM physical timer PPI that matches the current EL:
/// - EL2: hypervisor physical timer (CNTHP, PPI 26)
/// - EL1: non-secure physical timer (CNTP,  PPI 30)
///
/// Call from `AArch64Arch::init()` instead of `gic::init()` when `board-rpi3`.
///
/// Note: TIMER_NS_PHYS_IRQ (bit 1) and TIMER_HP_IRQ (bit 2) are left disabled
/// because QEMU 10.x raspi3b does not wire the ARM generic timer PPIs through
/// BCM2836 to the CPU nIRQ line.  The scheduler tick uses the BCM2835 system
/// timer (C1, 1 MHz), routed via the BCM2835 peripheral IRQ controller which
/// IS properly connected in QEMU.  Set 0 here; bcm2835_systimer::init() will
/// enable C1 in the BCM2835 controller.
pub fn init() {
    // Disable FIQ routing — all IRQs use the IRQ line.
    wr(CORE0_FIQ_SOURCE, 0);
    // Disable all local timer IRQs (ARM generic timer PPIs); BCM2835
    // system timer provides the tick via GPU IRQ (bit 8) instead.
    wr(CORE0_TIMERS_IRQ, 0);
}

/// Enable routing of the GPU (BCM2835 peripheral) IRQ to Core 0.
///
/// Called from `bcm2835_legacy_irq::init()` to document the intent;
/// the routing is automatic on BCM2836 (no extra register write needed).
pub fn enable_gpu_irq_routing() {
    // GPU IRQs appear in CORE0_IRQ_SOURCE bit 8 automatically once the
    // BCM2835 legacy controller has an interrupt pending.  This function
    // exists as a call-site annotation, not a register write.
}

/// Add a timer IRQ enable bit without clearing existing bits.
///
/// Used by `bcm2835_legacy_irq` to add the GPU routing bit independently.
pub fn add_timer_enable(bits: u32) {
    let prev = rd(CORE0_TIMERS_IRQ);
    wr(CORE0_TIMERS_IRQ, prev | bits);
}

/// Read Core 0 IRQ source register (non-destructive status).
///
/// Returns a bitmask; check `IRQ_SRC_TIMER_NS`, `IRQ_SRC_TIMER_HP`, `IRQ_SRC_GPU`.
#[inline]
pub fn irq_source() -> u32 {
    rd(CORE0_IRQ_SOURCE)
}
