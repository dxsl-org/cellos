//! Per-Cell VT-d domain and Domain ID (DID) allocator.

use super::slpt::VtdSlpt;
use alloc::collections::BTreeSet;

/// DID capacity for the widest `GCAP.ND` encoding (16-bit DIDs).
pub const DID_WORDS: usize = (1 << 16) / 64;

/// Per-Cell VT-d domain (SLPT + DID + exact requester IDs).
pub struct VtdDomain {
    pub slpt: VtdSlpt,
    pub did: u16,
    pub bdfs: BTreeSet<u16>,
}

impl VtdDomain {
    pub fn new(slpt: VtdSlpt, did: u16) -> Self {
        Self {
            slpt,
            did,
            bdfs: BTreeSet::new(),
        }
    }
}

/// Decode domain-id capacity from `GCAP.ND` (bits[2:0]).
pub fn did_limit(gcap: u64) -> Option<u32> {
    match (gcap & 0x7) as u32 {
        0 => Some(16),
        1 => Some(64),
        2 => Some(256),
        3 => Some(1024),
        4 => Some(4096),
        5 => Some(16384),
        6 => Some(65536),
        _ => None,
    }
}

pub struct DidAllocator {
    used: [u64; DID_WORDS],
    limit: u32,
}

impl DidAllocator {
    pub const fn new() -> Self {
        Self {
            used: [0; DID_WORDS],
            limit: 0,
        }
    }

    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit.min(1 << 16);
        self.used = [0; DID_WORDS];
        self.used[0] = 1; // DID 0 is reserved
    }

    pub fn allocate(&mut self) -> Option<u16> {
        if self.limit <= 1 {
            return None;
        }
        for (word_idx, word) in self.used.iter_mut().enumerate() {
            if *word == u64::MAX {
                continue;
            }
            let bit = (!*word).trailing_zeros() as usize;
            let id = (word_idx * 64 + bit) as u32;
            if id >= self.limit {
                return None;
            }
            *word |= 1u64 << bit;
            return Some(id as u16);
        }
        None
    }

    pub fn release(&mut self, did: u16) {
        let did = did as u32;
        if did == 0 || did >= self.limit {
            return;
        }
        let word_idx = (did / 64) as usize;
        let bit = (did % 64) as usize;
        if word_idx < DID_WORDS {
            self.used[word_idx] &= !(1u64 << bit);
        }
    }
}
impl Default for DidAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn did_limit_decodes_gcap_nd_only() {
        assert_eq!(did_limit(0), Some(16));
        assert_eq!(did_limit(2), Some(256));
        assert_eq!(did_limit(6), Some(1 << 16));
        assert_eq!(did_limit(7), None);
        assert_eq!(did_limit((0x66 << 16) | 2), Some(256));
    }

    #[test]
    fn did_allocator_issues_bounded_unique_ids_and_reuses_released_ones() {
        let mut dids = DidAllocator::new();
        assert_eq!(dids.allocate(), None);
        for limit in [16u32, 130, 1 << 16] {
            let mut dids = DidAllocator::new();
            dids.set_limit(limit);
            for expected in 1..limit {
                assert_eq!(dids.allocate(), Some(expected as u16));
            }
            assert_eq!(dids.allocate(), None);
            dids.release(0);
            if limit <= u16::MAX as u32 {
                dids.release(limit as u16);
            }
            assert_eq!(dids.allocate(), None);
            let freed = (limit / 2) as u16;
            dids.release(freed);
            assert_eq!(dids.allocate(), Some(freed));
            assert_eq!(dids.allocate(), None);
        }
        dids.set_limit(16);
        assert_eq!(dids.allocate(), Some(1));
    }
}
