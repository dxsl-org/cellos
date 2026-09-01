use crate::local_queue::{BrokerState, QueuedReply, REPLY_TRY_SEND_BUDGET};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrySendResult {
    Delivered,
    Busy,
}

pub const MAX_REPLY_SEND_ATTEMPTS: u8 = 32;

/// Number of replies eligible for one pump turn.
///
/// Snapshotting the queue length prevents a requeued Busy reply from consuming
/// several lifetime attempts before its receiver gets another scheduler turn.
pub const fn reply_turn_budget(pending_replies: usize) -> usize {
    if pending_replies < REPLY_TRY_SEND_BUDGET {
        pending_replies
    } else {
        REPLY_TRY_SEND_BUDGET
    }
}

// The reply remains inline and Copy because this no-alloc retry path must hand
// ownership back to its caller when the bounded queue is saturated.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetainBusyResult {
    Queued,
    Saturated(QueuedReply),
    Exhausted,
}

/// Retain a reply after `sys_try_send` reports Busy.
///
/// A saturated queue returns ownership to the caller for cooperative
/// backpressure. A reply that reaches the bounded retry limit is accounted as
/// terminal because the kernel cannot distinguish a busy target from an exited
/// one.
pub fn retain_busy_reply(state: &mut BrokerState, reply: QueuedReply) -> RetainBusyResult {
    state.note_try_send_busy();
    if reply.attempts >= MAX_REPLY_SEND_ATTEMPTS - 1 {
        state.note_terminal_reply();
        return RetainBusyResult::Exhausted;
    }
    if state.requeue_reply(reply) {
        RetainBusyResult::Queued
    } else {
        let mut pending = reply;
        pending.attempts = pending.attempts.saturating_add(1);
        RetainBusyResult::Saturated(pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_ingress::ReplyStatus;
    use crate::local_queue::LOCAL_REPLY_QUEUE_CAP;

    #[test]
    fn busy_reply_is_retained_when_capacity_exists() {
        let mut state = BrokerState::new();
        assert_eq!(
            retain_busy_reply(
                &mut state,
                QueuedReply::new(1, 2, 3, ReplyStatus::Busy, &[], 0),
            ),
            RetainBusyResult::Queued
        );
        assert_eq!(state.reply_len(), 1);
        assert_eq!(state.counters.try_send_busy, 1);
    }

    #[test]
    fn saturated_queue_returns_reply_to_caller() {
        let mut state = BrokerState::new();
        for order in 0..LOCAL_REPLY_QUEUE_CAP as u64 {
            assert!(state.requeue_reply(QueuedReply::new(
                1,
                order,
                order,
                ReplyStatus::Busy,
                b"x",
                order,
            )));
        }
        let pending = QueuedReply::new(
            2,
            u64::MAX,
            u64::MAX,
            ReplyStatus::Indeterminate,
            &[],
            u64::MAX,
        );

        let mut retained = pending;
        retained.attempts = 1;
        assert_eq!(
            retain_busy_reply(&mut state, pending),
            RetainBusyResult::Saturated(retained)
        );
        assert_eq!(state.reply_len(), LOCAL_REPLY_QUEUE_CAP);
        assert_eq!(state.counters.try_send_busy, 1);
        assert_eq!(state.counters.terminal, 0);
    }

    #[test]
    fn one_busy_reply_consumes_one_attempt_per_pump_turn() {
        let mut state = BrokerState::new();
        assert!(state.requeue_reply(QueuedReply::new(2, 3, 4, ReplyStatus::Success, &[], 0,)));
        let attempts_before = state.take_next_reply().expect("queued reply").attempts;
        assert!(state.requeue_reply(QueuedReply::new(2, 3, 4, ReplyStatus::Success, &[], 0,)));

        let budget = reply_turn_budget(state.reply_len());
        assert_eq!(budget, 1);
        for _ in 0..budget {
            let reply = state.take_next_reply().expect("turn reply");
            assert_eq!(
                retain_busy_reply(&mut state, reply),
                RetainBusyResult::Queued
            );
        }
        let retained = state.take_next_reply().expect("retained reply");
        assert_eq!(retained.attempts, attempts_before + 1);
    }

    #[test]
    fn unreachable_reply_exhaustion_is_terminal_and_bounded() {
        let mut state = BrokerState::new();
        let mut reply = QueuedReply::new(2, 3, 4, ReplyStatus::Success, &[], 0);
        reply.attempts = MAX_REPLY_SEND_ATTEMPTS - 1;

        assert_eq!(
            retain_busy_reply(&mut state, reply),
            RetainBusyResult::Exhausted
        );
        assert_eq!(state.reply_len(), 0);
        assert_eq!(state.counters.try_send_busy, 1);
        assert_eq!(state.counters.terminal, 1);
    }
}
