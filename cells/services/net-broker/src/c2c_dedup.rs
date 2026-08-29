// SPDX-License-Identifier: Apache-2.0
//! Fixed-capacity C2C request deduplication and response retention.

mod source_window;
mod types;

use crate::c2c_envelope::{RetryClass, MAX_C2C_PAYLOAD};
use types::{Entry, EntryState, SourceWindow};

pub use types::{
    C2cStatus, CachedReply, DedupDecision, DedupError, DedupKey, DEDUP_CAPACITY, DEDUP_TTL_MS,
    SOURCE_WINDOW_CAPACITY,
};

/// No-allocation cache. In-flight entries are never evicted.
pub struct DedupCache {
    entries: [Option<Entry>; DEDUP_CAPACITY],
    sources: [Option<SourceWindow>; SOURCE_WINDOW_CAPACITY],
}

pub const DEDUP_STATIC_BYTES: usize = core::mem::size_of::<DedupCache>();

impl DedupCache {
    pub const fn new() -> Self {
        Self {
            entries: [None; DEDUP_CAPACITY],
            sources: [None; SOURCE_WINDOW_CAPACITY],
        }
    }

    /// Admit a request or classify its duplicate without dispatching it.
    pub fn begin(&mut self, key: DedupKey, retry_class: RetryClass, now_ms: u64) -> DedupDecision {
        if !key.is_valid() {
            return DedupDecision::Indeterminate;
        }
        if self.is_stale_boot(key) {
            return DedupDecision::Indeterminate;
        }
        if let Some(index) = self.find(key) {
            let entry = self.entries[index].as_mut().expect("found");
            if entry.retry_class != retry_class {
                return DedupDecision::Indeterminate;
            }
            let expired = now_ms.saturating_sub(entry.first_seen_ms) >= DEDUP_TTL_MS;
            return match (entry.state, expired, retry_class) {
                (EntryState::Accepted | EntryState::Dispatched, _, _) => DedupDecision::Busy,
                (EntryState::Completed, false, _) => DedupDecision::Replay(index),
                (EntryState::Completed, true, RetryClass::Idempotent) => {
                    self.entries[index] = Some(Entry::accepted(key, retry_class, now_ms));
                    DedupDecision::Dispatch
                }
                (EntryState::Completed, true, _) => {
                    entry.state = EntryState::ExpiredNonReplayable;
                    entry.payload_len = 0;
                    DedupDecision::Indeterminate
                }
                (EntryState::ExpiredNonReplayable, _, _) => DedupDecision::Indeterminate,
            };
        }

        let source_slot = match self.source_slot(key) {
            Ok(slot) => slot,
            Err(decision) => return decision,
        };
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .or_else(|| self.evictable_slot(now_ms));
        let Some(index) = slot else {
            return DedupDecision::Busy;
        };
        self.entries[index] = Some(Entry::accepted(key, retry_class, now_ms));
        self.sources[source_slot]
            .as_mut()
            .expect("source slot reserved")
            .high_request_id = key.request_id;
        DedupDecision::Dispatch
    }

    /// Mark a newly admitted request as locally dispatched.
    pub fn mark_dispatched(&mut self, key: DedupKey) -> Result<(), DedupError> {
        let entry = self.entry_mut(key)?;
        if entry.state != EntryState::Accepted {
            return Err(DedupError::InvalidTransition);
        }
        entry.state = EntryState::Dispatched;
        Ok(())
    }

    /// Cache one completed response for replay within the retention window.
    pub fn complete(
        &mut self,
        key: DedupKey,
        status: C2cStatus,
        payload: &[u8],
    ) -> Result<(), DedupError> {
        if payload.len() > MAX_C2C_PAYLOAD {
            return Err(DedupError::PayloadTooLarge);
        }
        let entry = self.entry_mut(key)?;
        if !matches!(entry.state, EntryState::Accepted | EntryState::Dispatched) {
            return Err(DedupError::InvalidTransition);
        }
        entry.payload[..payload.len()].copy_from_slice(payload);
        entry.payload_len = payload.len();
        entry.status = status;
        entry.state = EntryState::Completed;
        Ok(())
    }

    /// Borrow a cached response selected by `DedupDecision::Replay`.
    pub fn replay(&self, slot: usize, key: DedupKey, now_ms: u64) -> Option<CachedReply<'_>> {
        let entry = self.entries.get(slot)?.as_ref()?;
        if entry.key != key
            || entry.state != EntryState::Completed
            || now_ms.saturating_sub(entry.first_seen_ms) >= DEDUP_TTL_MS
        {
            return None;
        }
        Some(CachedReply {
            status: entry.status,
            payload: &entry.payload[..entry.payload_len],
        })
    }

    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    pub(crate) fn retire_entries(&mut self) {
        self.entries = [None; DEDUP_CAPACITY];
    }

    fn find(&self, key: DedupKey) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.key == key))
    }
    fn entry_mut(&mut self, key: DedupKey) -> Result<&mut Entry, DedupError> {
        let index = self.find(key).ok_or(DedupError::Stale)?;
        Ok(self.entries[index].as_mut().expect("found"))
    }

    fn evictable_slot(&self, now_ms: u64) -> Option<usize> {
        self.entries.iter().position(|entry| {
            entry.is_some_and(|entry| {
                matches!(
                    entry.state,
                    EntryState::Completed | EntryState::ExpiredNonReplayable
                ) && now_ms.saturating_sub(entry.first_seen_ms) >= DEDUP_TTL_MS
            })
        })
    }
}

impl Default for DedupCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
