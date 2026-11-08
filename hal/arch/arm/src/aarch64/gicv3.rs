// SPDX-License-Identifier: MPL-2.0
//! ARM Generic Interrupt Controller version 3 (GICv3) driver.
//!
//! Provides distributor, redistributor, and system-register CPU interface (ICC)
//! management for AArch64 (Phase 02c). Supports both Group 1 (IRQ) and Group 0 (FIQ).
use hal_arch_trait::{vi_gpio_notify_irq, vi_ipi_service, vi_timer_tick};
/// Trait defining the physical base addresses for a board's GICv3 controller.
pub trait GicV3Layout {
    /// Distributor MMIO base (GICD).
    const DISTRIBUTOR_BASE: usize;
    /// Redistributors MMIO base (GICR) for all cores.
    const REDISTRIBUTOR_BASE: usize;
    /// Stride between redistributors (standard 128 KiB: 64 KiB RD + 64 KiB SGI).
    const REDISTRIBUTOR_STRIDE: usize = 128 * 1024;
    /// GIC ID of the board's GPIO controller interrupt; `u32::MAX` when none.
    const GPIO_GIC_ID: u32 = u32::MAX;

    /// Enable board-specific device interrupts on the distributor.
    fn enable_board_irqs();

    /// Dispatch a shared peripheral interrupt (SPI, GIC ID >= 32).
    fn dispatch_spi(irq: u32);
}

// ── Register offsets ──────────────────────────────────────────────────────────
const GICD_CTLR: usize = 0x0000;
#[allow(dead_code)]
const GICD_TYPER: usize = 0x0004;
#[allow(dead_code)]
const GICD_ISENABLER: usize = 0x0100;
#[allow(dead_code)]
const GICD_IPRIORITYR: usize = 0x0400;
#[allow(dead_code)]
const GICD_IGROUPR: usize = 0x0080;
// GICR offsets
const GICR_WAKER: usize = 0x0014;
const GICR_SGI_OFFSET: usize = 0x10000; // SGI frame starts 64 KiB after RD frame
const GICR_IGROUPR0: usize = 0x0080;
const GICR_ISENABLER0: usize = 0x0100;
const GICR_IPRIORITYR0: usize = 0x0400;

pub const SGI_IPI: u32 = 0;

fn gicd<G: GicV3Layout>(offset: usize) -> *mut u32 {
    (G::DISTRIBUTOR_BASE + offset) as *mut u32
}

fn gicr_rd<G: GicV3Layout>(cpu: usize, offset: usize) -> *mut u32 {
    (G::REDISTRIBUTOR_BASE + cpu * G::REDISTRIBUTOR_STRIDE + offset) as *mut u32
}

fn gicr_sgi<G: GicV3Layout>(cpu: usize, offset: usize) -> *mut u32 {
    (G::REDISTRIBUTOR_BASE + cpu * G::REDISTRIBUTOR_STRIDE + GICR_SGI_OFFSET + offset) as *mut u32
}

#[inline(always)]
fn wr(ptr: *mut u32, val: u32) {
    unsafe { core::ptr::write_volatile(ptr, val) }
}

#[inline(always)]
fn rd(ptr: *mut u32) -> u32 {
    unsafe { core::ptr::read_volatile(ptr) }
}

/// Enable GICv3 system register CPU interface (ICC).
#[inline(always)]
pub unsafe fn enable_system_registers() {
    if super::el2::is_el2() {
        // Enable SRE at EL2
        core::arch::asm!(
            "mrs x9, icc_sre_el2",
            "orr x9, x9, #0x7", // Enable SRE, DIB, DFB
            "msr icc_sre_el2, x9",
            "isb",
            out("x9") _,
            options(nostack),
        );
    }
    core::arch::asm!(
        "mrs x9, icc_sre_el1",
        "orr x9, x9, #0x1", // Enable SRE
        "msr icc_sre_el1, x9",
        "isb",
        out("x9") _,
        options(nostack),
    );
}

/// Initialise GICv3 distributor on the boot core.
pub fn init_distributor<G: GicV3Layout>() {
    // Disable distributor while configuring
    wr(gicd::<G>(GICD_CTLR), 0);
    unsafe { core::arch::asm!("dsb ish", options(nostack)) };

    // Enable Affinity Routing (ARE_NS = bit 4, EnableGrp1NS = bit 1, EnableGrp0 = bit 0)
    // ARE_NS (bit 4) is required for GICv3 system registers.
    wr(gicd::<G>(GICD_CTLR), (1 << 4) | (1 << 1) | (1 << 0));
    unsafe { core::arch::asm!("dsb ish", options(nostack)) };

    G::enable_board_irqs();
}

/// Initialise the calling core's redistributor and system-register CPU interface.
pub fn init_cpu<G: GicV3Layout>(cpu_id: usize) {
    unsafe {
        enable_system_registers();
    }

    // Wake up redistributor: clear ProcessorSleep (bit 1) in GICR_WAKER
    let waker_ptr = gicr_rd::<G>(cpu_id, GICR_WAKER);
    let mut val = rd(waker_ptr);
    val &= !(1 << 1); // clear ProcessorSleep
    wr(waker_ptr, val);

    // Wait until ChildrenAsleep (bit 2) clears
    while (rd(waker_ptr) & (1 << 2)) != 0 {
        core::hint::spin_loop();
    }

    // Configure SGIs/PPIs: Group 1 for all (IGROUPR0 = 0xFFFF_FFFF)
    wr(gicr_sgi::<G>(cpu_id, GICR_IGROUPR0), 0xFFFF_FFFF);

    // Set default priority (0xA0) for all SGIs/PPIs
    for i in 0..8 {
        wr(gicr_sgi::<G>(cpu_id, GICR_IPRIORITYR0 + i * 4), 0xA0A0_A0A0);
    }

    // Enable SGI 0 (IPI) and timer PPIs
    let timer_irq = if super::el2::is_el2() { 26 } else { 30 };
    let enable_mask = (1 << SGI_IPI) | (1 << timer_irq);
    wr(gicr_sgi::<G>(cpu_id, GICR_ISENABLER0), enable_mask);

    // Configure CPU interface system registers
    unsafe {
        core::arch::asm!(
            "msr icc_pmr_el1, {pmr}",     // Priority mask: unmask all (0xFF)
            "msr icc_bpr1_el1, {bpr}",    // Binary point: 0
            "msr icc_ctlr_el1, {ctlr}",   // Default CTLR
            "msr icc_igrpen1_el1, {en}",  // Enable Group 1 interrupts
            "msr icc_igrpen0_el1, {en}",  // Enable Group 0 (FIQ) interrupts
            "isb",
            pmr = in(reg) 0xFFu64,
            bpr = in(reg) 0u64,
            ctlr = in(reg) 0u64,
            en = in(reg) 1u64,
            options(nostack),
        );
    }
}

/// Acknowledge Group 1 interrupt (IRQ).
#[inline(always)]
pub fn claim_irq() -> u32 {
    let intid: u64;
    unsafe {
        core::arch::asm!("mrs {}, icc_iar1_el1", out(reg) intid, options(nomem, nostack));
    }
    intid as u32
}

/// Complete Group 1 interrupt (IRQ).
#[inline(always)]
pub fn complete_irq(intid: u32) {
    unsafe {
        core::arch::asm!("msr icc_eoir1_el1, {}", in(reg) intid as u64, options(nomem, nostack));
    }
}

/// Acknowledge Group 0 interrupt (FIQ).
#[inline(always)]
pub fn claim_fiq() -> u32 {
    let intid: u64;
    unsafe {
        core::arch::asm!("mrs {}, icc_iar0_el1", out(reg) intid, options(nomem, nostack));
    }
    intid as u32
}

/// Complete Group 0 interrupt (FIQ).
#[inline(always)]
pub fn complete_fiq(intid: u32) {
    unsafe {
        core::arch::asm!("msr icc_eoir0_el1, {}", in(reg) intid as u64, options(nomem, nostack));
    }
}

/// Send cross-hart IPI via GICv3 ICC_SGI1R_EL1.
pub fn send_sgi(target_cpu: u32, sgi_id: u32) {
    let aff0 = (target_cpu & 0xFF) as u64;
    let sgi_val = ((sgi_id as u64 & 0xF) << 24) | (1u64 << aff0);
    unsafe {
        core::arch::asm!("msr icc_sgi1r_el1, {}", in(reg) sgi_val, options(nomem, nostack));
        core::arch::asm!("isb", options(nostack));
    }
}

/// Handle one GICv3 IRQ event.
pub fn handle_gicv3_irq<G: GicV3Layout>() {
    let intid = claim_irq();
    let timer_irq = if super::el2::is_el2() { 26 } else { 30 };

    if intid == SGI_IPI {
        unsafe {
            vi_ipi_service();
        }
        return;
    }

    if intid == timer_irq {
        super::timer::reset();
        complete_irq(intid);
        unsafe {
            vi_timer_tick();
        }
    } else if intid == G::GPIO_GIC_ID {
        complete_irq(intid);
        unsafe {
            vi_gpio_notify_irq();
        }
    } else if intid >= 32 && intid < 1020 {
        G::dispatch_spi(intid);
    }

    if intid < 1020 {
        complete_irq(intid);
    }
}

/// Handle one GICv3 FIQ event.
pub fn handle_gicv3_fiq<G: GicV3Layout>() {
    let intid = claim_fiq();
    let timer_irq = if super::el2::is_el2() { 26 } else { 30 };

    if intid == timer_irq {
        super::timer::reset();
        complete_fiq(intid);
        unsafe {
            vi_timer_tick();
        }
    }

    if intid < 1020 {
        complete_fiq(intid);
    }
}
