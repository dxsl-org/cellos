// SPDX-License-Identifier: Apache-2.0
//! Boot-local destination server incarnation epochs.

pub use types::c2c::ServerEpoch;

/// Returned when a request targets a replaced server incarnation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleServerEpoch;

/// Issues one fresh epoch for each successful export registration.
///
/// The source is volatile and belongs to one net-broker incarnation. Remote
/// dispatch must remain disabled until authenticated session state guarantees
/// that endpoints learned under an older broker incarnation cannot be reused.
pub struct ServerEpochSource {
    next: u64,
}

impl ServerEpochSource {
    /// Start a fresh boot-local sequence at epoch one.
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Issue the next server epoch, or `None` after exhausting the `u64` space.
    pub fn issue(&mut self) -> Option<ServerEpoch> {
        let epoch = ServerEpoch::new(self.next)?;
        self.next = self.next.checked_add(1).unwrap_or(0);
        Some(epoch)
    }
}

impl Default for ServerEpochSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Accept only the currently registered server incarnation.
///
/// Callers must perform this check before dedup admission or local dispatch.
pub const fn require_current(
    current: ServerEpoch,
    requested: ServerEpoch,
) -> Result<(), StaleServerEpoch> {
    if current.get() == requested.get() {
        Ok(())
    } else {
        Err(StaleServerEpoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_issues_a_distinct_epoch() {
        let mut source = ServerEpochSource::new();
        let first = source.issue().unwrap();
        let replacement = source.issue().unwrap();
        assert_ne!(first, replacement);
        assert_eq!(require_current(replacement, first), Err(StaleServerEpoch));
        assert_eq!(require_current(replacement, replacement), Ok(()));
    }

    #[test]
    fn exhausted_source_fails_closed() {
        let mut source = ServerEpochSource { next: u64::MAX };
        assert_eq!(source.issue().map(ServerEpoch::get), Some(u64::MAX));
        assert_eq!(source.issue(), None);
    }
}
