/// Result of one nonblocking admission offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionAttempt {
    /// The target accepted the request exactly once.
    Admitted,
    /// The bounded target mailbox rejected this offer.
    Rejected,
    /// Shutdown won the atomic admission race.
    Cancelled,
}

/// Result of bounded request admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendAdmission {
    /// The target accepted the request exactly once.
    Admitted,
    /// Shutdown won before or during the admission attempt.
    Cancelled,
}
/// Failure of a bounded IPC admission or receive operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadlineError {
    /// The monotonic scheduler clock was unavailable.
    ClockUnavailable,
    /// The shared end-to-end deadline elapsed.
    DeadlineElapsed,
    /// The admitted receive operation failed.
    ReceiveFailed,
}

/// Offers one request without blocking until admission, cancellation, or timeout.
///
/// `started_at` and `timeout_ticks` define the shared end-to-end budget.
/// `cancelled` checks shutdown before each offer, `try_send` performs one
/// atomically classified nonblocking offer, `now_ticks` samples scheduler time,
/// and `yield_once` yields after rejection.
///
/// # Errors
/// Returns [`DeadlineError::ClockUnavailable`] when `now_ticks` has no value and
/// [`DeadlineError::DeadlineElapsed`] when the shared budget is exhausted.
pub fn admit_request_until_deadline(
    started_at: u64,
    timeout_ticks: u64,
    mut cancelled: impl FnMut() -> bool,
    mut try_send: impl FnMut() -> AdmissionAttempt,
    mut now_ticks: impl FnMut() -> Option<u64>,
    mut yield_once: impl FnMut(),
) -> Result<SendAdmission, DeadlineError> {
    loop {
        if cancelled() {
            return Ok(SendAdmission::Cancelled);
        }
        let elapsed = now_ticks()
            .ok_or(DeadlineError::ClockUnavailable)?
            .wrapping_sub(started_at);
        if elapsed >= timeout_ticks {
            return Err(DeadlineError::DeadlineElapsed);
        }
        match try_send() {
            AdmissionAttempt::Admitted => return Ok(SendAdmission::Admitted),
            AdmissionAttempt::Cancelled => return Ok(SendAdmission::Cancelled),
            AdmissionAttempt::Rejected => yield_once(),
        }
    }
}

/// Receives one admitted reply through cancellable timeout slices.
///
/// `started_at` and `timeout_ticks` retain the admission deadline.
/// `poll_ticks` bounds shutdown observation latency. `recv_slice` returns
/// `Ok(None)` when one slice expires and `Ok(Some(sender))` on delivery.
/// Returns `Ok(None)` when cancellation wins.
///
/// # Errors
/// Returns [`DeadlineError::ClockUnavailable`] when `now_ticks` has no value,
/// [`DeadlineError::DeadlineElapsed`] when the shared budget is exhausted, and
/// propagates [`DeadlineError::ReceiveFailed`] from `recv_slice`.
pub fn receive_until_deadline(
    started_at: u64,
    timeout_ticks: u64,
    poll_ticks: u64,
    mut cancelled: impl FnMut() -> bool,
    mut now_ticks: impl FnMut() -> Option<u64>,
    mut recv_slice: impl FnMut(u64) -> Result<Option<usize>, DeadlineError>,
) -> Result<Option<usize>, DeadlineError> {
    loop {
        if cancelled() {
            return Ok(None);
        }
        let elapsed = now_ticks()
            .ok_or(DeadlineError::ClockUnavailable)?
            .wrapping_sub(started_at);
        let remaining = timeout_ticks.saturating_sub(elapsed);
        if remaining == 0 {
            return Err(DeadlineError::DeadlineElapsed);
        }
        if let Some(sender) = recv_slice(remaining.min(poll_ticks))? {
            return Ok(Some(sender));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_before_admission_stops_send_retries() {
        let tick = core::cell::Cell::new(10u64);
        let attempts = core::cell::Cell::new(0usize);
        let result = admit_request_until_deadline(
            tick.get(),
            5,
            || false,
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() == 1 {
                    AdmissionAttempt::Rejected
                } else {
                    AdmissionAttempt::Cancelled
                }
            },
            || Some(tick.get()),
            || tick.set(tick.get() + 1),
        );
        assert_eq!(result, Ok(SendAdmission::Cancelled));
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn successful_admission_is_not_retried() {
        let attempts = core::cell::Cell::new(0usize);
        let result = admit_request_until_deadline(
            20,
            5,
            || false,
            || {
                attempts.set(attempts.get() + 1);
                AdmissionAttempt::Admitted
            },
            || Some(20),
            || unreachable!(),
        );
        assert_eq!(result, Ok(SendAdmission::Admitted));
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn shutdown_after_admission_stops_sliced_receive() {
        let tick = core::cell::Cell::new(30u64);
        let slices = core::cell::Cell::new(0usize);
        let shutdown = core::cell::Cell::new(false);
        let result = receive_until_deadline(
            tick.get(),
            20,
            4,
            || shutdown.get(),
            || Some(tick.get()),
            |slice| {
                assert_eq!(slice, 4);
                slices.set(slices.get() + 1);
                tick.set(tick.get() + slice);
                shutdown.set(true);
                Ok(None)
            },
        );
        assert_eq!(result, Ok(None));
        assert_eq!(slices.get(), 1);
    }
}
