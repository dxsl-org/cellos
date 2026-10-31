//! Fault injection for the Phase 08A block-outcome witness.
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
}

const INDETERMINATE_MARKER: &[u8] = b"F08A-IND";
const NOT_ISSUED_MARKER: &[u8] = b"F08A-NI";

static INDETERMINATE_FIRED: AtomicBool = AtomicBool::new(false);
static NOT_ISSUED_FIRED: AtomicBool = AtomicBool::new(false);

fn carries(payload: &[u8], marker: &[u8]) -> bool {
    payload.windows(marker.len()).any(|window| window == marker)
}

/// Arm the controller for a write of `payload` to `lba`.
pub fn before_write(ctrl: &mut AhciController, lba: u64, payload: &[u8]) {
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
