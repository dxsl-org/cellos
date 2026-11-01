//! Fault injection for the block-outcome and driver-death witnesses.
//!
//! Compiled only with the `fault-inject` feature, which no production image
//! enables: the canonical x86 assembler profile builds `driver-ahci` with its
//! default features. A write whose sector payload carries a marker is faulted
//! once:
//!
//! - `F08A-IND`: the command is issued (`PxCI`) and runs, then its completion
//!   is suppressed, so the driver takes the post-issue timeout path and replies
//!   `Indeterminate`. Any later write carrying the same marker is a replay and
//!   is logged as one.
//! - `F08A-NI`: refused before `PxCI`, replying `NotIssued`. The next write
//!   carrying the marker is the caller's retry and is logged when it completes.
//! - `F08B-DIE`: the driver exits on the spot while the HBA it programmed stays
//!   bus-mastering, so the kernel's retirement quiescence (Phase 08B) has a
//!   live function to stop.
//! - `F09C-DIE-ISSUED`: the command is issued (`PxCI`) and the driver exits
//!   (70) before polling its completion, so the caller's outcome is
//!   indeterminate. A replay of that write reaching the replacement driver
//!   shows as a second exit line for the marker.
//! - `F09C-DIE-SHM`: the driver allocates and maps one SHM frame, then exits
//!   (70) holding it, so its retirement verdict is `Unproven(SHM_UNTRACKED)`.

use crate::controller::AhciController;
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::io::print_fmt;
use types::block_wire::BlockOutcome;

/// Action applied to the next issued command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inject {
    None,
    RefuseBeforeIssue,
    SuppressCompletion,
    /// Exit (70) right after the `PxCI` write of the write to `lba`.
    ExitAfterIssue { lba: u64 },
}

const INDETERMINATE_MARKER: &[u8] = b"F08A-IND";
const NOT_ISSUED_MARKER: &[u8] = b"F08A-NI";
const DIE_MARKER: &[u8] = b"F08B-DIE";
const DIE_ISSUED_MARKER: &[u8] = b"F09C-DIE-ISSUED";
const DIE_SHM_MARKER: &[u8] = b"F09C-DIE-SHM";

static INDETERMINATE_FIRED: AtomicBool = AtomicBool::new(false);
static NOT_ISSUED_FIRED: AtomicBool = AtomicBool::new(false);

fn carries(payload: &[u8], marker: &[u8]) -> bool {
    payload.windows(marker.len()).any(|window| window == marker)
}

/// Arm the controller for a write of `payload` to `lba`.
pub fn before_write(ctrl: &mut AhciController, lba: u64, payload: &[u8]) {
    if carries(payload, DIE_MARKER) {
        let _ = print_fmt(format_args!("[ahci] fault-inject: driver exiting at lba={}\n", lba));
        ostd::syscall::sys_exit(70);
    }
    if carries(payload, DIE_SHM_MARKER) {
        exit_holding_shm(lba);
    }
    if carries(payload, DIE_ISSUED_MARKER) {
        ctrl.inject_next(Inject::ExitAfterIssue { lba });
        return;
    }
    if carries(payload, INDETERMINATE_MARKER) {
        if INDETERMINATE_FIRED.swap(true, Ordering::Relaxed) {
            let _ = print_fmt(format_args!(
                "[ahci] fault-inject: REPLAY of indeterminate write lba={}\n",
                lba
            ));
        } else {
            ctrl.inject_next(Inject::SuppressCompletion);
        }
    } else if carries(payload, NOT_ISSUED_MARKER) && !NOT_ISSUED_FIRED.swap(true, Ordering::Relaxed)
    {
        ctrl.inject_next(Inject::RefuseBeforeIssue);
    }
}

/// Report how a marked write ended.
pub fn after_write(lba: u64, payload: &[u8], outcome: BlockOutcome) {
    if carries(payload, INDETERMINATE_MARKER) || carries(payload, NOT_ISSUED_MARKER) {
        let _ = print_fmt(format_args!(
            "[ahci] fault-inject: marked write lba={} outcome={:?}\n",
            lba, outcome
        ));
    }
}

/// Allocate and map one SHM frame, then exit (70) without releasing it. If the
/// kernel refuses the frame, log the refusal and let the write proceed.
fn exit_holding_shm(lba: u64) {
    use ostd::syscall::{sys_shm_alloc, sys_shm_map, SyscallResult};
    let mapped = match sys_shm_alloc(4096) {
        SyscallResult::Ok(frame) => matches!(sys_shm_map(frame, 0), SyscallResult::Ok(_)),
        SyscallResult::Err(_) => false,
    };
    if !mapped {
        let _ = print_fmt(format_args!("[ahci] fault-inject: SHM refused at lba={}\n", lba));
        return;
    }
    let _ = print_fmt(format_args!(
        "[ahci] fault-inject: driver exiting holding SHM at lba={}\n",
        lba
    ));
    ostd::syscall::sys_exit(70);
}
