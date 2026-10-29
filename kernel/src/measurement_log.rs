//! Per-Cell integrity measurement log (Linux IMA-style, TPM-free).
//!
//! On every `spawn_from_path`, the loader records `SHA-256(elf_bytes)` here
//! BEFORE the cell is scheduled to run. Entries form an append-only log plus a
//! rolling aggregate (`agg = SHA256(agg || entry_hash)`) — the single value a
//! future DICE/EAT remote-attestation token signs to prove the exact software
//! that ran on this device. See
//! `docs/research/research-cell-security-permissions.md` §3.6.
//!
//! This is *measurement* (evidence), not *enforcement*: it does not block a
//! cell. Signature-based rejection (Cell binary signing) is a separate,
//! orthogonal gate. Pairing the two gives "measured + verified launch".

use crate::sync::Spinlock;
use alloc::string::String;
use alloc::vec::Vec;

/// Soft cap on retained entries — bounds kernel memory. Beyond this individual
/// entries stop being appended (the aggregate still advances, so attestation
/// integrity is preserved) and a warning is logged. Cell spawns are bounded in
/// practice on G1.
const MAX_ENTRIES: usize = 256;

/// Most detailed entries one creator may retain, counting entries already
/// committed through prepared publication and outstanding promises. A creator
/// that reaches it through its OWN committed entries keeps going: `reserve`
/// evicts that creator's oldest committed detailed entry (the aggregate, which
/// already covers it, is untouched), so one creator can never occupy more than
/// its share and a long-lived creator is never locked out. Only a creator whose
/// outstanding promises alone reach the share is refused. Entries of other
/// creators and ordinary (`commit_staged`) launches are never evicted.
pub(crate) const CREATOR_SHARE: usize = MAX_ENTRIES / 4;

/// Identity of the Cell that prepared a child: `(cell id, generation)`.
pub type Creator = (u64, u64);

/// Creator recorded on entries that no prepared publication produced.
pub const NO_CREATOR: Creator = (0, 0);

/// Per-creator count of OUTSTANDING promises. A row exists only while its count
/// is non-zero, so the table never exceeds `MAX_ENTRIES` rows.
struct Share {
    creator: Creator,
    promised: usize,
}

/// One measured cell: its TID, the SHA-256 of its ELF image, its path, and the
/// creator that prepared it (`NO_CREATOR` for ordinary launches).
pub struct MeasureEntry {
    pub tid: u32,
    pub hash: [u8; 32],
    pub path: String,
    pub creator: Creator,
}

struct Log {
    entries: Vec<MeasureEntry>,
    aggregate: [u8; 32],
    /// Detailed-log slots promised to prepared (not yet published) children.
    /// Invariants under `LOG`: `entries.len() + promises <= MAX_ENTRIES` and
    /// `entries.capacity() >= entries.len() + promises`, so a reserved commit
    /// appends without allocating and no ordinary launch can consume the slot.
    promises: usize,
    /// Per-creator share accounting for prepared publications.
    shares: Vec<Share>,
}

static LOG: Spinlock<Log> = Spinlock::new(Log {
    entries: Vec::new(),
    aggregate: [0u8; 32],
    promises: 0,
    shares: Vec::new(),
});

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart kernel; called only from the fault/panic path with interrupts
/// disabled. Force-unlocking an already-free Spinlock is a no-op.
pub unsafe fn force_unlock_locks() {
    LOG.force_unlock();
}

fn advance_aggregate(log: &mut Log, hash: &[u8; 32]) {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&log.aggregate);
    buf[32..].copy_from_slice(hash);
    log.aggregate = crate::sha256::sha256(&buf);
}

#[cfg(cellos_p01_trace)]
fn attribute_log_buffers(log: &Log) {
    if log.entries.capacity() != 0 {
        crate::memory::heap::p01_owner::shared_buffer(log.entries.as_ptr() as usize,
            core::alloc::Layout::array::<MeasureEntry>(log.entries.capacity()).unwrap(),
            crate::memory::heap::p01_owner::LOG_SHARED);
    }
}

#[cfg(cellos_p01_trace)]
fn attribute_path(path: &String) {
    if path.capacity() != 0 {
        crate::memory::heap::p01_owner::shared_buffer(path.as_ptr() as usize,
            core::alloc::Layout::array::<u8>(path.capacity()).unwrap(),
            crate::memory::heap::p01_owner::LOG_PATH);
    }
}

fn emit_audit(tid: usize, hash: &[u8; 32]) {
    let hp = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
    crate::audit::log_event(
        crate::audit::AuditEvent::CellMeasure,
        &crate::audit::encode_u32x2(tid as u32, hp),
    );
}

/// Commit a digest and owned label staged before atomic publication.
///
/// Ordinary launches never consume a slot promised to a prepared child: they
/// append only while `len + promises < MAX_ENTRIES` and while capacity for the
/// new entry *and* every outstanding promise can be reserved. Otherwise the
/// aggregate-only fallback below applies, exactly as at the historical cap.
pub(crate) fn commit_staged(tid: usize, path: String, hash: [u8; 32]) {
    {
        let mut log = LOG.lock();
        #[cfg(cellos_p01_trace)]
        let _semantic = crate::memory::heap::p01_owner::creation(None, crate::memory::heap::p01_owner::LOG_SHARED);
        #[cfg(cellos_p01_trace)]
        attribute_log_buffers(&log);
        advance_aggregate(&mut log, &hash);

        let promised = log.promises;
        if log.entries.len() + promised < MAX_ENTRIES {
            // Growth is fallible: `Vec::push` reallocates through the infallible
            // allocator, and a capacity doubling that cannot be satisfied halts
            // the kernel instead of dropping a measurement. Dropping the entry
            // keeps the aggregate advancing, which is the property attestation
            // depends on. Capacity for outstanding promises is kept in addition
            // to this entry.
            if log.entries.try_reserve(promised + 1).is_err() {
                log::warn!(
                    "[measure] no heap for entry {} — aggregate still advancing",
                    log.entries.len()
                );
            } else {
                #[cfg(cellos_p01_trace)]
                attribute_path(&path);
                log.entries.push(MeasureEntry {
                    tid: tid as u32,
                    hash,
                    path,
                    creator: NO_CREATOR,
                });
            }
        } else {
            log::warn!(
                "[measure] log full ({} entries, {} promised) — aggregate still advancing",
                log.entries.len(),
                promised
            );
        }
    }
    emit_audit(tid, &hash);
}

/// Promise of one detailed log entry for a prepared, unpublished child.
///
/// `reserve` charges bounded capacity before any visible publication; `commit`
/// consumes exactly that promise, appends and advances the aggregate together,
/// and cannot allocate or fail. Dropping an uncommitted guard releases the
/// promise without touching the aggregate or the log.
pub(crate) struct PublicationReservation {
    armed: bool,
    creator: Creator,
}

impl PublicationReservation {
    /// Approximate retained bytes one committed entry costs (excluding the path
    /// bytes, which the staged label already owns).
    pub(crate) const ENTRY_BYTES: usize = core::mem::size_of::<MeasureEntry>();

    /// `creator` is charged one outstanding promise. At its share through its own
    /// committed entries (or with the log full), its oldest committed entry is
    /// evicted to make room; it is refused only when its promises alone reach the
    /// share or when the capacity is held by other creators' entries. Eviction
    /// happens only after every fallible step succeeded.
    pub(crate) fn reserve(creator: Creator) -> Result<Self, types::ViError> {
        let mut log = LOG.lock();
        let slot = log.shares.iter().position(|share| share.creator == creator);
        let promised = slot.map_or(0, |i| log.shares[i].promised);
        if promised >= CREATOR_SHARE {
            return Err(types::ViError::OutOfMemory);
        }
        let own = log.entries.iter().position(|entry| entry.creator == creator);
        let committed = if own.is_some() {
            log.entries.iter().filter(|entry| entry.creator == creator).count()
        } else {
            0
        };
        let wanted = log
            .entries
            .len()
            .checked_add(log.promises)
            .and_then(|n| n.checked_add(1))
            .ok_or(types::ViError::OutOfMemory)?;
        let evict = if wanted > MAX_ENTRIES || promised + committed >= CREATOR_SHARE {
            // The detailed record is required for a prepared child. Room is made
            // only from this creator's own oldest entry; with none, preparation is
            // refused, never silently omitted.
            Some(own.ok_or(types::ViError::OutOfMemory)?)
        } else {
            None
        };
        let additional = log.promises + 1;
        #[cfg(cellos_p01_trace)]
        let _semantic = crate::memory::heap::p01_owner::creation(None, crate::memory::heap::p01_owner::LOG_SHARED);
        log.entries
            .try_reserve(additional)
            .map_err(|_| types::ViError::OutOfMemory)?;
        match slot {
            Some(i) => log.shares[i].promised += 1,
            None => {
                log.shares
                    .try_reserve(1)
                    .map_err(|_| types::ViError::OutOfMemory)?;
                log.shares.push(Share { creator, promised: 1 });
            }
        }
        if let Some(index) = evict {
            // Capacity only shrinks the entry count; the aggregate is untouched.
            drop(log.entries.remove(index));
        }
        log.promises += 1;
        Ok(Self { armed: true, creator })
    }

    /// Publish the promised entry. The path was staged and owned before this
    /// call; nothing here allocates.
    pub(crate) fn commit(mut self, tid: usize, path: String, hash: [u8; 32]) {
        {
            let mut log = LOG.lock();
            #[cfg(cellos_p01_trace)]
            attribute_log_buffers(&log);
            assert!(
                log.promises != 0 && log.entries.len() < log.entries.capacity(),
                "measurement publication promise lost its reserved slot"
            );
            advance_aggregate(&mut log, &hash);
            log.promises -= 1;
            // The promise becomes a committed entry: it stops counting as outstanding
            // for the creator (no allocation: the row is only shrunk or swapped out).
            if let Some(i) = log.shares.iter().position(|share| share.creator == self.creator) {
                log.shares[i].promised = log.shares[i].promised.saturating_sub(1);
                if log.shares[i].promised == 0 {
                    log.shares.swap_remove(i);
                }
            }
            #[cfg(cellos_p01_trace)]
            attribute_path(&path);
            log.entries.push(MeasureEntry {
                tid: tid as u32,
                hash,
                path,
                creator: self.creator,
            });
        }
        self.armed = false;
        emit_audit(tid, &hash);
    }
}

impl Drop for PublicationReservation {
    fn drop(&mut self) {
        if self.armed {
            let mut log = LOG.lock();
            log.promises = log.promises.saturating_sub(1);
            // The promise returns to the creator.
            if let Some(i) = log.shares.iter().position(|share| share.creator == self.creator) {
                log.shares[i].promised = log.shares[i].promised.saturating_sub(1);
                if log.shares[i].promised == 0 {
                    log.shares.swap_remove(i);
                }
            }
        }
    }
}

/// (entries, capacity, promises, maximum) for publication regression tests.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_shape() -> (usize, usize, usize, usize) {
    let log = LOG.lock();
    (log.entries.len(), log.entries.capacity(), log.promises, MAX_ENTRIES)
}

/// `(outstanding promises, retained committed entries)` charged to `creator`.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_creator_used(creator: Creator) -> (usize, usize) {
    let log = LOG.lock();
    (
        log.shares.iter().find(|share| share.creator == creator).map_or(0, |share| share.promised),
        log.entries.iter().filter(|entry| entry.creator == creator).count(),
    )
}

/// Path of the oldest retained entry committed by `creator`.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_oldest_path(creator: Creator) -> Option<String> {
    LOG.lock().entries.iter().find(|entry| entry.creator == creator).map(|entry| entry.path.clone())
}

/// Rolling aggregate over every measured cell (for future remote attestation).
pub fn aggregate() -> [u8; 32] {
    LOG.lock().aggregate
}

/// Number of entries currently retained (diagnostics).
pub fn entry_count() -> usize {
    LOG.lock().entries.len()
}
