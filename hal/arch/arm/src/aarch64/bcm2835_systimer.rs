//! BCM2835 System Timer driver — Raspberry Pi 3.
//!
//! The BCM2835 has a 64-bit free-running 1 MHz counter and four compare
//! registers (C0–C3). C1 and C3 are available (C0/C2 are used by the
//! VideoCore GPU firmware). A compare match raises IRQ 1 (C1) or IRQ 3
//! (C3) in the BCM2835 peripheral interrupt controller, which then appears
//! as bit 8 (GPU IRQ) in BCM2836 CORE0_IRQ_SOURCE. This path IS fully
//! connected in QEMU raspi3b, unlike the BCM2836 ARM-local timer routing.
//!
//! We use C1 at a 10 ms period (10 000 ticks @ 1 MHz) to drive the kernel
//! scheduler tick. The IRQ is acknowledged by writing 1 to CS bit 1, then
//! re-armed by advancing C1 by PERIOD ticks.
//!
//! Interrupt number: IRQ 1 → Enable1 bit 1 in BCM2835 interrupt controller.

const SYSTIMER_BASE: usize = 0x3F00_3000;
const SYSTIMER_CS: usize = SYSTIMER_BASE; // control/status (w1c bits 0–3)
const SYSTIMER_CLO: usize = SYSTIMER_BASE + 0x04; // free-running counter, lower 32 bits
const SYSTIMER_C1: usize = SYSTIMER_BASE + 0x10; // compare register 1

/// 10 ms @ 1 MHz = 10 000 ticks.
const PERIOD: u32 = 10_000;

const IRQ_BASE: usize = 0x3F00_B200;
const IRQ_PENDING1: usize = IRQ_BASE + 0x04; // pending bits for IRQs 0–31
const IRQ_ENABLE1: usize = IRQ_BASE + 0x10; // enable  bits for IRQs 0–31

#[inline(always)]
fn wr(addr: usize, val: u32) {
    // SAFETY: bare-metal MMIO; identity-mapped before paging.
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}
#[inline(always)]
fn rd(addr: usize) -> u32 {
    // SAFETY: bare-metal MMIO; identity-mapped before paging.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Initialise the BCM2835 system timer to fire every 10 ms on C1.
///
/// Call from `timer::init()` (board-rpi3 path) instead of CNTP setup.
pub fn init() {
    // Clear any stale C1 match flag.
    wr(SYSTIMER_CS, 1 << 1);
    // Set compare to current counter + PERIOD.
    let now = rd(SYSTIMER_CLO);
    wr(SYSTIMER_C1, now.wrapping_add(PERIOD));
    // Enable C1 interrupt in the BCM2835 peripheral interrupt controller.
    // IRQ 1 = bit 1 of Enable_IRQs_1.
    wr(IRQ_ENABLE1, 1 << 1);
}

/// Acknowledge C1 match and re-arm for the next period.
///
/// Call from the IRQ handler after detecting a C1 match.
/// Advances the compare register relative to the PREVIOUS fire time
/// so drift does not accumulate.
pub fn ack_and_rearm() {
    // Read the compare value that just fired (not CLO, to avoid drift).
    let prev = rd(SYSTIMER_C1);
    // Clear C1 match flag (w1c).
    wr(SYSTIMER_CS, 1 << 1);
    // Set next compare = previous fire time + PERIOD.
    wr(SYSTIMER_C1, prev.wrapping_add(PERIOD));
}

/// Check whether the C1 compare match IRQ is pending.
///
/// Used by `vi_aarch64_irq_handler` to identify BCM2835 timer IRQs
/// within the GPU IRQ path (bit 8 of CORE0_IRQ_SOURCE).
#[inline]
pub fn is_c1_pending() -> bool {
    rd(IRQ_PENDING1) & (1 << 1) != 0
}
