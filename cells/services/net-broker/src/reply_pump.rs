use crate::local_queue::{BrokerState, QueuedReply};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrySendResult {
    Delivered,
    Busy,
}

pub fn dispatch_or_queue<F>(state: &mut BrokerState, reply: QueuedReply, send: F)
where
    F: Fn(&QueuedReply) -> TrySendResult,
{
    if send(&reply) == TrySendResult::Busy {
        state.note_try_send_busy();
        let _ = state.requeue_reply(reply);
    }
}

pub fn pump_turn<F>(state: &mut BrokerState, budget: usize, send: F) -> bool
where
    F: Fn(&QueuedReply) -> TrySendResult,
{
    let mut progressed = false;
    for _ in 0..budget {
        let Some(reply) = state.take_next_reply() else {
            break;
        };
        progressed = true;
        if send(&reply) == TrySendResult::Busy {
            state.note_try_send_busy();
            let _ = state.requeue_reply(reply);
        }
    }
    progressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_ingress::ReplyStatus;
    use crate::local_queue::REPLY_TRY_SEND_BUDGET;
    use core::cell::Cell;

    #[test]
    fn dispatch_or_queue_retains_busy_reply() {
        let mut state = BrokerState::new();
        dispatch_or_queue(
            &mut state,
            QueuedReply::new(1, 2, 3, ReplyStatus::Busy, &[], 0),
            |_| TrySendResult::Busy,
        );
        assert_eq!(state.reply_len(), 1);
        assert_eq!(state.counters.try_send_busy, 1);
    }

    #[test]
    fn pump_turn_stops_at_retry_budget_and_retains_requeued_reply() {
        let mut state = BrokerState::new();
        for order in 0..10 {
            assert!(state.requeue_reply(QueuedReply::new(
                1,
                order,
                order,
                ReplyStatus::Busy,
                b"x",
                order,
            )));
        }

        let calls = Cell::new(0usize);
        let progressed = pump_turn(&mut state, REPLY_TRY_SEND_BUDGET, |_| {
            calls.set(calls.get() + 1);
            TrySendResult::Busy
        });

        assert!(progressed);
        assert_eq!(calls.get(), REPLY_TRY_SEND_BUDGET);
        assert_eq!(state.reply_len(), 10);
        assert_eq!(state.counters.try_send_busy, REPLY_TRY_SEND_BUDGET as u64);
        let retained = state.take_next_reply().expect("reply retained");
        assert_eq!(retained.attempts, 1);
    }
}
