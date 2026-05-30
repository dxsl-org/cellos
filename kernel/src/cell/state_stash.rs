//! Kernel state stash for hot-migration (Phase 20).
//!
//! A Cell about to be replaced serialises its state and `stash`es it under a
//! well-known key; the replacement instance `restore`s it on startup. This is
//! the kernel-mediated state-transfer primitive that survives a cell respawn —
//! the live, message-preserving orchestrator in `hotswap.rs` builds on top of
//! it. Keeping the bytes in the kernel (not a file) means the transfer works
//! before the VFS is reachable and outlives the old cell's address space.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::sync::Spinlock;

/// Upper bound on a single stashed blob (64 KB) — matches the hotswap
/// serialise buffer and bounds kernel memory held on a cell's behalf.
pub const MAX_STASH_LEN: usize = 64 * 1024;

/// key → serialized state bytes. Keys are cell-chosen (typically a stable
/// FNV hash of the cell name), so a replacement instance reads the same slot.
static STASH: Spinlock<BTreeMap<u64, Vec<u8>>> = Spinlock::new(BTreeMap::new());

/// Store `bytes` under `key`, replacing any previous value. Returns the number
/// of bytes stored (clamped to [`MAX_STASH_LEN`]).
pub fn stash(key: u64, bytes: &[u8]) -> usize {
    let n = bytes.len().min(MAX_STASH_LEN);
    STASH.lock().insert(key, bytes[..n].to_vec());
    n
}

/// Copy stashed bytes for `key` into `buf`. Returns the number of bytes written
/// (0 if no state is stashed). The stash entry is left in place so multiple
/// readers (or a retry) can recover it.
pub fn restore(key: u64, buf: &mut [u8]) -> usize {
    let guard = STASH.lock();
    let Some(bytes) = guard.get(&key) else { return 0 };
    let n = bytes.len().min(buf.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    n
}

/// Boot-time self-test of the state-transfer primitive: stash a sentinel under
/// a scratch key, restore it, and confirm the bytes round-trip. Logs the
/// outcome so an integration test can assert it. The scratch entry is removed
/// afterwards so it never collides with a real cell's state.
pub fn self_test() {
    const SCRATCH_KEY: u64 = 0xFFFF_FFFF_FFFF_FFFEu64;
    let sentinel: [u8; 8] = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0x12, 0x34];
    stash(SCRATCH_KEY, &sentinel);
    let mut buf = [0u8; 8];
    let n = restore(SCRATCH_KEY, &mut buf);
    STASH.lock().remove(&SCRATCH_KEY);
    if n == 8 && buf == sentinel {
        log::info!("state-stash: round-trip OK");
    } else {
        log::error!("state-stash: round-trip FAILED (n={}, buf={:?})", n, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stash_restore_round_trip() {
        let key = 42u64;
        let data = [1u8, 2, 3, 4, 5];
        assert_eq!(stash(key, &data), 5);
        let mut out = [0u8; 5];
        assert_eq!(restore(key, &mut out), 5);
        assert_eq!(out, data);
    }

    #[test]
    fn restore_missing_key_returns_zero() {
        let mut out = [0u8; 8];
        assert_eq!(restore(0xDEAD_0000_0000_0001, &mut out), 0);
    }

    #[test]
    fn stash_overwrites_previous() {
        let key = 7u64;
        stash(key, &[9u8; 4]);
        stash(key, &[1u8, 2]);
        let mut out = [0u8; 4];
        assert_eq!(restore(key, &mut out), 2);
        assert_eq!(&out[..2], &[1u8, 2]);
    }
}
