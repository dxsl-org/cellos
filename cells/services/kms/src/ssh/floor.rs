//! Per-principal replay floors (O-4): four principals, the least recently
//! advanced is evicted. Eviction resets that principal's floor to zero.

use super::limits::FLOOR_SLOTS;
use super::Principal;

#[derive(Debug, Clone, Copy)]
struct Entry {
    principal: Principal,
    floor: u64,
    seq: u64,
}

#[derive(Debug)]
pub(crate) struct FloorTable {
    entries: [Option<Entry>; FLOOR_SLOTS],
    seq: u64,
}

impl FloorTable {
    pub(crate) const fn new() -> Self {
        Self {
            entries: [None; FLOOR_SLOTS],
            seq: 0,
        }
    }

    /// The principal's floor; `0` for a principal that is not remembered.
    pub(crate) fn floor(&self, principal: Principal) -> u64 {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.principal == principal)
            .map_or(0, |entry| entry.floor)
    }

    /// A request id is acceptable when nonzero and strictly above the floor.
    pub(crate) fn admits(&self, principal: Principal, request_id: u64) -> bool {
        request_id != 0 && request_id > self.floor(principal)
    }

    /// Record a successful operation. Only called after every check passed.
    pub(crate) fn advance(&mut self, principal: Principal, request_id: u64) {
        self.seq = self.seq.saturating_add(1);
        let seq = self.seq;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.principal == principal)
        {
            entry.floor = request_id;
            entry.seq = seq;
            return;
        }
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, entry)| entry.map_or(0, |entry| entry.seq))
                    .map(|(index, _)| index)
            })
            .unwrap_or(0);
        if let Some(entry) = self.entries.get_mut(slot) {
            *entry = Some(Entry {
                principal,
                floor: request_id,
                seq,
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }
}
