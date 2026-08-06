//! Serialized ownership of the single NET_RX completion reservation.

use super::{CompletionQueue, SlotId};
use crate::sync::Spinlock;
use alloc::sync::Arc;

enum Reservation {
    /// No slot is armed. The last queue handle stays here so finishing an ISR
    /// completion never drops its final `Arc` in interrupt context.
    Idle(Option<Arc<CompletionQueue>>),
    Armed {
        queue: Arc<CompletionQueue>,
        slot: SlotId,
    },
    Completing {
        queue: Arc<CompletionQueue>,
        slot: SlotId,
    },
}

static RESERVATION: Spinlock<Reservation> = Spinlock::new(Reservation::Idle(None));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisarmResult {
    Owned(SlotId),
    Completing,
    NotOwned,
}

pub struct PendingCompletion {
    queue: Arc<CompletionQueue>,
    slot: SlotId,
}

pub fn arm(queue: Arc<CompletionQueue>, slot: SlotId) {
    let arriving = queue.cell().0;
    let displaced = loop {
        let mut state = RESERVATION.lock();
        match &mut *state {
            Reservation::Idle(retained) => {
                let retired = retained.take();
                *state = Reservation::Armed { queue, slot };
                drop(state);
                // Replacing an idle queue happens in syscall context, where
                // releasing its final reference may safely reach the allocator.
                drop(retired);
                return;
            }
            Reservation::Armed {
                queue: previous,
                slot: previous_slot,
            } => {
                let pending = PendingCompletion {
                    queue: Arc::clone(previous),
                    slot: *previous_slot,
                };
                *state = Reservation::Completing {
                    queue: Arc::clone(previous),
                    slot: *previous_slot,
                };
                break pending;
            }
            Reservation::Completing { .. } => {
                drop(state);
                core::hint::spin_loop();
            }
        }
    };

    let displaced_cell = displaced.queue.cell().0;
    let _ = displaced.queue.complete_from(
        displaced.slot,
        api::completion::source::NET_RX,
        api::completion::RESULT_ABANDONED as isize,
    );
    let mut state = RESERVATION.lock();
    *state = Reservation::Armed { queue, slot };
    log::warn!(
        "[net-rx] cell {} took over the reservation held by cell {}",
        arriving,
        displaced_cell
    );
}

pub fn disarm(queue: &Arc<CompletionQueue>) -> DisarmResult {
    let mut state = RESERVATION.lock();
    match &*state {
        Reservation::Armed { queue: armed, slot } if Arc::ptr_eq(armed, queue) => {
            let owned = *slot;
            *state = Reservation::Idle(Some(Arc::clone(armed)));
            DisarmResult::Owned(owned)
        }
        Reservation::Completing {
            queue: completing, ..
        } if Arc::ptr_eq(completing, queue) => DisarmResult::Completing,
        _ => DisarmResult::NotOwned,
    }
}

pub fn begin_signal() -> Option<PendingCompletion> {
    let mut state = RESERVATION.lock();
    let pending = match &*state {
        Reservation::Armed { queue, slot } => PendingCompletion {
            queue: Arc::clone(queue),
            slot: *slot,
        },
        Reservation::Idle(_) | Reservation::Completing { .. } => return None,
    };
    *state = Reservation::Completing {
        queue: Arc::clone(&pending.queue),
        slot: pending.slot,
    };
    Some(pending)
}

pub fn finish_signal(pending: PendingCompletion, result: isize) {
    let _ = pending
        .queue
        .complete_from(pending.slot, api::completion::source::NET_RX, result);
    let mut state = RESERVATION.lock();
    if matches!(
        &*state,
        Reservation::Completing { queue, slot }
            if Arc::ptr_eq(queue, &pending.queue) && *slot == pending.slot
    ) {
        *state = Reservation::Idle(Some(Arc::clone(&pending.queue)));
    }
}
