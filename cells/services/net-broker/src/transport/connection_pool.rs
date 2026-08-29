use super::{NoiseSession, MAX_SESSIONS};
use ostd::{ViError, ViResult};
use service_net_broker::session_pool::BoundedSessionPool;

/// Bounded pool of active Noise sessions.
///
/// Occupied sessions are never displaced. Full-pool pressure is reported as
/// `ViError::WouldBlock` for later mapping to the typed remote `Busy` outcome.
pub struct ConnectionPool {
    sessions: BoundedSessionPool<NoiseSession, MAX_SESSIONS>,
}

impl ConnectionPool {
    /// Create an empty connection pool.
    pub const fn new() -> Self {
        Self {
            sessions: BoundedSessionPool::new(),
        }
    }

    /// Return whether a new session can be admitted without displacement.
    pub fn is_full(&self) -> bool {
        self.sessions.is_full()
    }

    /// Admit a session without replacing any occupied slot.
    ///
    /// # Errors
    /// Returns `ViError::WouldBlock` and leaves all existing sessions unchanged
    /// when the pool is full.
    pub fn try_insert(&mut self, session: NoiseSession) -> ViResult<usize> {
        self.sessions
            .try_insert(session)
            .map_err(|_| ViError::WouldBlock)
    }

    /// Borrow one occupied session mutably.
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut NoiseSession> {
        self.sessions.get_mut(slot)
    }

    /// Remove every session using `cap_id`, such as after peer reset.
    pub fn remove_by_cap(&mut self, cap_id: u32) {
        self.sessions
            .remove_where(|session| session.cap_id == cap_id);
    }

    /// Return the number of occupied session slots.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}
