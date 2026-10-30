//! KSSH K4b host tests (feature `ssh-dev-persist`): the file persister and its
//! record format, tested at the leaf level through the same ten handlers as the
//! K5 suite, over an in-memory `SshBlobIo`. The service-level wiring and the
//! VFS-backed `VfsBlobIo` are not host-testable (K4b CP1 section 6).

use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::persist::blob_io::{Slot, RECORD_MAX};
use crate::ssh::persist::FilePersister;
use mem_io::MemBlobIo;

mod boundary;
mod devroot;
mod leaf;
mod listing;
mod load;
mod mem_io;
mod plain;
mod record;
mod secrecy;
mod stack;
mod torn;

pub(super) type FileLeaf = SshLeaf<FilePersister<MemBlobIo>>;

/// The key name every persist test imports under.
pub(super) const NAME: &[u8] = b"wk";

/// A leaf that has not loaded yet (production shape: lazy load) over `io`.
pub(super) fn leaf_over(io: MemBlobIo) -> FileLeaf {
    SshLeaf::new(FilePersister::new(io))
}

pub(super) fn fresh() -> FileLeaf {
    leaf_over(MemBlobIo::new())
}

/// A new leaf over the same files: a reboot.
pub(super) fn reboot(leaf: &FileLeaf) -> FileLeaf {
    leaf_over(leaf.persist.io.reopen())
}

/// Passphrase of the crafted wrapped key.
pub(super) const PASS: &[u8] = b"persist-pass";

/// Crafted wrapped container (RFC 8032 TEST 1 seed, cheap KDF, rounds 1).
pub(super) fn cheap_blob() -> Vec<u8> {
    Craft::wrapped(&RFC8032_SEED_1, PASS, &[0x11; 16], 1).bytes()
}

pub(super) fn cheap_public() -> [u8; 32] {
    public_of(&RFC8032_SEED_1)
}

pub(super) fn import_cheap(
    leaf: &mut FileLeaf,
    admin: &Actor,
) -> Result<types::kms::SshKeyStatusPayload, KmsErrorCode> {
    import_blob(leaf, admin, NAME, &cheap_blob())
}

/// Import then delete: leaves a key record at one slot and a tombstone at the
/// other, so both files exist.
pub(super) fn leaf_with_two_records(admin: &Actor) -> FileLeaf {
    let mut leaf = fresh();
    import_cheap(&mut leaf, admin).unwrap();
    delete_key(&mut leaf, admin, NAME, cheap_public()).unwrap();
    leaf
}

pub(super) fn leaf_readiness(
    leaf: &mut FileLeaf,
    actor: &Actor,
) -> types::kms::SshKeyStatusPayload {
    status(leaf, actor, b"").unwrap()
}

/// The raw bytes of a slot, which must exist.
pub(super) fn bytes_of(leaf: &FileLeaf, slot: Slot) -> Vec<u8> {
    leaf.persist.io.file(slot).expect("slot written").clone()
}
