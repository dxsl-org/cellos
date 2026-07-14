//! BCM2835 peripheral interrupt controller — Raspberry Pi 3.
//!
//! Manages GPU (peripheral) IRQs 0–63: GPIO, UART, SPI, I2C, etc.
//! These reach Core 0 via the BCM2836 local controller's GPU IRQ pass-through
//! (IRQ_SRC_GPU bit 8 in Core 0 IRQ Source).
//!
//! GPIO bank IRQ numbers (BCM2835 legacy numbering):
//!   bank0 (pins 0–27):  IRQ 49 → Enable2 bit 17
//!   bank1 (pins 28–45): IRQ 50 → Enable2 bit 18
//!
//! P04: Init (disable all) + GPIO bank enable/disable + pending identify.
//! P05: GPIO bank IRQs enabled when gpio-bcm cell claims the MMIO region.

const IRQ_BASE: usize = 0x3F00_B200;
const IRQ_PENDING2: usize = IRQ_BASE + 0x08;
const IRQ_ENABLE1: usize = IRQ_BASE + 0x10;
const IRQ_ENABLE2: usize = IRQ_BASE + 0x14;
const IRQ_DISABLE1: usize = IRQ_BASE + 0x1C;
const IRQ_DISABLE2: usize = IRQ_BASE + 0x20;

/// GPIO IRQ numbers in BCM2835 legacy numbering.
pub const GPIO_BANK0_IRQ: u32 = 49; // pins 0–27 → Enable2 bit 17
pub const GPIO_BANK1_IRQ: u32 = 50; // pins 28–45 → Enable2 bit 18

#[inline(always)]
fn wr(addr: usize, val: u32) {
    // SAFETY: bare-metal MMIO; single-core boot, no concurrent writes.
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}
#[inline(always)]
fn rd(addr: usize) -> u32 {
    // SAFETY: same.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Initialize BCM2835 legacy IRQ controller.
///
/// Starts with all IRQs disabled; individual drivers enable theirs.
pub fn init() {
    wr(IRQ_DISABLE1, 0xFFFF_FFFF);
    wr(IRQ_DISABLE2, 0xFFFF_FFFF);
    super::bcm2836_irq::enable_gpu_irq_routing();
}

/// Enable a BCM2835 legacy peripheral IRQ (0–63).
pub fn enable_irq(irq: u32) {
    if irq < 32 {
        wr(IRQ_ENABLE1, 1 << irq);
    } else if irq < 64 {
        wr(IRQ_ENABLE2, 1 << (irq - 32));
    }
}

/// Disable a BCM2835 legacy peripheral IRQ (0–63).
pub fn disable_irq(irq: u32) {
    if irq < 32 {
        wr(IRQ_DISABLE1, 1 << irq);
    } else if irq < 64 {
        wr(IRQ_DISABLE2, 1 << (irq - 32));
    }
}

/// Identify a pending GPIO bank IRQ from the legacy controller.
///
/// Returns `Some(irq_number)` when a GPIO bank fires, `None` otherwise.
/// Called from `vi_aarch64_irq_handler` when `IRQ_SRC_GPU` is set.
pub fn identify_gpio_irq() -> Option<u32> {
    let p2 = rd(IRQ_PENDING2);
    if p2 & (1 << 17) != 0 {
        return Some(GPIO_BANK0_IRQ);
    }
    if p2 & (1 << 18) != 0 {
        return Some(GPIO_BANK1_IRQ);
    }
    None
}
