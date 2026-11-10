// SPDX-License-Identifier: MPL-2.0
//! ARM Generic Interrupt Controller version 3 (GICv3) driver.
//!
//! Distributor, redistributors and the system-register CPU interface (ICC),
//! with affinity routing. Every function is generic over the board's
//! [`GicV3Layout`], whose windows may come from the firmware device tree, so
//! the bases are runtime values. Device interrupts are Group 1 (IRQ); Group 0
//! (FIQ) is enabled only on a single-security-state GIC.
use hal_arch_trait::{vi_gpio_notify_irq, vi_ipi_service, vi_timer_tick};

use super::gicv3_layout::{
    find_redistributor, irouter_for, mpidr_affinity, GicV3Windows, GICR_FRAME_SIZE, GICR_TYPER,
};

/// One board's GICv3 wiring.
pub trait GicV3Layout {
    /// GIC ID of the board's GPIO controller interrupt; `u32::MAX` when none.
    const GPIO_GIC_ID: u32 = u32::MAX;

    /// The distributor and redistributor windows in use.
    fn windows() -> GicV3Windows;

    /// Enable board-specific device interrupts ([`enable_spi`]) once the
    /// distributor is configured.
    fn enable_board_irqs();

    /// Dispatch a shared peripheral interrupt (SPI, GIC ID >= 32) that is
    /// neither the timer nor the GPIO controller.
    fn dispatch_spi(irq: u32);
}

// ── Register offsets ──────────────────────────────────────────────────────────
const GICD_CTLR: usize = 0x0000;
const GICD_TYPER: usize = 0x0004;
const GICD_IGROUPR: usize = 0x0080;
const GICD_ISENABLER: usize = 0x0100;
const GICD_ICENABLER: usize = 0x0180;
const GICD_IPRIORITYR: usize = 0x0400;
const GICD_ICFGR: usize = 0x0C00;
const GICD_IROUTER: usize = 0x6000;
/// `GICD_CTLR.RWP`: a write to `GICD_CTLR` has not yet taken effect.
const GICD_CTLR_RWP: u32 = 1 << 31;
/// `GICD_CTLR.DS`: single security state, so Group 0 is signalled to this EL as FIQ.
const GICD_CTLR_DS: u32 = 1 << 6;
/// `GICD_CTLR.ARE_NS` (ARE with `DS` set): affinity routing, which the ICC
/// system-register interface requires.
const GICD_CTLR_ARE_NS: u32 = 1 << 4;
/// `GICD_CTLR.EnableGrp1NS` and `EnableGrp0`.
const GICD_CTLR_ENABLE_GROUPS: u32 = (1 << 1) | 1;
// GICR offsets: RD_base frame, then the SGI_base frame 64 KiB above it.
const GICR_WAKER: usize = 0x0014;
const GICR_WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;
const GICR_WAKER_CHILDREN_ASLEEP: u32 = 1 << 2;
const GICR_IGROUPR0: usize = 0x0080;
const GICR_ISENABLER0: usize = 0x0100;
const GICR_ICENABLER0: usize = 0x0180;
const GICR_IPRIORITYR0: usize = 0x0400;
/// Default priority of every interrupt: below the 0xFF mask, so all are taken.
const PRIORITY: u32 = 0xA0A0_A0A0;

/// Software-generated interrupt used as the kernel's cross-hart IPI.
pub const SGI_IPI: u32 = 0;

fn gicd<G: GicV3Layout>(offset: usize) -> *mut u32 {
    (G::windows().distributor + offset) as *mut u32
}

#[inline(always)]
fn wr(ptr: *mut u32, val: u32) {
    // SAFETY: every pointer is inside a GICv3 window the board validated as
    // identity-mapped device memory.
    unsafe { core::ptr::write_volatile(ptr, val) }
}

#[inline(always)]
fn rd(ptr: *mut u32) -> u32 {
    // SAFETY: as in `wr`.
    unsafe { core::ptr::read_volatile(ptr) }
}

fn mpidr() -> u64 {
    let mpidr: u64;
    // SAFETY: MPIDR_EL1 is a read-only identification register.
    unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nomem, nostack)) };
    mpidr
}

/// The host tick's PPI: CNTHP (26) for an EL2 host, CNTP (30) at EL1.
fn timer_ppi() -> u32 {
    if super::el2::is_el2() {
        26
    } else {
        30
    }
}

/// Enable GICv3 system register CPU interface (ICC).
///
/// # Safety
///
/// Must run on the core whose CPU interface is being enabled, before any other
/// `ICC_*` access on it. When the kernel runs at EL1, EL2 firmware must already
/// have set `ICC_SRE_EL2.Enable`, or the `ICC_SRE_EL1` access traps to EL2.
#[inline(always)]
pub unsafe fn enable_system_registers() {
    if super::el2::is_el2() {
        // SRE | DFB | DIB | Enable: Enable (bit 3) is what lets EL1 reach
        // ICC_SRE_EL1 at all; without it every EL1 access traps to EL2.
        core::arch::asm!(
            "mrs x9, icc_sre_el2",
            "orr x9, x9, #0xf",
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

/// Spin until the last `GICD_CTLR` write has taken effect (GICv3 §12.9.4).
fn wait_distributor_rwp<G: GicV3Layout>() {
    while rd(gicd::<G>(GICD_CTLR)) & GICD_CTLR_RWP != 0 {
        core::hint::spin_loop();
    }
}

/// Initialise the distributor on the boot core: every SPI disabled, Group 1,
/// level-triggered, default priority and routed to the boot core; then the
/// board's device interrupts are enabled.
pub fn init_distributor<G: GicV3Layout>() {
    wr(gicd::<G>(GICD_CTLR), 0);
    wait_distributor_rwp::<G>();
    // Affinity routing goes on before any group is enabled; GICD_IROUTER only
    // exists once it is set.
    wr(gicd::<G>(GICD_CTLR), GICD_CTLR_ARE_NS);
    wait_distributor_rwp::<G>();

    // GICD_TYPER.ITLinesNumber: 32 * (N + 1) INTIDs. Index 0 of each banked
    // array covers SGIs/PPIs, which live in the redistributor instead.
    let lines = ((rd(gicd::<G>(GICD_TYPER)) & 0x1F) as usize + 1) * 32;
    let route = irouter_for(mpidr());
    for word in 1..lines / 32 {
        wr(gicd::<G>(GICD_ICENABLER + word * 4), u32::MAX);
        wr(gicd::<G>(GICD_IGROUPR + word * 4), u32::MAX);
    }
    for word in 8..lines / 4 {
        wr(gicd::<G>(GICD_IPRIORITYR + word * 4), PRIORITY);
    }
    for word in 2..lines / 16 {
        wr(gicd::<G>(GICD_ICFGR + word * 4), 0);
    }
    for intid in 32..lines {
        let router = (G::windows().distributor + GICD_IROUTER + intid * 8) as *mut u64;
        // SAFETY: GICD_IROUTER<n> is a 64-bit register inside the distributor frame.
        unsafe { core::ptr::write_volatile(router, route) };
    }

    wr(
        gicd::<G>(GICD_CTLR),
        GICD_CTLR_ARE_NS | GICD_CTLR_ENABLE_GROUPS,
    );
    wait_distributor_rwp::<G>();

    G::enable_board_irqs();
}

/// Enable SPI `intid` (GIC ID >= 32) at the distributor.
pub fn enable_spi<G: GicV3Layout>(intid: u32) {
    let intid = intid as usize;
    wr(
        gicd::<G>(GICD_ISENABLER + (intid / 32) * 4),
        1 << (intid % 32),
    );
}

/// Initialise the calling core's redistributor and system-register CPU
/// interface, and enable its IPI SGI and host-tick PPI.
///
/// # Panics
/// When the redistributor region holds no frame for this core's affinity: the
/// core could take neither ticks nor IPIs.
pub fn init_cpu<G: GicV3Layout>() {
    // SAFETY: runs on the core being configured, before any other ICC access.
    unsafe {
        enable_system_registers();
    }

    let mpidr = mpidr();
    let rd_base = find_redistributor(&G::windows(), mpidr_affinity(mpidr), |rd_base| {
        // SAFETY: the walk stays inside the validated redistributor region.
        unsafe { core::ptr::read_volatile((rd_base + GICR_TYPER) as *const u64) }
    })
    .unwrap_or_else(|| panic!("[gicv3] no redistributor for MPIDR {mpidr:#x}"));
    let sgi = |offset: usize| (rd_base + GICR_FRAME_SIZE + offset) as *mut u32;

    // Wake the redistributor and wait until it reports its children awake.
    let waker = (rd_base + GICR_WAKER) as *mut u32;
    wr(waker, rd(waker) & !GICR_WAKER_PROCESSOR_SLEEP);
    while rd(waker) & GICR_WAKER_CHILDREN_ASLEEP != 0 {
        core::hint::spin_loop();
    }

    // SGIs/PPIs: all Group 1 at the default priority, only IPI + tick enabled.
    wr(sgi(GICR_ICENABLER0), u32::MAX);
    wr(sgi(GICR_IGROUPR0), u32::MAX);
    for i in 0..8 {
        wr(sgi(GICR_IPRIORITYR0 + i * 4), PRIORITY);
    }
    wr(sgi(GICR_ISENABLER0), (1 << SGI_IPI) | (1 << timer_ppi()));

    // SAFETY: ICC system registers of this core, enabled above.
    unsafe {
        core::arch::asm!(
            "msr icc_pmr_el1, {pmr}",     // Priority mask: unmask all (0xFF)
            "msr icc_bpr1_el1, {bpr}",    // Binary point: 0
            "msr icc_ctlr_el1, {ctlr}",   // Default CTLR (EOImode = 0)
            "msr icc_igrpen1_el1, {en}",  // Enable Group 1 interrupts
            "isb",
            pmr = in(reg) 0xFFu64,
            bpr = in(reg) 0u64,
            ctlr = in(reg) 0u64,
            en = in(reg) 1u64,
            options(nostack),
        );
    }
    // Group 0 is Secure when the GIC has two security states: the register then
    // belongs to EL3 (and traps there when SCR_EL3.FIQ is set), so only a
    // single-security-state GIC lets this EL take Group 0 as FIQ.
    if rd(gicd::<G>(GICD_CTLR)) & GICD_CTLR_DS != 0 {
        // SAFETY: as above; ICC_IGRPEN0_EL1 is this EL's with DS set.
        unsafe {
            core::arch::asm!(
                "msr icc_igrpen0_el1, {en}",
                "isb",
                en = in(reg) 1u64,
                options(nostack),
            );
        }
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

/// Send cross-hart IPI via GICv3 `ICC_SGI1R_EL1`.
///
/// `target_cpu` is split as Aff1 = `target_cpu / 16`, Aff0 = `target_cpu % 16`:
/// the SGI target list only addresses 16 Aff0 values per Aff1 cluster.
pub fn send_sgi(target_cpu: u32, sgi_id: u32) {
    let aff1 = u64::from((target_cpu >> 4) & 0xFF);
    let target_list = 1u64 << (target_cpu & 0xF);
    let sgi_val = ((u64::from(sgi_id) & 0xF) << 24) | (aff1 << 16) | target_list;
    unsafe {
        core::arch::asm!("msr icc_sgi1r_el1, {}", in(reg) sgi_val, options(nomem, nostack));
        core::arch::asm!("isb", options(nostack));
    }
}

/// INTIDs 1020..=1023 are special (spurious / no pending interrupt): never EOI'd.
const INTID_SPECIAL: u32 = 1020;

/// Handle one GICv3 IRQ event.
///
/// Each acknowledged INTID is EOI'd exactly once. Timer and GPIO are EOI'd
/// before their hook because `vi_timer_tick` may context-switch away; a second
/// EOI on return would drop the priority of whatever interrupt is active then.
pub fn handle_gicv3_irq<G: GicV3Layout>() {
    let intid = claim_irq();

    if intid == SGI_IPI {
        unsafe {
            vi_ipi_service();
        }
        complete_irq(intid);
    } else if intid == timer_ppi() {
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
    } else if (32..INTID_SPECIAL).contains(&intid) {
        G::dispatch_spi(intid);
        complete_irq(intid);
    } else if intid < INTID_SPECIAL {
        complete_irq(intid);
    }
}

/// Handle one GICv3 FIQ event (Group 0; EOI'd exactly once, as in [`handle_gicv3_irq`]).
pub fn handle_gicv3_fiq<G: GicV3Layout>() {
    let intid = claim_fiq();

    if intid == timer_ppi() {
        super::timer::reset();
        complete_fiq(intid);
        unsafe {
            vi_timer_tick();
        }
    } else if intid < INTID_SPECIAL {
        complete_fiq(intid);
    }
}
