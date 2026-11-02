extern crate std;

use super::*;
use std::vec::Vec;

const PENDING: u32 = LR_PENDING;
const ACTIVE: u32 = LR_ACTIVE;
const TIMER: u32 = 27;

fn pending_from(list: &[u32]) -> impl FnMut() -> Option<u32> + '_ {
    let mut it = list.iter().copied();
    move || it.next()
}

fn level_timer(intid: u32) -> bool {
    intid == TIMER
}

#[test]
fn plan_loads_pending_as_group0_pending() {
    let (lrs, used) = plan_entry_lrs(&[], 4, pending_from(&[27, 49]));
    assert_eq!(used, 2);
    assert_eq!(lrs[0], 0x1000_0000 | 27);
    assert_eq!(lrs[1], 0x1000_0000 | 49);
    assert_eq!(lrs[0] & (1 << 30), 0, "Group 1 bit must stay clear");
}

#[test]
fn plan_keeps_active_lrs_first_and_verbatim() {
    let saved = ACTIVE | (0x1F << 23) | 49;
    let (lrs, used) = plan_entry_lrs(&[saved], 4, pending_from(&[27]));
    assert_eq!(used, 2);
    assert_eq!(lrs[0], saved);
    assert_eq!(lrs[1], PENDING | 27);
}

#[test]
fn plan_merges_pending_into_matching_active_lr() {
    let (lrs, used) = plan_entry_lrs(&[ACTIVE | 49], 4, pending_from(&[49, 50]));
    assert_eq!(used, 2, "a merged INTID must not take a second LR");
    assert_eq!(lrs[0], ACTIVE | PENDING | 49);
    assert_eq!(lrs[1], PENDING | 50);
}

#[test]
fn plan_stops_pulling_when_lrs_are_full() {
    let mut pulled = Vec::new();
    let mut source = [3u32, 4, 5, 6].into_iter();
    let (lrs, used) = plan_entry_lrs(&[ACTIVE | 1, ACTIVE | 2], 3, || {
        let next = source.next();
        pulled.extend(next);
        next
    });
    assert_eq!(used, 3);
    assert_eq!(lrs[2], PENDING | 3);
    assert_eq!(
        pulled,
        [3],
        "INTIDs that do not fit stay in the pending set"
    );
}

#[test]
fn plan_honours_fewer_implemented_lrs() {
    let (_, used) = plan_entry_lrs(&[], 2, pending_from(&[1, 2, 3]));
    assert_eq!(used, 2);
    let (_, used) = plan_entry_lrs(&[], 16, pending_from(&[1, 2, 3, 4, 5]));
    assert_eq!(used, MAX_LRS);
}

#[test]
fn sort_requeues_unacknowledged_edge_irqs() {
    let mut requeued = Vec::new();
    let (_, n_active) = sort_exit_lrs(&[PENDING | 49], 0, level_timer, |i| requeued.push(i));
    assert_eq!(n_active, 0);
    assert_eq!(requeued, [49]);
}

#[test]
fn sort_drops_pending_only_level_line() {
    let mut requeued = Vec::new();
    let (_, n_active) = sort_exit_lrs(&[PENDING | TIMER, PENDING | 49], 0, level_timer, |i| {
        requeued.push(i)
    });
    assert_eq!(n_active, 0);
    assert_eq!(
        requeued,
        [49],
        "the timer is re-sampled at entry, not re-queued"
    );
}

#[test]
fn sort_keeps_active_lrs_and_counts_them() {
    let mut requeued = Vec::new();
    let lrs = [
        ACTIVE | 49,
        ACTIVE | PENDING | 50,
        PENDING | 51,
        ACTIVE | PENDING | TIMER,
    ];
    let (active, n_active) = sort_exit_lrs(&lrs, 0, level_timer, |i| requeued.push(i));
    assert_eq!(n_active, 3);
    assert_eq!(active[0], ACTIVE | 49);
    assert_eq!(
        active[1],
        ACTIVE | PENDING | 50,
        "edge Pending+Active stays verbatim"
    );
    assert_eq!(
        active[2],
        ACTIVE | TIMER,
        "a level line's pending state is re-sampled"
    );
    assert_eq!(requeued, [51]);
}

#[test]
fn sort_skips_empty_lrs() {
    let mut requeued = Vec::new();
    // LR0 empty (ELRSR bit 0), LR1 active; stale bits in an empty LR are ignored.
    let (active, n_active) = sort_exit_lrs(&[PENDING | 49, ACTIVE | 50], 0b01, level_timer, |i| {
        requeued.push(i)
    });
    assert_eq!(n_active, 1);
    assert_eq!(active[0], ACTIVE | 50);
    assert!(requeued.is_empty());
}

#[test]
fn exit_then_entry_redelivers_only_what_is_still_owed() {
    // Guest took 49 (Active) and never acknowledged 50; the timer was pending.
    let mut pending = Vec::new();
    let (active, n_active) = sort_exit_lrs(
        &[ACTIVE | 49, PENDING | 50, PENDING | TIMER],
        0,
        level_timer,
        |i| pending.push(i),
    );
    assert_eq!(pending, [50]);
    let (lrs, used) = plan_entry_lrs(&active[..n_active], 4, pending_from(&pending));
    assert_eq!(used, 2);
    assert_eq!(lrs[0], ACTIVE | 49);
    assert_eq!(lrs[1], PENDING | 50);
}

#[test]
fn vtimer_line_needs_enable_and_no_mask() {
    assert!(
        vtimer_line_asserted(0b001, 100, 100),
        "fires when the count reaches CVAL"
    );
    assert!(vtimer_line_asserted(0b001, 100, 101));
    assert!(!vtimer_line_asserted(0b001, 100, 99));
    assert!(!vtimer_line_asserted(0b000, 0, u64::MAX), "disabled");
    assert!(!vtimer_line_asserted(0b011, 0, u64::MAX), "masked");
    assert!(
        !vtimer_line_asserted(0b010, 0, u64::MAX),
        "masked and disabled"
    );
}

#[test]
fn vtimer_line_ignores_stale_istatus_and_compares_unsigned() {
    // ISTATUS (bit 2) is a snapshot from the last exit; only the count decides.
    assert!(!vtimer_line_asserted(0b101, 100, 99));
    assert!(vtimer_line_asserted(0b001, 0, 0));
    assert!(!vtimer_line_asserted(0b001, u64::MAX, u64::MAX - 1));
    assert!(vtimer_line_asserted(0b001, u64::MAX, u64::MAX));
    // A CVAL with the top bit set is far in the future, not in the past.
    assert!(!vtimer_line_asserted(0b001, 1 << 63, 5));
}
