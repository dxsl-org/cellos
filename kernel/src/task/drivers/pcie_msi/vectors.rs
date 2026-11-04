//! IDT vector pool for MSI/MSI-X allocations.
//!
//! The logical IRQ number a cell waits on (`WaitIrq`) is the vector itself, so
//! a contiguous vector block is also the contiguous IRQ block CP1 §4.1 promises.

use hal::x86_64::msi::is_msi_vector;

/// Allocation bitmap over the 256 IDT vectors; only MSI-range vectors are ever set.
pub struct VectorPool {
    used: [u64; 4],
}

impl VectorPool {
    pub const fn new() -> Self {
        Self { used: [0; 4] }
    }

    fn is_used(&self, vector: usize) -> bool {
        self.used[vector / 64] & (1 << (vector % 64)) != 0
    }

    fn set(&mut self, vector: usize, used: bool) {
        let bit = 1u64 << (vector % 64);
        if used {
            self.used[vector / 64] |= bit;
        } else {
            self.used[vector / 64] &= !bit;
        }
    }

    /// Reserve `count` contiguous MSI-range vectors whose first vector is a
    /// multiple of `align` (a power of two; MSI multi-message needs the block
    /// aligned to its size because the device ORs the message number into the
    /// data's low bits). Lowest fit wins.
    pub fn alloc(&mut self, count: usize, align: usize) -> Option<u8> {
        if count == 0 || !align.is_power_of_two() {
            return None;
        }
        let mut first = 0usize;
        while first + count <= 256 {
            let fits = (first..first + count).all(|v| is_msi_vector(v as u8) && !self.is_used(v));
            if fits {
                (first..first + count).for_each(|v| self.set(v, true));
                return Some(first as u8);
            }
            first += align;
        }
        None
    }

    /// Return a block from [`Self::alloc`].
    pub fn free(&mut self, first: u8, count: usize) {
        let first = first as usize;
        (first..(first + count).min(256)).for_each(|v| self.set(v, false));
    }

    #[cfg(test)]
    pub fn used_count(&self) -> u32 {
        self.used.iter().map(|w| w.count_ones()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hal::x86_64::msi::{VECTOR_FIRST, VECTOR_LAST};

    #[test]
    fn single_vectors_fill_the_range_and_skip_int80() {
        let mut pool = VectorPool::new();
        let mut got = alloc::vec::Vec::new();
        while let Some(v) = pool.alloc(1, 1) {
            got.push(v);
        }
        assert_eq!(got.first(), Some(&VECTOR_FIRST));
        assert_eq!(got.last(), Some(&VECTOR_LAST));
        assert!(!got.contains(&0x80));
        assert_eq!(got.len(), (VECTOR_LAST - VECTOR_FIRST) as usize);
    }

    #[test]
    fn aligned_blocks_never_straddle_int80_or_overlap() {
        let mut pool = VectorPool::new();
        let a = pool.alloc(4, 4).unwrap();
        assert_eq!(a % 4, 0);
        let b = pool.alloc(32, 32).unwrap();
        assert_eq!(b % 32, 0);
        assert!(!(b..b + 32).contains(&0x80));
        assert!(b >= a + 4);
        // A block that would need 0x80 is skipped, not split.
        let c = pool.alloc(32, 32).unwrap();
        assert!(c > 0x80 || c + 32 <= 0x80);
        assert_eq!(pool.used_count(), 68);
    }

    #[test]
    fn free_returns_vectors_for_reuse() {
        let mut pool = VectorPool::new();
        let a = pool.alloc(8, 8).unwrap();
        pool.free(a, 8);
        assert_eq!(pool.used_count(), 0);
        assert_eq!(pool.alloc(8, 8), Some(a));
    }

    #[test]
    fn exhaustion_and_bad_requests_fail() {
        let mut pool = VectorPool::new();
        assert_eq!(pool.alloc(0, 1), None);
        assert_eq!(pool.alloc(1, 3), None);
        assert_eq!(pool.alloc(256, 1), None);
        while pool.alloc(1, 1).is_some() {}
        assert_eq!(pool.alloc(1, 1), None);
    }
}
