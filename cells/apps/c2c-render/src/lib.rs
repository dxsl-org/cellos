#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;

pub mod worker_runtime;

/// Hardware-monotonic timestamps normalized to ns. Frequency is mandatory:
/// an unavailable clock is an error, never a guessed unit or a wall-clock fallback.
#[derive(Clone, Copy)]
pub struct Clock {
    frequency: u64,
}

impl Clock {
    pub fn new() -> Result<Self, String> {
        ostd::syscall::sys_get_scheduler_ticks()
            .ok_or_else(|| String::from("monotonic GetTime syscall unavailable"))?;
        let frequency = ostd::syscall::sys_get_timer_freq()
            .ok_or_else(|| String::from("monotonic timer frequency unavailable"))?;
        Ok(Self { frequency })
    }

    pub fn now_ns(self) -> u64 {
        let ticks = ostd::syscall::sys_get_time();
        ((u128::from(ticks) * 1_000_000_000) / u128::from(self.frequency)).min(u128::from(u64::MAX))
            as u64
    }

    pub fn elapsed_ns(self, start: u64) -> u64 {
        self.now_ns().saturating_sub(start)
    }

    pub fn frequency(self) -> u64 {
        self.frequency
    }
}

pub fn pixel_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

/// Nearest-rank percentile, on a sorted nonempty list.
pub fn percentile(sorted: &[u64], percent: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

#[cfg(test)]
mod tests {
    use super::{percentile, pixel_hash};

    #[test]
    fn nearest_rank_percentiles_keep_tail_samples() {
        assert_eq!(percentile(&[1, 2, 3, 4], 50), 2);
        assert_eq!(percentile(&[1, 2, 3, 4], 95), 4);
        assert_eq!(percentile(&[], 99), 0);
    }

    #[test]
    fn pixel_hash_is_byte_sensitive() {
        assert_eq!(pixel_hash(b""), 0xcbf29ce484222325);
        assert_ne!(pixel_hash(&[1, 2, 3]), pixel_hash(&[1, 2, 4]));
    }
}
