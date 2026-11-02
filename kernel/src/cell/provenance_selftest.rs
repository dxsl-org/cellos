//! Private 1/2-hart witness for CSpace authority provenance (ginux-lessons 07C).
//!
//! Compiled only with `test-hooks` on RV64. Hart 0 registers two private
//! authorities and runs the derive probe through the real dispatcher; when the
//! RT hart is online it runs the same probe there against the shared table, so
//! the checks are proven from a remote hart, not only the publisher's. The
//! authorities use a generation no real Cell reaches and are retired afterwards.

use super::cspace::{
    dispatch_handle_control, register_cell_authority, retire_cell_authority, ControlStatus,
    Principal, ResourceKind, CELL_AUTHORITIES,
};
use crate::task::cap::CapSet;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use types::CellId;

const SOURCE: (u64, u64) = (44, 0x07C0_0001);
const PEER: (u64, u64) = (45, 0x07C0_0002);

const IDLE: u8 = 0;
const ARMED: u8 = 1;
const PASSED: u8 = 2;
const FAILED: u8 = 3;

static PHASE: AtomicU8 = AtomicU8::new(IDLE);
static ROOT: AtomicU64 = AtomicU64::new(0);
static FILE: AtomicU64 = AtomicU64::new(0);

fn derive(ticket: u64, destination: (u64, u64), rights: u32) -> (ControlStatus, u64) {
    let mut req = [0u8; 48];
    req[0..4].copy_from_slice(&1u32.to_le_bytes());
    req[4..8].copy_from_slice(&1u32.to_le_bytes());
    req[8..16].copy_from_slice(&ticket.to_le_bytes());
    req[16..24].copy_from_slice(&destination.0.to_le_bytes());
    req[24..32].copy_from_slice(&destination.1.to_le_bytes());
    req[32..36].copy_from_slice(&rights.to_le_bytes());
    let header = dispatch_handle_control(CellId(SOURCE.0), SOURCE.1, &req);
    (header.status, header.ticket)
}

/// Negatives (cross-Cell, stale, unregistered, non-delegable) and the positive
/// equal-rights local derive that must keep the Kernel issuer.
fn probe() -> Result<(), &'static str> {
    let (root, file) = (ROOT.load(Ordering::Acquire), FILE.load(Ordering::Acquire));
    if derive(root, PEER, 0x1).0 != ControlStatus::Denied {
        return Err("cross-cell derive not denied");
    }
    if derive(root, (PEER.0, PEER.1 + 1), 0x1).0 != ControlStatus::Stale {
        return Err("stale destination not refused");
    }
    if derive(root, (46, 1), 0x1).0 != ControlStatus::Stale {
        return Err("unregistered destination not refused");
    }
    if derive(file, SOURCE, 0x1).0 != ControlStatus::Denied {
        return Err("non-delegable root derived");
    }
    let (status, child) = derive(root, SOURCE, 0x3);
    if status != ControlStatus::Success {
        return Err("equal-rights local derive refused");
    }
    let mut auths = CELL_AUTHORITIES.lock();
    let slot = auths
        .get_mut(&SOURCE)
        .and_then(|auth| auth.verify_ticket(child).ok());
    match slot {
        Some(slot) if slot.issuer == Principal::Kernel && slot.rights == 0x3 && slot.delegable => {
            Ok(())
        }
        _ => Err("derived slot lost issuer, rights or delegability"),
    }
}

fn report(hart: usize, result: Result<(), &'static str>) -> bool {
    match result {
        Ok(()) => log::info!("[selftest] AUTHORITY-PROVENANCE: PASS hart={}", hart),
        Err(why) => log::error!(
            "[selftest] AUTHORITY-PROVENANCE: FAIL hart={} ({})",
            hart,
            why
        ),
    }
    result.is_ok()
}

pub fn run_primary() {
    if !register_cell_authority(CellId(SOURCE.0), SOURCE.1, CapSet::EMPTY)
        || !register_cell_authority(CellId(PEER.0), PEER.1, CapSet::EMPTY)
    {
        log::error!("[selftest] AUTHORITY-PROVENANCE: FAIL hart=0 (authority registration)");
        return;
    }
    {
        let mut auths = CELL_AUTHORITIES.lock();
        let Some(auth) = auths.get_mut(&SOURCE) else {
            return;
        };
        let (Ok(root), Ok(file)) = (
            auth.mint_kernel_root(ResourceKind::CSpaceAuthority, 0x3, true),
            auth.mint_kernel_root(ResourceKind::BootFSFile, 0x1, false),
        ) else {
            drop(auths);
            log::error!("[selftest] AUTHORITY-PROVENANCE: FAIL hart=0 (root mint)");
            return;
        };
        ROOT.store(root, Ordering::Release);
        FILE.store(file, Ordering::Release);
    }

    if report(0, probe()) {
        if crate::task::smp::is_rt_hart_online() {
            PHASE.store(ARMED, Ordering::Release);
            let deadline = crate::task::system_ticks() + 500;
            loop {
                match PHASE.load(Ordering::Acquire) {
                    PASSED | FAILED => break,
                    _ if crate::task::system_ticks() >= deadline => {
                        log::error!("[selftest] AUTHORITY-PROVENANCE: FAIL hart=1 (no answer)");
                        break;
                    }
                    _ => core::hint::spin_loop(),
                }
            }
        } else {
            log::info!("[selftest] AUTHORITY-PROVENANCE: remote RUNTIME-GATED (hart 1 offline)");
        }
    }
    PHASE.store(IDLE, Ordering::Release);
    retire_cell_authority(CellId(SOURCE.0), SOURCE.1);
    retire_cell_authority(CellId(PEER.0), PEER.1);
}

pub fn run_secondary(hart_id: usize) {
    if hart_id != crate::task::smp::HART_RT {
        return;
    }
    let deadline = crate::task::system_ticks() + 500;
    while PHASE.load(Ordering::Acquire) != ARMED {
        if crate::task::system_ticks() >= deadline {
            return;
        }
        core::hint::spin_loop();
    }
    let passed = report(hart_id, probe());
    PHASE.store(if passed { PASSED } else { FAILED }, Ordering::Release);
}
