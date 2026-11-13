// SPDX-License-Identifier: MPL-2.0
//! ACPI SCI: ACPI mode, the fixed power button and its relay to the supervisor
//! (plan 261103-2218 phase 06, owner decision H-S2-7).
//!
//! Contracts:
//! - [`init`] runs once on the BSP inside the x86 ACPI init block, before any
//!   reader, and publishes the power facts 436 acts on.
//! - The SCI route and `PWRBTN_EN` are set from the first hart-0 timer tick
//!   ([`relay_pending`]): the I/O APICs and the BSP APIC ID are registered only
//!   after the ACPI init block.
//! - The ISR ([`vi_handle_sci`]) touches only PM1 status ports and atomics: no
//!   lock, no scheduler, no IPC.
//! - [`relay_pending`] runs on hart 0 in `vi_timer_tick` with no lock held and
//!   posts the supervisor-private `OP_POWER` poweroff request from sender 0. A
//!   post that fails stays pending and is retried on the next tick.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, Ordering};

use abi::supervisor_power::{KERNEL_SENDER, POWER_BUTTON_REQUEST};

use super::power::{self, inb, inw, io_port, outb, outw};
use crate::acpi::AcpiInfo;

/// No SCI: no FADT, hardware-reduced ACPI, no I/O PM1 event block, ACPI mode
/// refused, or no I/O APIC serves the SCI.
const STATE_OFF: u8 = 0;
/// ACPI mode is on; the route and `PWRBTN_EN` wait for the first hart-0 tick.
const STATE_ROUTE_PENDING: u8 = 1;
const STATE_ARMED: u8 = 2;

const PM1_PWRBTN: u16 = 1 << 8;
const PM1_SCI_EN: u16 = 1 << 0;
/// Every fixed-event status bit (TMR, BM, GBL, PWRBTN, SLPBTN, RTC, WAK);
/// writing one clears it.
const PM1_ALL_FIXED_STATUS: u16 = 0x8731;
/// Status-port reads while firmware hands over to ACPI mode (≥ ~3 s on
/// hardware, as ACPI-aware OSes allow).
const ACPI_ENABLE_POLLS: usize = 3_000_000;

static STATE: AtomicU8 = AtomicU8::new(STATE_OFF);
static PM1A_STATUS: AtomicU16 = AtomicU16::new(0);
static PM1B_STATUS: AtomicU16 = AtomicU16::new(0);
/// Offset of the enable register from the status register (half the block).
static PM1_ENABLE_OFFSET: AtomicU16 = AtomicU16::new(0);
static SCI_GSI: AtomicU32 = AtomicU32::new(0);
static SCI_FLAGS: AtomicU16 = AtomicU16::new(0);
static PENDING: AtomicBool = AtomicBool::new(false);
static RELAY_BLOCKED_LOGGED: AtomicBool = AtomicBool::new(false);

/// Read the power facts, enter ACPI mode, quiet every GPE and PM1 event, and
/// record the SCI route for [`relay_pending`] to program.
pub fn init(
    info: &AcpiInfo,
    rsdp_phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
) {
    let Some(fadt) = info.fadt else {
        log::warn!("[acpi] sci: no FADT; power button off, reset without FADT register");
        return;
    };
    let facts = power::discover(&fadt, rsdp_phys, map_physical);
    power::record(facts);
    if fadt.hw_reduced() {
        log::info!("[acpi] sci: hardware-reduced ACPI; no fixed power button");
        return;
    }
    let half = u16::from(fadt.pm1a_evt.bit_width / 16);
    let (Some(status_a), true) = (io_port(&fadt.pm1a_evt), half >= 2) else {
        log::warn!("[acpi] sci: no I/O PM1a event block; power button off");
        return;
    };
    let status_b = io_port(&fadt.pm1b_evt);
    if !enable_acpi_mode(&fadt) {
        log::warn!("[acpi] sci: SCI_EN stayed clear; power button off");
        return;
    }
    // No GPE handler exists: an enabled GPE would hold the level SCI asserted.
    for block in facts.gpe.iter().filter(|block| block.len >= 2) {
        let gpe_half = u16::from(block.len / 2);
        for index in 0..gpe_half {
            outb(block.port + gpe_half + index, 0);
            outb(block.port + index, 0xFF);
        }
    }
    for status in [Some(status_a), status_b].into_iter().flatten() {
        outw(status + half, 0);
        outw(status, PM1_ALL_FIXED_STATUS);
    }

    // MPS INTI flags `00` mean "conforms to the bus", which for the SCI is
    // level-triggered, active-low (ACPI 6.5 Table 5.9, SCI_INT).
    let sci = u32::from(fadt.sci_int);
    let (gsi, flags) = match usize::try_from(sci).ok().filter(|&irq| irq < 16) {
        Some(irq) => (info.irq_overrides[irq], info.irq_override_flags[irq]),
        None => (sci, 0),
    };
    let polarity = match flags & 0b11 {
        0 => 0b11,
        given => given,
    };
    let trigger = match (flags >> 2) & 0b11 {
        0 => 0b11,
        given => given,
    };
    PM1A_STATUS.store(status_a, Ordering::Relaxed);
    PM1B_STATUS.store(status_b.unwrap_or(0), Ordering::Relaxed);
    PM1_ENABLE_OFFSET.store(half, Ordering::Relaxed);
    SCI_GSI.store(gsi, Ordering::Relaxed);
    SCI_FLAGS.store(polarity | trigger << 2, Ordering::Relaxed);
    STATE.store(STATE_ROUTE_PENDING, Ordering::Release);
    log::info!(
        "[acpi] sci: ACPI mode on, sci_int={} gsi={} flags={:#x} vector={:#x}",
        sci,
        gsi,
        polarity | trigger << 2,
        crate::hal::idt::policy::SCI_VECTOR
    );
}

/// `SCI_EN` set, by the `SMI_CMD` handover when firmware left it clear.
fn enable_acpi_mode(fadt: &crate::acpi::FadtInfo) -> bool {
    let Some(control) = io_port(&fadt.pm1a_cnt) else {
        return false;
    };
    if inw(control) & PM1_SCI_EN != 0 {
        return true;
    }
    let Ok(smi_cmd) = u16::try_from(fadt.smi_cmd) else {
        return false;
    };
    if smi_cmd == 0 || fadt.acpi_enable == 0 {
        return false;
    }
    outb(smi_cmd, fadt.acpi_enable);
    for _ in 0..ACPI_ENABLE_POLLS {
        if inw(control) & PM1_SCI_EN != 0 {
            return true;
        }
        let _ = inb(0x80);
    }
    false
}

/// Program the SCI route on the BSP, then enable the power button.
fn arm() {
    let Some(destination) = crate::hal::apic::cpu_apic_id(0) else {
        return;
    };
    let gsi = SCI_GSI.load(Ordering::Relaxed);
    let flags = SCI_FLAGS.load(Ordering::Relaxed);
    let vector = crate::hal::idt::policy::SCI_VECTOR;
    if !crate::hal::ioapic::route_gsi(gsi, vector, destination, flags) {
        STATE.store(STATE_OFF, Ordering::Release);
        log::warn!(
            "[acpi] sci: no I/O APIC serves GSI {}; power button off",
            gsi
        );
        return;
    }
    let half = PM1_ENABLE_OFFSET.load(Ordering::Relaxed);
    for status in [&PM1A_STATUS, &PM1B_STATUS]
        .map(|port| port.load(Ordering::Relaxed))
        .into_iter()
        .filter(|&port| port != 0)
    {
        outw(status, PM1_PWRBTN);
        outw(status + half, inw(status + half) | PM1_PWRBTN);
    }
    STATE.store(STATE_ARMED, Ordering::Release);
    log::info!(
        "[acpi] sci: armed gsi={} vector={:#x} apic={} PWRBTN_EN",
        gsi,
        vector,
        destination
    );
}

/// SCI interrupt: acknowledge `PWRBTN_STS` and mark a press pending.
#[no_mangle]
pub extern "Rust" fn vi_handle_sci() {
    let mut pressed = false;
    for status in [&PM1A_STATUS, &PM1B_STATUS]
        .map(|port| port.load(Ordering::Relaxed))
        .into_iter()
        .filter(|&port| port != 0)
    {
        if inw(status) & PM1_PWRBTN != 0 {
            outw(status, PM1_PWRBTN);
            pressed = true;
        }
    }
    if pressed {
        PENDING.store(true, Ordering::Release);
    }
}

/// Hart-0 tick work: program the route once, then relay a pending press.
pub fn relay_pending() {
    match STATE.load(Ordering::Acquire) {
        STATE_ROUTE_PENDING => return arm(),
        STATE_ARMED => {}
        _ => return,
    }
    if !PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    let supervisor = crate::cell::service_registry::lookup(abi::syscall::service::SUPERVISOR);
    match supervisor.filter(|&tid| {
        crate::task::ipc_post_nonblock(KERNEL_SENDER, tid, &POWER_BUTTON_REQUEST).is_ok()
    }) {
        Some(tid) => {
            RELAY_BLOCKED_LOGGED.store(false, Ordering::Relaxed);
            log::warn!(
                "[acpi] power button: poweroff request relayed to supervisor tid {}",
                tid
            );
        }
        None => {
            PENDING.store(true, Ordering::Release);
            if !RELAY_BLOCKED_LOGGED.swap(true, Ordering::Relaxed) {
                log::warn!("[acpi] power button: supervisor unreachable; retried every tick");
            }
        }
    }
}
