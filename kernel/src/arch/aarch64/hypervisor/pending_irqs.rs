//! Bounded coalescing bitset for a vCPU's pending virtual IRQs.
//!
//! A pending interrupt is set-membership, not a queue: an INTID is either
//! pending or it isn't, and re-raising an already-pending INTID is a no-op —
//! this matches real GICv2 pending-state coalescing. Storage is a fixed
//! `[u64; WORDS]` sized to the full GICv2 INTID space, so the memory used by
//! a vCPU's pending set is constant regardless of how many times (or how
//! fast) a guest raises interrupts.

/// GICv2 INTID space is 0..=1019 (SGI/PPI/SPI); round up to a whole number of
/// `u64` words for the bitset.
const MAX_INTID: usize = 1024;
const WORDS: usize = MAX_INTID / 64;

/// Fixed-size (128 B) coalescing set of pending INTIDs for one vCPU.
pub struct PendingIrqs {
    bits: [u64; WORDS],
}

impl Default for PendingIrqs {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingIrqs {
    pub const fn new() -> Self {
        Self { bits: [0; WORDS] }
    }

    fn word_bit(intid: u32) -> Option<(usize, u32)> {
        let intid = intid as usize;
        if intid >= MAX_INTID {
            return None;
        }
        Some((intid / 64, (intid % 64) as u32))
    }

    /// Mark `intid` pending. Idempotent: raising the same INTID any number of
    /// times only ever sets one bit, so a guest spamming a single masked
    /// INTID cannot grow this structure's memory use. Out-of-range INTIDs are
    /// silently ignored — defense in depth; the syscall layer already
    /// rejects `intid` > 1019 before `inject_irq` is reached.
    pub fn set(&mut self, intid: u32) {
        if let Some((w, b)) = Self::word_bit(intid) {
            self.bits[w] |= 1 << b;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// Remove and return the lowest-numbered pending INTID, if any.
    ///
    /// Ascending-INTID order is used as the GICH-LR load order; it is not a
    /// GIC-mandated priority order (arrival order isn't guaranteed by real
    /// GIC hardware either), just a stable, cheap-to-compute one.
    pub fn take_lowest(&mut self) -> Option<u32> {
        for (i, word) in self.bits.iter_mut().enumerate() {
            if *word != 0 {
                let bit = word.trailing_zeros();
                *word &= !(1 << bit);
                return Some((i as u32) * 64 + bit);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spamming_one_intid_stays_bounded() {
        let mut p = PendingIrqs::new();
        for _ in 0..10_000 {
            p.set(42);
        }
        assert_eq!(p.take_lowest(), Some(42));
        assert!(p.is_empty());
        assert_eq!(p.take_lowest(), None);
    }

    #[test]
    fn take_lowest_is_ascending() {
        let mut p = PendingIrqs::new();
        p.set(100);
        p.set(3);
        p.set(64);
        assert_eq!(p.take_lowest(), Some(3));
        assert_eq!(p.take_lowest(), Some(64));
        assert_eq!(p.take_lowest(), Some(100));
        assert!(p.is_empty());
    }

    #[test]
    fn out_of_range_intid_ignored() {
        let mut p = PendingIrqs::new();
        p.set(1024);
        p.set(u32::MAX);
        assert!(p.is_empty());
    }
}
