/// Atomic ownership state for one restart-sensitive network exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum RestartIpcState {
    Idle = 0,
    Admitting = 1,
    Active = 2,
    ArmedAdmitting = 3,
    ArmedActive = 4,
    Acked = 5,
}

impl RestartIpcState {
    /// Decodes the value stored in the restart oracle's atomic state.
    pub const fn from_raw(raw: usize) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Admitting),
            2 => Some(Self::Active),
            3 => Some(Self::ArmedAdmitting),
            4 => Some(Self::ArmedActive),
            5 => Some(Self::Acked),
            _ => None,
        }
    }

    /// Returns the stable integer representation used by `AtomicUsize`.
    pub const fn raw(self) -> usize {
        self as usize
    }
}

/// Computes shutdown's ownership transition.
///
/// The boolean is `true` only when shutdown linearizes before admission and may
/// acknowledge immediately. `None` means shutdown was already armed or acked.
pub const fn arm_shutdown(state: RestartIpcState) -> Option<(RestartIpcState, bool)> {
    match state {
        RestartIpcState::Idle => Some((RestartIpcState::Acked, true)),
        RestartIpcState::Admitting => Some((RestartIpcState::ArmedAdmitting, false)),
        RestartIpcState::Active => Some((RestartIpcState::ArmedActive, false)),
        RestartIpcState::ArmedAdmitting | RestartIpcState::ArmedActive | RestartIpcState::Acked => {
            None
        }
    }
}

/// Computes the state published after one guarded nonblocking post attempt.
///
/// The boolean is `true` when shutdown owns the attempt and the caller must
/// stop. `None` means the caller no longer owns an admission attempt.
pub const fn finish_admission(
    state: RestartIpcState,
    admitted: bool,
) -> Option<(RestartIpcState, bool)> {
    match state {
        RestartIpcState::Admitting if admitted => Some((RestartIpcState::Active, false)),
        RestartIpcState::Admitting => Some((RestartIpcState::Idle, false)),
        RestartIpcState::ArmedAdmitting => Some((RestartIpcState::Acked, true)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_before_shutdown_leaves_an_immediately_acknowledgeable_idle_state() {
        let (state, shutdown_owned) =
            finish_admission(RestartIpcState::Admitting, false).expect("owned admission");
        assert_eq!(state, RestartIpcState::Idle);
        assert!(!shutdown_owned);

        let (state, before_admission) = arm_shutdown(state).expect("shutdown arms idle state");
        assert_eq!(state, RestartIpcState::Acked);
        assert!(before_admission);
    }

    #[test]
    fn shutdown_during_failed_admission_is_acked_by_the_finisher() {
        let (state, before_admission) =
            arm_shutdown(RestartIpcState::Admitting).expect("shutdown arms admission");
        assert_eq!(state, RestartIpcState::ArmedAdmitting);
        assert!(!before_admission);

        let (state, shutdown_owned) = finish_admission(state, false).expect("finisher owns arm");
        assert_eq!(state, RestartIpcState::Acked);
        assert!(shutdown_owned);
    }
}
