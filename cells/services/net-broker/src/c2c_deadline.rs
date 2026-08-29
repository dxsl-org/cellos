// SPDX-License-Identifier: Apache-2.0
//! Relative monotonic deadline classification for C2C requests.

use types::c2c::RelativeDeadline;

/// Whether local dispatch has become externally observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadlinePhase {
    BeforeDispatch,
    AfterDispatch,
}

/// Delivery claim permitted at the current monotonic time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadlineDecision {
    Continue,
    Timeout,
    Indeterminate,
}

/// Absolute monotonic deadline derived from one bounded relative budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestDeadline {
    expires_at_ms: u64,
}

impl RequestDeadline {
    /// Convert a validated relative budget into an absolute monotonic deadline.
    ///
    /// Returns `None` when the absolute deadline would overflow.
    pub const fn from_relative(now_ms: u64, relative: RelativeDeadline) -> Option<Self> {
        match now_ms.checked_add(relative.milliseconds() as u64) {
            Some(expires_at_ms) => Some(Self { expires_at_ms }),
            None => None,
        }
    }

    /// Classify whether work may continue at `now_ms`.
    ///
    /// Expiry before dispatch is a definite `Timeout`. Once dispatch may have
    /// occurred, expiry is `Indeterminate` because completion cannot be denied.
    pub const fn classify(self, now_ms: u64, phase: DeadlinePhase) -> DeadlineDecision {
        if now_ms < self.expires_at_ms {
            return DeadlineDecision::Continue;
        }
        match phase {
            DeadlinePhase::BeforeDispatch => DeadlineDecision::Timeout,
            DeadlinePhase::AfterDispatch => DeadlineDecision::Indeterminate,
        }
    }

    /// Return the absolute monotonic expiry used by the broker scheduler.
    pub const fn expires_at_ms(self) -> u64 {
        self.expires_at_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_budget_fails_closed() {
        assert_eq!(
            RequestDeadline::from_relative(u64::MAX, RelativeDeadline::new(1).unwrap()),
            None
        );
    }

    #[test]
    fn exact_expiry_depends_on_dispatch_observability() {
        let deadline =
            RequestDeadline::from_relative(10, RelativeDeadline::new(5).unwrap()).unwrap();
        assert_eq!(deadline.expires_at_ms(), 15);
        assert_eq!(
            deadline.classify(14, DeadlinePhase::BeforeDispatch),
            DeadlineDecision::Continue
        );
        assert_eq!(
            deadline.classify(15, DeadlinePhase::BeforeDispatch),
            DeadlineDecision::Timeout
        );
        assert_eq!(
            deadline.classify(15, DeadlinePhase::AfterDispatch),
            DeadlineDecision::Indeterminate
        );
    }
}
