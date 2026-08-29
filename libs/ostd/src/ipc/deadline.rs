//! End-to-end scheduler-tick deadlines for strict-rendezvous service calls.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExchangeError {
    Send,
    Recv,
    WrongSender,
}

/// Admit one request and receive its reply within one scheduler-tick budget.
pub(super) fn exchange_until_deadline(
    service_tid: usize,
    timeout_ticks: u64,
    mut try_send: impl FnMut() -> bool,
    mut now_ticks: impl FnMut() -> Option<u64>,
    mut yield_once: impl FnMut(),
    mut recv: impl FnMut(u64) -> Result<usize, ()>,
) -> Result<(), ExchangeError> {
    let started = now_ticks().ok_or(ExchangeError::Send)?;
    loop {
        let elapsed = now_ticks()
            .ok_or(ExchangeError::Send)?
            .wrapping_sub(started);
        if elapsed >= timeout_ticks {
            return Err(ExchangeError::Send);
        }
        if try_send() {
            break;
        }
        yield_once();
    }

    let remaining = now_ticks()
        .ok_or(ExchangeError::Recv)
        .map(|now| timeout_ticks.saturating_sub(now.wrapping_sub(started)))?;
    match recv(remaining).map_err(|_| ExchangeError::Recv)? {
        0 => Err(ExchangeError::Recv),
        sender if sender == service_tid => Ok(()),
        _ => Err(ExchangeError::WrongSender),
    }
}

#[cfg(test)]
mod tests {
    use super::{exchange_until_deadline, ExchangeError};
    use core::cell::Cell;

    #[test]
    fn never_receiving_target_expires_without_entering_receive() {
        let tick = Cell::new(10u64);
        let attempts = Cell::new(0usize);
        let result = exchange_until_deadline(
            7,
            5,
            || {
                attempts.set(attempts.get() + 1);
                false
            },
            || Some(tick.get()),
            || tick.set(tick.get() + 1),
            |_| panic!("an unadmitted request must not receive"),
        );
        assert_eq!(result, Err(ExchangeError::Send));
        assert_eq!(attempts.get(), 5);
        assert_eq!(tick.get(), 15);
    }

    #[test]
    fn transient_target_uses_only_remaining_reply_ticks() {
        let tick = Cell::new(20u64);
        let attempts = Cell::new(0usize);
        let observed_reply_budget = Cell::new(0u64);
        let result = exchange_until_deadline(
            7,
            8,
            || {
                attempts.set(attempts.get() + 1);
                attempts.get() == 4
            },
            || Some(tick.get()),
            || tick.set(tick.get() + 1),
            |remaining| {
                observed_reply_budget.set(remaining);
                Ok(7)
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(observed_reply_budget.get(), 5);
    }

    #[test]
    fn clock_loss_after_delivery_is_uncertain_receive() {
        let samples = Cell::new(0usize);
        let result = exchange_until_deadline(
            7,
            5,
            || true,
            || {
                samples.set(samples.get() + 1);
                (samples.get() <= 2).then_some(10)
            },
            || unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(result, Err(ExchangeError::Recv));
    }
}
