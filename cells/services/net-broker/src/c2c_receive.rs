// SPDX-License-Identifier: Apache-2.0
//! Local-only receive admission ordering for decoded C2C requests.

use crate::c2c_dedup::{DedupCache, DedupDecision, DedupKey};
use crate::c2c_envelope::{C2cEnvelope, EnvelopeKind, ServerEpoch};
use crate::server_epoch::require_current;

/// Rejected server-incarnation transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceServerError {
    NonIncreasingEpoch,
}
/// Applies target-incarnation validation before dedup admission.
pub struct ReceiveGate {
    current_server_epoch: ServerEpoch,
    dedup: DedupCache,
}

impl ReceiveGate {
    /// Create a gate for the currently registered server incarnation.
    pub const fn new(current_server_epoch: ServerEpoch) -> Self {
        Self {
            current_server_epoch,
            dedup: DedupCache::new(),
        }
    }

    /// Replace the live target after a successful server restart.
    ///
    /// Old-epoch in-flight work can no longer complete, so its response entries
    /// are retired atomically while authenticated source replay floors remain.
    ///
    /// # Errors
    /// Returns `NonIncreasingEpoch` without changing state when `next` is equal
    /// to or below the current boot-local epoch.
    pub fn replace_server(&mut self, next: ServerEpoch) -> Result<(), ReplaceServerError> {
        if next <= self.current_server_epoch {
            return Err(ReplaceServerError::NonIncreasingEpoch);
        }
        self.current_server_epoch = next;
        self.dedup.retire_entries();
        Ok(())
    }

    /// Validate a decoded request and consult bounded dedup state.
    ///
    /// Non-request frames and stale target epochs return `Indeterminate`
    /// without entering dedup state. A valid current request inherits the
    /// cache's `Dispatch`, `Busy`, replay, or indeterminate decision.
    pub fn begin(&mut self, envelope: &C2cEnvelope<'_>, now_ms: u64) -> DedupDecision {
        if envelope.kind != EnvelopeKind::Request
            || require_current(self.current_server_epoch, envelope.dst_server_epoch).is_err()
        {
            return DedupDecision::Indeterminate;
        }
        self.dedup.begin(
            DedupKey {
                src_node: envelope.src_node,
                src_boot_epoch: envelope.src_boot_epoch,
                request_id: envelope.request_id,
                dst_server_epoch: envelope.dst_server_epoch,
            },
            envelope.retry_class,
            now_ms,
        )
    }

    /// Return the number of admitted response-cache entries.
    pub fn len(&self) -> usize {
        self.dedup.len()
    }

    /// Return whether no response-cache entries have been admitted.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests;
