// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The bounded reset wait, driven by fake status readers: plain fake BAR
//! memory always reads back the 0 the reset wrote, so a device that never
//! clears (or has vanished) is modelled at the status-read seam.

use crate::regs::wait_for_reset;
use crate::RESET_SPIN_BUDGET;

#[test]
fn reset_wait_gives_up_when_status_never_clears() {
    let mut reads = 0u32;
    let done = wait_for_reset(
        || {
            reads += 1;
            0x0F
        },
        RESET_SPIN_BUDGET,
    );
    assert!(!done);
    assert_eq!(reads, RESET_SPIN_BUDGET);
}

#[test]
fn reset_wait_treats_all_ones_as_gone() {
    let mut reads = 0u32;
    let done = wait_for_reset(
        || {
            reads += 1;
            0xFF
        },
        RESET_SPIN_BUDGET,
    );
    assert!(!done);
    assert_eq!(reads, 1);
}

#[test]
fn reset_wait_succeeds_once_status_clears() {
    let mut statuses = [0x03u8, 0x01, 0x00].into_iter();
    let mut reads = 0u32;
    let done = wait_for_reset(
        || {
            reads += 1;
            statuses.next().expect("polled after reset completed")
        },
        RESET_SPIN_BUDGET,
    );
    assert!(done);
    assert_eq!(reads, 3);
}
