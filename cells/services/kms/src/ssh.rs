//! KMS custody of one OpenSSH ed25519 user identity (opcodes 15-24), T0.
//!
//! Compiled only with the opt-in `ssh-dev-custody` feature. The store is
//! volatile (`DevPostureVolatile`), holds exactly one identity (LR-1 = Y), and
//! never leaves KMS: the only outputs are public status facts and signatures
//! over a reconstructed RFC 4252 userauth input.
//!
//! State lives in [`SshLeaf`]; the ten handlers are in `dispatch/ssh.rs`.
//! Persistence is a seam ([`SshPersist`]): T0 plugs [`NoPersist`], and the
//! seam already enforces persist-before-swap so a later persister adds no
//! custody logic here.

extern crate alloc;

use core::fmt;

use blake2::{Blake2s256, Digest};
use types::kms::{KmsCapabilityReadiness, KmsCustodyPosture, KmsErrorCode, KMS_SSH_SECRET_MAX};
use zeroize::{Zeroize, Zeroizing};

pub(crate) mod floor;
pub(crate) mod import;
pub(crate) mod limits;
pub(crate) mod parser;
#[cfg(feature = "ssh-dev-persist")]
pub(crate) mod persist;
pub(crate) mod sign;
pub(crate) mod store;
pub(crate) mod unlock;
pub(crate) mod unwrap;

use floor::FloorTable;
use import::{Arena, ImportSlot};
use store::{RecordView, StoredIdentity};
use unlock::UnlockSlot;

/// Attested `(cell_id, generation)`: the key of replay floors and transfer
/// slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Principal {
    pub cell_id: u64,
    pub generation: u64,
}

/// Whether the persister wrote the record durably.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stored {
    /// Produced by a durable persister (K4b) and the test fakes.
    #[allow(dead_code)]
    Written,
    VolatileOnly,
}

/// A persister could not store or tombstone; the service answers
/// `PersistFailed` and changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PersistError;

/// What a persister found when asked to load. Variants other than `Empty` are
/// produced only by a durable persister (K4b) and by the test fakes.
#[allow(dead_code, clippy::large_enum_variant)] // no heap in this cell: the identity is inline
pub(crate) enum Loaded {
    Empty,
    Found {
        revision: u64,
        identity: StoredIdentity,
    },
    Corrupt,
    RollbackDetected,
    IoError,
}

/// Persistence seam (K5 CP1 section 5.3). `store` and `tombstone` are called
/// before the RAM state changes; a failure leaves everything unchanged.
pub(crate) trait SshPersist {
    fn load(&mut self) -> Loaded;
    fn store(&mut self, record: &RecordView<'_>) -> Result<Stored, PersistError>;
    fn tombstone(&mut self, next_revision: u64) -> Result<Stored, PersistError>;
}

/// T0: nothing is persisted.
#[cfg_attr(all(feature = "ssh-dev-persist", not(test)), allow(dead_code))]
pub(crate) struct NoPersist;

impl SshPersist for NoPersist {
    fn load(&mut self) -> Loaded {
        Loaded::Empty
    }

    fn store(&mut self, _record: &RecordView<'_>) -> Result<Stored, PersistError> {
        Ok(Stored::VolatileOnly)
    }

    fn tombstone(&mut self, _next_revision: u64) -> Result<Stored, PersistError> {
        Ok(Stored::VolatileOnly)
    }
}

/// The persister of the running service: `NoPersist` (T0), or the VFS-backed
/// `FilePersister` in a T1 (`ssh-dev-persist`) bare-metal build. Host tests
/// always get `NoPersist` so the K5 suite keeps asserting volatile behaviour;
/// the file persister is tested at the leaf level with in-memory fakes.
#[cfg(not(all(feature = "ssh-dev-persist", not(test))))]
pub(crate) type Persister = NoPersist;
#[cfg(all(feature = "ssh-dev-persist", not(test)))]
pub(crate) type Persister = persist::FilePersister<persist::blob_io::VfsBlobIo>;

/// The leaf the service starts with.
#[cfg(not(all(feature = "ssh-dev-persist", not(test))))]
pub(crate) fn new_leaf() -> SshLeaf<Persister> {
    SshLeaf::serving(NoPersist)
}

/// T1: the store is loaded lazily on the first admitted SSH opcode, never at
/// boot, so a missing VFS cannot block startup.
#[cfg(all(feature = "ssh-dev-persist", not(test)))]
pub(crate) fn new_leaf() -> SshLeaf<Persister> {
    SshLeaf::new(persist::FilePersister::new(
        persist::blob_io::VfsBlobIo::new(),
    ))
}

/// Leaf serving state. T0 uses `Serving` (and `Unavailable` when the handle
/// counter is exhausted); the other variants exist so a durable persister adds
/// no custody logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeafState {
    Serving,
    Unavailable,
    NotLoaded,
    /// Readable but not serving; only import (15-18) may repair it. The
    /// readiness is `ProviderError` or `PolicyMismatch`.
    Repairable(KmsCapabilityReadiness),
}

/// Handle spaces: a handle for one kind never validates for the other.
pub(crate) const HANDLE_IMPORT: u8 = 1;
pub(crate) const HANDLE_UNLOCK: u8 = 2;

pub(crate) struct SshLeaf<P: SshPersist> {
    pub(crate) state: LeafState,
    pub(crate) identity: Option<StoredIdentity>,
    /// Reported as `store_revision`; bumps on import and delete.
    pub(crate) revision: u64,
    pub(crate) posture: KmsCustodyPosture,
    pub(crate) import: ImportSlot,
    pub(crate) arena: Arena,
    pub(crate) unlock: UnlockSlot,
    /// Staged passphrase bytes; wiped on every exit of the unlock slot.
    pub(crate) secret: Zeroizing<[u8; KMS_SSH_SECRET_MAX]>,
    pub(crate) floors: FloorTable,
    pub(crate) handle_counter: u64,
    /// Consecutive `SshUnlockFailed` results for the one key.
    pub(crate) failures: u8,
    pub(crate) persist: P,
}

impl<P: SshPersist> SshLeaf<P> {
    /// A leaf that asks its persister on first use.
    #[allow(dead_code)]
    pub(crate) fn new(persist: P) -> Self {
        Self::with_state(persist, LeafState::NotLoaded)
    }

    /// A leaf that is serving an empty store from the start (T0).
    #[cfg_attr(all(feature = "ssh-dev-persist", not(test)), allow(dead_code))]
    pub(crate) fn serving(persist: P) -> Self {
        Self::with_state(persist, LeafState::Serving)
    }

    fn with_state(persist: P, state: LeafState) -> Self {
        Self {
            state,
            identity: None,
            revision: 0,
            posture: KmsCustodyPosture::DevPostureVolatile,
            import: ImportSlot::Idle,
            arena: Arena::new(),
            unlock: UnlockSlot::Empty,
            secret: Zeroizing::new([0; KMS_SSH_SECRET_MAX]),
            floors: FloorTable::new(),
            handle_counter: 0,
            failures: 0,
            persist,
        }
    }

    /// Run the one lazy load while the state is `NotLoaded`.
    pub(crate) fn ensure_loaded(&mut self) {
        if self.state != LeafState::NotLoaded {
            return;
        }
        match self.persist.load() {
            Loaded::Empty => self.state = LeafState::Serving,
            Loaded::Found { revision, identity } => {
                self.identity = Some(identity);
                self.revision = revision;
                self.posture = KmsCustodyPosture::DevPosturePersistent;
                self.state = LeafState::Serving;
            }
            Loaded::Corrupt => {
                self.state = LeafState::Repairable(KmsCapabilityReadiness::ProviderError)
            }
            Loaded::RollbackDetected => {
                self.state = LeafState::Repairable(KmsCapabilityReadiness::PolicyMismatch)
            }
            // Stay `NotLoaded`: the opcode answers unavailable, the next
            // opcode retries.
            Loaded::IoError => {}
        }
    }

    /// Next opaque handle. Domain-separated BLAKE2s over the counter and the
    /// bound principal and task; nonzero. Handles are identifiers, not secrets:
    /// the checks on every use enforce access. The counter never wraps: at the
    /// limit the leaf becomes unavailable.
    pub(crate) fn next_handle(
        &mut self,
        kind: u8,
        principal: Principal,
        tid: usize,
    ) -> Result<u64, KmsErrorCode> {
        let Some(counter) = self.handle_counter.checked_add(1) else {
            self.state = LeafState::Unavailable;
            return Err(KmsErrorCode::SshKeyUnavailable);
        };
        self.handle_counter = counter;
        let mut hash = Blake2s256::new();
        hash.update(b"cellos-kssh-handle-v1");
        hash.update([kind]);
        hash.update(counter.to_le_bytes());
        hash.update(principal.cell_id.to_le_bytes());
        hash.update(principal.generation.to_le_bytes());
        hash.update((tid as u64).to_le_bytes());
        let digest = hash.finalize();
        let mut raw = [0u8; 8];
        raw.copy_from_slice(digest.get(..8).unwrap_or(&[0; 8]));
        Ok(u64::from_le_bytes(raw) | 0x8000_0000_0000_0001)
    }

    /// Destroy any pending import and wipe the staging arena.
    pub(crate) fn wipe_import(&mut self) {
        self.import = ImportSlot::Idle;
        self.arena.wipe();
    }

    /// Destroy any unlock slot and wipe the staged secret.
    pub(crate) fn wipe_unlock(&mut self) {
        self.unlock = UnlockSlot::Empty;
        self.secret.zeroize();
    }
}

impl<P: SshPersist> fmt::Debug for SshLeaf<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshLeaf")
            .field("state", &self.state)
            .field("key_count", &usize::from(self.identity.is_some()))
            .field("revision", &self.revision)
            .field("secret", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// Best-effort stack scrub after every unlock and sign: a zero-filled region
/// written through `black_box` so it is not trivially elided. The crate
/// forbids `unsafe`, so this is the only technique available; it is not a
/// proof that no remnant survives.
#[inline(never)]
pub(crate) fn scrub_stack() {
    let mut region = [0u8; limits::SCRUB_LEN];
    core::hint::black_box(&mut region);
    region.zeroize();
    core::hint::black_box(&region);
}

/// Shared transfer bookkeeping for import and unlock slots.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Transfer {
    pub handle: u64,
    pub principal: Principal,
    pub tid: usize,
    pub total_len: u32,
    pub chunk_count: u32,
    pub next_index: u32,
    pub received: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Violation {
    Handle,
    Order,
}

impl Violation {
    pub(crate) fn code(self) -> KmsErrorCode {
        match self {
            Violation::Handle => KmsErrorCode::SshHandleInvalid,
            Violation::Order => KmsErrorCode::SshOrderInvalid,
        }
    }
}

impl Transfer {
    pub(crate) fn new(handle: u64, principal: Principal, tid: usize, total_len: u32) -> Self {
        Self {
            handle,
            principal,
            tid,
            total_len,
            chunk_count: types::kms::ssh_chunk_count(total_len),
            next_index: 0,
            received: 0,
        }
    }

    pub(crate) fn owned_by(&self, handle: u64, principal: Principal, tid: usize) -> bool {
        self.handle == handle && self.principal == principal && self.tid == tid
    }

    /// Validate one chunk against the canonical sequence; returns the offset
    /// at which its data goes.
    pub(crate) fn validate_chunk(
        &self,
        chunk: &types::kms::SshChunkRequestPayload,
        principal: Principal,
        tid: usize,
    ) -> Result<usize, Violation> {
        if !self.owned_by(chunk.handle, principal, tid) {
            return Err(Violation::Handle);
        }
        if chunk.chunk_index != self.next_index {
            return Err(Violation::Order);
        }
        let expected =
            types::kms::ssh_chunk_len(self.total_len, chunk.chunk_index).ok_or(Violation::Order)?;
        if chunk.chunk_len != expected {
            return Err(Violation::Order);
        }
        self.received
            .checked_add(u32::from(chunk.chunk_len))
            .filter(|end| *end <= self.total_len)
            .ok_or(Violation::Order)?;
        Ok(self.received as usize)
    }

    pub(crate) fn advance(&mut self, chunk_len: u16) {
        self.next_index = self.next_index.saturating_add(1);
        self.received = self.received.saturating_add(u32::from(chunk_len));
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.received == self.total_len
    }
}
