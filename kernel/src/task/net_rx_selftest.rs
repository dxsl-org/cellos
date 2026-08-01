//! Boot self-test for the NET_RX reservation: the interrupt half of the wait.
//!
//! These properties are the ones the wait's correctness rests on:
//!
//! - a signal fills the outstanding reservation, and does *not* also leave the
//!   level flag set — a frame handed to a waiter must not be remembered as still
//!   pending, or the waiter's next wait returns immediately for a frame it
//!   already has;
//! - a signal with nobody waiting sets the flag instead, and the flag survives
//!   until consumed — that is the lost-wakeup guard, and it is the only thing
//!   that makes a level-triggered condition visible to a wait that starts after
//!   it;
//! - a reservation displaced by a later one is completed as abandoned rather
//!   than dropped, so its waiter is never left with a slot nothing will fill;
//! - releasing a reservation releases only the caller's own;
//! - an in-flight ISR publication cannot look like an empty reservation.

use super::completion::CompletionQueue;
use super::completion_selftest::{insert, queue, remove};
use super::waker;
use alloc::sync::Arc;
use api::completion::RESULT_ABANDONED;
use api::syscall::events::NET_RX;

/// Synthetic tids and cells outside anything the boot sequence has assigned.
const TID_ONE: usize = 9311;
const TID_TWO: usize = 9312;
const CELL_ONE: u64 = 9411;
const CELL_TWO: u64 = 9412;

fn fail(reason: &str) -> bool {
    log::error!("[selftest] NET-RX-RESERVATION: FAIL — {}", reason);
    false
}

/// Leave the source exactly as it was found, whatever the rows above did.
fn reset(queues: &[&Arc<CompletionQueue>]) {
    for q in queues {
        if let waker::DisarmResult::Owned(slot) = waker::disarm_net_rx(q) {
            let _ = q.complete(slot, RESULT_ABANDONED as isize);
        }
        while q.drain().is_some() {}
    }
    let _ = waker::consume_pending(NET_RX);
}

/// A signal fills the outstanding reservation and leaves no phantom pending
/// frame behind.
fn signal_fills_the_reservation() -> bool {
    insert(TID_ONE, CELL_ONE);
    let mut ok = true;
    match queue(TID_ONE) {
        Some(q) => {
            match q.reserve() {
                Some(slot) => {
                    waker::arm_net_rx(q.clone(), slot);
                    waker::signal_net_rx();
                    match q.drain() {
                        Some(done) if done.slot == slot && done.result == NET_RX as isize => {}
                        other => {
                            ok = fail(&alloc::format!(
                                "drained {:?}, expected the reserved slot carrying the RX bit",
                                other
                            ))
                        }
                    }
                    if waker::has_any_pending() {
                        ok = fail("a frame handed to a waiter was also left pending");
                    }
                    if waker::disarm_net_rx(&q) != waker::DisarmResult::NotOwned {
                        ok = fail("the signal left the reservation armed");
                    }
                }
                None => ok = fail("an empty queue refused the first reservation"),
            }
            reset(&[&q]);
        }
        None => ok = fail("no queue could be reached from the task record"),
    }
    remove(TID_ONE);
    ok
}

/// With nobody waiting, the frame is remembered until the next wait consumes it.
fn signal_with_no_waiter_is_remembered() -> bool {
    let mut ok = true;
    if waker::has_any_pending() {
        ok = fail("a frame was already pending before the row started");
    }
    waker::signal_net_rx();
    if !waker::has_any_pending() {
        ok = fail("a frame that arrived with nobody waiting was forgotten");
    }
    if waker::consume_pending(NET_RX) != NET_RX {
        ok = fail("the pending frame was not reported to the first consumer");
    }
    if waker::consume_pending(NET_RX) != 0 {
        ok = fail("the same frame was reported to a second consumer");
    }
    ok
}

/// Arming over a live reservation completes the displaced one rather than
/// dropping it: a promised landing place is always filled.
fn takeover_completes_the_displaced_reservation() -> bool {
    insert(TID_ONE, CELL_ONE);
    insert(TID_TWO, CELL_TWO);
    let mut ok = true;
    match (queue(TID_ONE), queue(TID_TWO)) {
        (Some(first), Some(second)) => {
            match (first.reserve(), second.reserve()) {
                (Some(first_slot), Some(second_slot)) => {
                    waker::arm_net_rx(first.clone(), first_slot);
                    waker::arm_net_rx(second.clone(), second_slot);

                    match first.drain() {
                        Some(done)
                            if done.slot == first_slot
                                && done.result == RESULT_ABANDONED as isize => {}
                        other => {
                            ok = fail(&alloc::format!(
                                "displaced waiter drained {:?}, expected its slot released",
                                other
                            ))
                        }
                    }
                    if waker::disarm_net_rx(&first) != waker::DisarmResult::NotOwned {
                        ok = fail("the displaced reservation is still the armed one");
                    }
                    if waker::disarm_net_rx(&second) != waker::DisarmResult::Owned(second_slot) {
                        ok = fail("the reservation that took over is not the armed one");
                    }
                }
                _ => ok = fail("an empty queue refused a reservation"),
            }
            reset(&[&first, &second]);
        }
        _ => ok = fail("a live task could not reach a queue"),
    }
    remove(TID_ONE);
    remove(TID_TWO);
    ok
}

/// Taking the slot and publishing its completion are separate ISR steps. The
/// waiter must observe the intermediate state instead of returning a timeout.
fn split_signal_publication_is_visible() -> bool {
    insert(TID_ONE, CELL_ONE);
    let mut ok = true;
    match queue(TID_ONE) {
        Some(q) => match q.reserve() {
            Some(slot) => {
                waker::arm_net_rx(q.clone(), slot);
                match waker::begin_signal_net_rx_for_test() {
                    Some(pending) => {
                        if waker::disarm_net_rx(&q) != waker::DisarmResult::Completing {
                            ok = fail("an in-flight signal looked like an empty reservation");
                        }
                        if q.drain().is_some() {
                            ok = fail("the split signal published before its finish step");
                        }
                        waker::finish_signal_net_rx_for_test(pending);
                        match q.drain() {
                            Some(done) if done.slot == slot && done.result == NET_RX as isize => {}
                            other => {
                                ok = fail(&alloc::format!(
                                    "split signal drained {:?}, expected the RX completion",
                                    other
                                ))
                            }
                        }
                    }
                    None => ok = fail("an armed reservation could not begin a signal"),
                }
                reset(&[&q]);
            }
            None => ok = fail("an empty queue refused a reservation"),
        },
        None => ok = fail("no queue could be reached from the task record"),
    }
    remove(TID_ONE);
    ok
}

/// Returns true iff the NET_RX reservation completes, remembers and releases as
/// specified. Logs a decisive serial line.
pub fn self_test() -> bool {
    let ok = signal_fills_the_reservation()
        & signal_with_no_waiter_is_remembered()
        & takeover_completes_the_displaced_reservation()
        & split_signal_publication_is_visible();

    if ok {
        log::info!("[selftest] NET-RX-RESERVATION: PASS (fills, remembers, releases)");
    } else {
        log::error!("[selftest] NET-RX-RESERVATION: FAIL");
    }
    ok
}
