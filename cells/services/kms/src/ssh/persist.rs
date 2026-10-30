//! T1 persistence for the one SSH identity (K4b, feature `ssh-dev-persist`,
//! `DEV_POSTURE`): a second implementation of the K5 [`SshPersist`] seam.
//!
//! Only passphrase-wrapped identities are written, as record v2 (see
//! [`record`]) into one of two files through [`SshBlobIo`], alternating. The
//! newest record is the one that *chains* (`successor.previous ==
//! digest(predecessor)`); revision numbers are never compared for order.
//!
//! What this does not give, stated plainly: no rollback protection (restoring
//! an older consistent pair is not detectable), no crash or power-loss
//! durability ("persisted" means a write returned `Ok` and an immediate
//! read-back in the same boot was byte-equal), a public dev root key (a
//! format check, not authentication), and a volatile failure counter. A
//! `Plain` identity is never written: the persister answers `VolatileOnly`
//! without reading the seed and without any I/O.
//!
//! This module is independent of the relay journal: it reaches storage only
//! through [`SshBlobIo`], holds no secret, and an SSH load or persist failure
//! changes nothing outside the SSH leaf.

use core::fmt;

use super::store::{RecordView, SecretView};
use super::{Loaded, PersistError, SshPersist, Stored};

pub(crate) mod blob_io;
pub(crate) mod devroot;
pub(crate) mod record;

use blob_io::{BlobError, Read, Slot, SshBlobIo, RECORD_MAX};
use record::Meta;

/// The newest valid record seen or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Head {
    pub slot: Slot,
    /// Digest of the whole record: the next record's `previous`.
    pub digest: [u8; 32],
    /// The record holds a key (not a tombstone).
    pub present: bool,
}

/// What one slot held at load time.
enum Seen {
    Absent,
    /// Present but not a valid record (torn, corrupt, foreign, oversize).
    Invalid,
    Valid(Meta),
}

pub(crate) struct FilePersister<IO: SshBlobIo> {
    pub(crate) io: IO,
    pub(crate) dirs_ready: bool,
    pub(crate) head: Option<Head>,
    /// Set by a load that found an unusable pair: the slot the repair import
    /// writes.
    pub(crate) repair_target: Option<Slot>,
}

impl<IO: SshBlobIo> fmt::Debug for FilePersister<IO> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilePersister")
            .field("dirs_ready", &self.dirs_ready)
            .field("head_slot", &self.head.map(|head| head.slot))
            .field("head_present", &self.head.map(|head| head.present))
            .field("repair_target", &self.repair_target)
            .finish_non_exhaustive()
    }
}

impl<IO: SshBlobIo> FilePersister<IO> {
    pub(crate) fn new(io: IO) -> Self {
        Self {
            io,
            dirs_ready: false,
            head: None,
            repair_target: None,
        }
    }

    fn ensure_dirs(&mut self) -> Result<(), BlobError> {
        if !self.dirs_ready {
            self.io.ensure_dirs()?;
            self.dirs_ready = true;
        }
        Ok(())
    }

    fn observe(&mut self, slot: Slot, buf: &mut [u8; RECORD_MAX]) -> Result<Seen, BlobError> {
        Ok(match self.io.read(slot, buf)? {
            Read::Absent => Seen::Absent,
            Read::Oversize => Seen::Invalid,
            Read::Present(len) => {
                match buf.get(..len).and_then(|bytes| record::meta(slot, bytes)) {
                    Some(meta) => Seen::Valid(meta),
                    None => Seen::Invalid,
                }
            }
        })
    }

    /// The winner is `slot`: remember it and report its state.
    fn adopt(&mut self, slot: Slot, meta: Meta, bytes: &[u8]) -> Loaded {
        if !meta.present {
            self.head = Some(Head {
                slot,
                digest: meta.digest,
                present: false,
            });
            return Loaded::Empty;
        }
        match bytes
            .get(..meta.len)
            .and_then(|record_bytes| record::identity(slot, record_bytes))
        {
            Some(identity) => {
                self.head = Some(Head {
                    slot,
                    digest: meta.digest,
                    present: true,
                });
                Loaded::Found {
                    revision: meta.revision,
                    identity,
                }
            }
            // The record validated a moment ago: unreachable, and fails
            // visibly if it is ever reached.
            None => {
                self.repair_target = Some(Slot::A);
                Loaded::Corrupt
            }
        }
    }

    /// The slot the next record goes to: a repair target if a load found an
    /// unusable pair, otherwise the slot that is not the head, else A.
    fn target_slot(&self) -> Slot {
        match (self.repair_target, self.head) {
            (Some(target), _) => target,
            (None, Some(head)) => head.slot.other(),
            (None, None) => Slot::A,
        }
    }

    /// Write protocol (CP1 section 4.7): dirs, encode, write, read back and
    /// compare, then and only then move the head.
    fn commit(
        &mut self,
        present: bool,
        encode: impl FnOnce(Slot, &[u8; 32], &mut [u8; RECORD_MAX]) -> Option<usize>,
    ) -> Result<Stored, PersistError> {
        self.ensure_dirs().map_err(|_| PersistError)?;
        let slot = self.target_slot();
        let previous = self.head.map_or([0; 32], |head| head.digest);
        let mut out = [0u8; RECORD_MAX];
        let len = encode(slot, &previous, &mut out).ok_or(PersistError)?;
        let bytes = out.get(..len).ok_or(PersistError)?;
        self.io.write(slot, bytes).map_err(|_| PersistError)?;
        let mut back = [0u8; RECORD_MAX];
        match self.io.read(slot, &mut back) {
            Ok(Read::Present(read_len)) if back.get(..read_len) == Some(bytes) => {}
            _ => return Err(PersistError),
        }
        self.head = Some(Head {
            slot,
            digest: record::digest(bytes),
            present,
        });
        self.repair_target = None;
        Ok(Stored::Written)
    }
}

impl<IO: SshBlobIo> SshPersist for FilePersister<IO> {
    fn load(&mut self) -> Loaded {
        self.head = None;
        self.repair_target = None;
        if self.ensure_dirs().is_err() {
            return Loaded::IoError;
        }
        let mut buf_a = [0u8; RECORD_MAX];
        let mut buf_b = [0u8; RECORD_MAX];
        let Ok(seen_a) = self.observe(Slot::A, &mut buf_a) else {
            return Loaded::IoError;
        };
        let Ok(seen_b) = self.observe(Slot::B, &mut buf_b) else {
            return Loaded::IoError;
        };
        match (seen_a, seen_b) {
            (Seen::Valid(a), Seen::Valid(b)) => {
                if b.previous == a.digest {
                    self.adopt(Slot::B, b, &buf_b)
                } else if a.previous == b.digest {
                    self.adopt(Slot::A, a, &buf_a)
                } else {
                    // Neither chains onto the other: a half-reverted pair.
                    // Keep the higher revision as the predecessor of the
                    // repair record and overwrite the other slot (tie: B).
                    let (slot, meta, target) = if b.revision > a.revision {
                        (Slot::B, b, Slot::A)
                    } else {
                        (Slot::A, a, Slot::B)
                    };
                    self.head = Some(Head {
                        slot,
                        digest: meta.digest,
                        present: meta.present,
                    });
                    self.repair_target = Some(target);
                    Loaded::RollbackDetected
                }
            }
            (Seen::Valid(a), _) => self.adopt(Slot::A, a, &buf_a),
            (_, Seen::Valid(b)) => self.adopt(Slot::B, b, &buf_b),
            (Seen::Absent, Seen::Absent) => Loaded::Empty,
            // File(s) present, none valid: visible, not an empty store.
            _ => {
                self.repair_target = Some(Slot::A);
                Loaded::Corrupt
            }
        }
    }

    fn store(&mut self, record: &RecordView<'_>) -> Result<Stored, PersistError> {
        // Plain: RAM only. The seed is not read and no I/O happens.
        if matches!(record.secret, SecretView::Plain(_)) {
            return Ok(Stored::VolatileOnly);
        }
        self.commit(true, |slot, previous, out| {
            record::encode_present(slot, record, previous, out)
        })
    }

    fn tombstone(&mut self, next_revision: u64) -> Result<Stored, PersistError> {
        // A RAM-only key has nothing persisted to delete.
        if !self.head.is_some_and(|head| head.present) {
            return Ok(Stored::VolatileOnly);
        }
        self.commit(false, |slot, previous, out| {
            record::encode_tombstone(slot, next_revision, previous, out)
        })
    }
}
