//! Kernel state stash for hot migration.
//!
//! A Cell about to be replaced serialises its state and `stash`es it under a
//! key; the replacement instance `restore`s it on startup. Keeping the bytes in
//! the kernel (not a file) means the transfer works before the VFS is reachable
//! and outlives the old cell's address space.
//!
//! Every entry belongs to the incarnation that first wrote it (Phase 09A,
//! `.agents/261030-1945-ginux-lessons-hardening/phase-09a-cp1.md`): only that
//! owner may write, read or clear it while it is `Open`. `SpawnReplacement`
//! seals the old incarnation's entries to the new one; from then on only that
//! destination may read or clear (consume) them. A supervisor may query the
//! length or abort, never read the bytes. A guessed key grants nothing. This is
//! byte transport only: business revision/fencing belongs to the provider's
//! own state authority.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Upper bound on a single stashed blob (1 MB) — bounds kernel memory held on
/// a cell's behalf for one slot.
pub const MAX_STASH_LEN: usize = 1024 * 1024;

/// Upper bound on the number of distinct stash slots. Caps total kernel memory
/// the stash can hold (≤ MAX_ENTRIES × MAX_STASH_LEN).
pub const MAX_ENTRIES: usize = 64;

/// Slots one incarnation may own, so one Cell cannot fill the table.
pub const MAX_ENTRIES_PER_OWNER: usize = 8;

/// A Cell incarnation: logical id plus the generation of this instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Incarnation {
    pub cell: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Open,
    Sealed { destination: Incarnation },
}

struct Entry {
    owner: Incarnation,
    phase: Phase,
    bytes: Vec<u8>,
}

impl Entry {
    fn readable_by(&self, caller: Incarnation) -> bool {
        match self.phase {
            Phase::Open => self.owner == caller,
            Phase::Sealed { destination } => destination == caller,
        }
    }
}

static STASH: Spinlock<BTreeMap<u64, Entry>> = Spinlock::new(BTreeMap::new());

/// Upper bound for a command line carried by one launch transaction.
pub(crate) const MAX_SPAWN_ARGV_LEN: usize = 512;

/// Stage `bytes` as the command line for this task's next launch.
///
/// The value lives on the task itself rather than in a keyed table: a staged
/// command line is one task's own state, so no other task's syscall — and no
/// cleanup for another task's failed launch — can address it. Returns the
/// number of bytes stored, or 0 when the payload exceeds the bound or the task
/// has no live record.
pub(crate) fn stage_spawn_argv(task_id: usize, bytes: &[u8]) -> usize {
    if bytes.len() > MAX_SPAWN_ARGV_LEN {
        return 0;
    }
    let mut guard = crate::task::SCHEDULER.lock();
    let Some(sched) = guard.as_mut() else {
        return 0;
    };
    let Some(task) = sched.tasks.get_mut(&task_id) else {
        return 0;
    };
    // Fallible copy: `to_vec` allocates infallibly, and a heap that cannot serve it
    // would halt the kernel instead of refusing the restore (0, the same sentinel
    // the other refusal paths return).
    let mut owned: Vec<u8> = Vec::new();
    if owned.try_reserve_exact(bytes.len()).is_err() {
        log::warn!("[stash] no heap for {} byte argv — refused", bytes.len());
        return 0;
    }
    owned.extend_from_slice(bytes);
    task.staged_argv = Some(owned);
    bytes.len()
}

/// Consume the command line this task staged, at the launch boundary, so a
/// failed attempt cannot leak it to a later child. The returned allocation is
/// owned by the unpublished launch.
pub(crate) fn take_spawn_argv(task_id: usize) -> Option<Vec<u8>> {
    take_task_argv(task_id, |task| &mut task.staged_argv)
}

/// Consume the command line this task's spawner handed it before it became
/// runnable. One-shot: the child reads it at most once.
pub(crate) fn take_inherited_argv(task_id: usize) -> Option<Vec<u8>> {
    take_task_argv(task_id, |task| &mut task.inherited_argv)
}

fn take_task_argv(
    task_id: usize,
    select: impl FnOnce(&mut crate::task::tcb::Task) -> &mut Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    crate::task::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&task_id))
        .and_then(|task| select(task).take())
}

/// Discard a task's staged, not-yet-launched command line.
///
/// Used after a rejected launch attempt so no command line can survive to a
/// later child. An *inherited* command line is never touched here: it belongs to
/// the receiving task until that task reads it or dies.
///
/// Lock contract: every caller of `handle_syscall` drops its own lock guards
/// before the staged-argv cleanup runs, because a guard created inside the
/// dispatch is dropped before the cleanup that was created ahead of the match.
pub(crate) fn discard_spawn_argv(task_id: usize) {
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&task_id) {
            task.staged_argv = None;
        }
    }
}

/// Store `bytes` under `key` for `caller`. Returns the bytes stored (clamped to
/// [`MAX_STASH_LEN`]) or 0 when refused: the key belongs to another incarnation
/// or is sealed, the caller already owns [`MAX_ENTRIES_PER_OWNER`] slots, the
/// table is full even after dropping open slots of dead owners, or no heap.
///
/// `is_live` answers whether an incarnation still exists. It is called with the
/// stash lock released, so it may take the scheduler lock.
pub fn stash(caller: Incarnation, key: u64, bytes: &[u8], is_live: impl Fn(Incarnation) -> bool) -> usize {
    if STASH.lock().len() >= MAX_ENTRIES {
        reclaim_dead_owners(is_live);
    }
    let n = bytes.len().min(MAX_STASH_LEN);
    // Fallible copy: an entry can be up to MAX_STASH_LEN, and `to_vec` allocates
    // infallibly — a full heap would halt the kernel instead of refusing the stash.
    let mut data: Vec<u8> = Vec::new();
    if data.try_reserve_exact(n).is_err() {
        log::warn!("[stash] no heap for {} byte entry — refused", n);
        return 0;
    }
    data.extend_from_slice(&bytes[..n]);
    let mut map = STASH.lock();
    match map.get_mut(&key) {
        Some(entry) if entry.owner == caller && entry.phase == Phase::Open => {
            entry.bytes = data;
            n
        }
        Some(_) => 0,
        None => {
            let owned = map.values().filter(|e| e.owner == caller).count();
            if owned >= MAX_ENTRIES_PER_OWNER || map.len() >= MAX_ENTRIES {
                log::warn!("[state-stash] refused new key: owner holds {owned}, table {}", map.len());
                return 0;
            }
            map.insert(key, Entry { owner: caller, phase: Phase::Open, bytes: data });
            n
        }
    }
}

/// Drop `Open` entries whose owner no longer exists, and sealed entries whose
/// destination no longer exists. Liveness is checked without the stash lock.
fn reclaim_dead_owners(is_live: impl Fn(Incarnation) -> bool) {
    let holders: Vec<(u64, Incarnation)> = STASH
        .lock()
        .iter()
        .map(|(key, e)| (*key, match e.phase {
            Phase::Open => e.owner,
            Phase::Sealed { destination } => destination,
        }))
        .collect();
    let dead: Vec<(u64, Incarnation)> = holders.into_iter().filter(|(_, h)| !is_live(*h)).collect();
    let mut map = STASH.lock();
    for (key, holder) in dead {
        // Re-check under the lock: the slot may have changed hands meanwhile.
        let still = map.get(&key).is_some_and(|e| match e.phase {
            Phase::Open => e.owner == holder,
            Phase::Sealed { destination } => destination == holder,
        });
        if still {
            map.remove(&key);
        }
    }
}

/// Copy the bytes for `key` into `buf` when `caller` may read them (owner while
/// open, destination once sealed). Returns the bytes copied, 0 when refused or
/// absent. The entry stays until cleared.
pub fn restore(caller: Incarnation, key: u64, buf: &mut [u8]) -> usize {
    let guard = STASH.lock();
    let Some(entry) = guard.get(&key).filter(|e| e.readable_by(caller)) else {
        return 0;
    };
    let n = entry.bytes.len().min(buf.len());
    buf[..n].copy_from_slice(&entry.bytes[..n]);
    n
}

/// Stored length of `key` for an authorized reader or a supervisor; 0 otherwise.
/// No bytes are returned: this is the coordinator's readiness probe.
pub fn status(caller: Incarnation, supervisor: bool, key: u64) -> usize {
    STASH
        .lock()
        .get(&key)
        .filter(|e| supervisor || e.readable_by(caller))
        .map_or(0, |e| e.bytes.len())
}

/// Remove `key`: the owner while open, the destination once sealed (consume), or
/// a supervisor (abort). Absent keys succeed; anything else is refused.
pub fn clear(caller: Incarnation, supervisor: bool, key: u64) -> Result<(), ()> {
    let mut map = STASH.lock();
    match map.get(&key) {
        None => Ok(()),
        Some(entry) if supervisor || entry.readable_by(caller) => {
            map.remove(&key);
            Ok(())
        }
        Some(_) => Err(()),
    }
}

/// Seal every open entry of `old` to `new`, at the replacement spawn. After this
/// the old incarnation can neither write nor read them. Returns the count sealed.
pub fn seal_for_replacement(old: Incarnation, new: Incarnation) -> usize {
    let mut sealed = 0;
    for entry in STASH.lock().values_mut() {
        if entry.owner == old && entry.phase == Phase::Open {
            entry.phase = Phase::Sealed { destination: new };
            sealed += 1;
        }
    }
    sealed
}

#[cfg(feature = "test-hooks")]
pub(crate) fn snapshot() -> alloc::vec::Vec<(u64, Vec<u8>)> {
    STASH
        .lock()
        .iter()
        .map(|(key, entry)| (*key, entry.bytes.clone()))
        .collect()
}

/// Boot-time self-test of the state-transfer primitive: stash a sentinel under
/// a scratch key, restore it, and confirm the bytes round-trip. Logs the
/// outcome so an integration test can assert it. The scratch entry is removed
/// afterwards so it never collides with a real cell's state.
pub fn self_test() {
    const SCRATCH_KEY: u64 = 0xFFFF_FFFF_FFFF_FFFEu64;
    let kernel = Incarnation { cell: u64::MAX, generation: u64::MAX };
    let stranger = Incarnation { cell: u64::MAX - 1, generation: 1 };
    let sentinel: [u8; 8] = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0x12, 0x34];
    stash(kernel, SCRATCH_KEY, &sentinel, |_| true);
    let mut buf = [0u8; 8];
    let n = restore(kernel, SCRATCH_KEY, &mut buf);
    let foreign = restore(stranger, SCRATCH_KEY, &mut [0u8; 8]);
    STASH.lock().remove(&SCRATCH_KEY);
    if n == 8 && buf == sentinel && foreign == 0 {
        log::info!("state-stash: round-trip OK, foreign read refused");
    } else {
        log::error!("state-stash: round-trip FAILED (n={}, foreign={}, buf={:?})", n, foreign, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Incarnation = Incarnation { cell: 0x7a01, generation: 3 };
    const A_NEXT: Incarnation = Incarnation { cell: 0x7a01, generation: 4 };
    const B: Incarnation = Incarnation { cell: 0x7b01, generation: 1 };
    const MALLORY: Incarnation = Incarnation { cell: 0x7c01, generation: 1 };

    fn guard() -> impl Drop {
        let g = crate::TEST_STATE_LOCK.lock();
        STASH.lock().clear();
        g
    }

    #[test]
    fn owner_round_trip_and_overwrite() {
        let _g = guard();
        assert_eq!(stash(A, 42, &[1, 2, 3, 4, 5], |_| true), 5);
        assert_eq!(stash(A, 42, &[9, 8], |_| true), 2);
        let mut out = [0u8; 5];
        assert_eq!(restore(A, 42, &mut out), 2);
        assert_eq!(&out[..2], &[9, 8]);
    }

    #[test]
    fn guessed_key_grants_nothing() {
        let _g = guard();
        stash(A, 0x00A3_0000_0000_0001, b"secret", |_| true);
        let mut out = [0u8; 6];
        assert_eq!(restore(MALLORY, 0x00A3_0000_0000_0001, &mut out), 0);
        assert_eq!(status(MALLORY, false, 0x00A3_0000_0000_0001), 0);
        assert_eq!(stash(MALLORY, 0x00A3_0000_0000_0001, b"forged", |_| true), 0);
        assert_eq!(clear(MALLORY, false, 0x00A3_0000_0000_0001), Err(()));
        assert_eq!(restore(A, 0x00A3_0000_0000_0001, &mut out), 6);
        assert_eq!(&out, b"secret");
    }

    #[test]
    fn a_later_incarnation_is_not_the_owner() {
        let _g = guard();
        stash(A, 7, b"old", |_| true);
        assert_eq!(restore(A_NEXT, 7, &mut [0u8; 3]), 0);
        assert_eq!(clear(A_NEXT, false, 7), Err(()));
    }

    #[test]
    fn sealing_hands_the_entry_to_the_destination_only() {
        let _g = guard();
        stash(A, 7, b"state", |_| true);
        stash(B, 8, b"other", |_| true);
        assert_eq!(seal_for_replacement(A, A_NEXT), 1);
        let mut out = [0u8; 5];
        assert_eq!(restore(A, 7, &mut out), 0, "old incarnation lost access");
        assert_eq!(stash(A, 7, b"late!", |_| true), 0, "old writer fenced");
        assert_eq!(restore(MALLORY, 7, &mut out), 0);
        assert_eq!(restore(A_NEXT, 7, &mut out), 5);
        assert_eq!(&out, b"state");
        assert_eq!(restore(A_NEXT, 7, &mut out), 5, "repeatable until consumed");
        assert_eq!(clear(A_NEXT, false, 7), Ok(()));
        assert_eq!(restore(A_NEXT, 7, &mut out), 0, "consumed");
        assert_eq!(restore(B, 8, &mut out), 5, "other owners unaffected");
    }

    #[test]
    fn supervisor_sees_length_and_may_abort_but_never_reads() {
        let _g = guard();
        stash(A, 9, b"payload", |_| true);
        assert_eq!(status(MALLORY, true, 9), 7);
        assert_eq!(status(MALLORY, false, 9), 0);
        assert_eq!(restore(MALLORY, 9, &mut [0u8; 7]), 0);
        assert_eq!(clear(MALLORY, true, 9), Ok(()));
        assert_eq!(status(A, false, 9), 0);
    }

    #[test]
    fn one_owner_cannot_fill_the_table_and_dead_owners_are_reclaimed() {
        let _g = guard();
        for key in 0..MAX_ENTRIES_PER_OWNER as u64 {
            assert_eq!(stash(A, 100 + key, b"x", |_| true), 1);
        }
        assert_eq!(stash(A, 999, b"x", |_| true), 0);
        for i in 0..(MAX_ENTRIES - MAX_ENTRIES_PER_OWNER) as u64 {
            let owner = Incarnation { cell: 0x9000 + i, generation: 1 };
            assert_eq!(stash(owner, 1000 + i, b"x", |_| true), 1);
        }
        assert_eq!(stash(B, 5000, b"x", |_| true), 0, "table full of live owners");
        assert_eq!(stash(B, 5000, b"x", |i| i != A), 1, "A's dead slots reclaimed");
        assert_eq!(restore(A, 100, &mut [0u8; 1]), 0);
    }

    #[test]
    fn spawn_argv_is_private_and_one_shot() {
        let _g = guard();
        const TID: usize = 0x1234;
        const FORMER_DERIVED_KEY: u64 = 0x0061_7267_7600_0000 ^ ((TID as u64) << 32);
        assert_eq!(stage_spawn_argv(TID, b"argv"), 0);
        assert_eq!(restore(A, FORMER_DERIVED_KEY, &mut [0u8; 4]), 0);
        assert_eq!(take_spawn_argv(TID), None);
    }
}
