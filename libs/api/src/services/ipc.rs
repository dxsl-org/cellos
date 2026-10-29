//! Typed IPC message enums for ViCell services.
//!
//! Both kernel and Cell crates link to `libs/api`, so these types are shared
//! across the IPC boundary without any unsafe casting.
//!
//! ## Wire format
//! `postcard::to_slice` serializes into a caller-provided `[u8; IPC_BUF_SIZE]`
//! stack buffer.  The receiver calls `postcard::from_bytes` to deserialize.
//! Discriminants are 1-byte varints (< 127 variants); slice lengths are varint-
//! prefixed.  Total overhead per message is minimal compared to the ecall trap.
//!
//! ## Lifetime contract
//! Types that borrow (`&'a str`, `&'a [u8]`) point into the caller's buffer.
//! The decoded value MUST be consumed before the buffer is reused.

use serde::{Deserialize, Serialize};

use crate::cell_session::ObjectRefV1;

/// IPC payload buffer size.  Must be large enough for the largest serialized
/// message in the system — currently a VFS Write with ~900-byte content
/// (~916 bytes encoded).  4 KiB gives comfortable headroom.
pub const IPC_BUF_SIZE: usize = 4096;
/// Maximum inline TCP data that leaves conservative postcard framing headroom.
pub const NET_TCP_INLINE_DATA_MAX: usize = IPC_BUF_SIZE - 256;

/// Largest single grant chunk a `ReadHandleGrant` / `WriteHandleGrant` may move.
///
/// Both sides must agree: the caller (hypervisor cell) batches one guest block
/// request into chunks of at most this size, and the VFS rejects anything
/// larger. Grants are backed by *contiguous* frames, so the cap trades round
/// trips against the allocator's ability to find a contiguous run — 16 pages,
/// with callers free to fall back to smaller chunks when allocation fails.
pub const VFS_GRANT_CHUNK: usize = 64 * 1024;

// ── VFS service ───────────────────────────────────────────────────────────────

/// Requests sent to the VFS service (`/bin/vfs`).
#[derive(Debug, Serialize, Deserialize)]
pub enum VfsRequest<'a> {
    /// Read file content from a VFS-managed path.
    GetFile(&'a str),
    /// List directory entries as a newline-separated string.
    ListDir(&'a str),
    /// Stat a path — returns size + is_dir flag.
    Stat(&'a str),
    /// Write (create/overwrite) a file.
    Write { path: &'a str, content: &'a [u8] },
    /// Append bytes to an existing file (or create it).
    Append { path: &'a str, content: &'a [u8] },
    /// Create a directory.
    Mkdir(&'a str),
    /// Remove an empty directory.
    Rmdir(&'a str),
    /// Delete a file.
    Unlink(&'a str),
    /// Remove a directory tree recursively.
    RmdirRecursive(&'a str),
    /// Start a non-blocking file read.  Returns `PendingHandle(id)` immediately;
    /// call `Poll { handle: id }` to retrieve data when ready.
    ReadAsync { path: &'a str },
    /// Poll a pending read for completion.
    Poll { handle: u32 },
    /// Zero-copy large read: VFS reads `size` bytes at `offset` from the file
    /// identified by `cap` directly into the caller's pre-allocated grant buffer.
    /// The caller allocates the grant, GrantShare's it RW to VFS, and then waits
    /// for `GrantDone`. VFS copies at most `min(size, grant_len, 4096, data_len -
    /// offset)` bytes and replies only after the copy completes.
    ReadGrant {
        cap: u64,
        offset: u64,
        size: usize,
        grant: usize,
    },
    /// Zero-copy large write: VFS reads `bytes` bytes from the caller's grant
    /// buffer and writes them at `offset` through a VFS-issued file-handle ID in
    /// `cap`. GrantDone is sent only after the write commits (write-through on
    /// FAT32) — F14 invariant.
    WriteGrant {
        cap: u64,
        offset: u64,
        grant: usize,
        bytes: usize,
    },
    /// Zero-copy full-file read by PATH into the caller's grant (G2 loader redesign).
    /// Unlike `ReadGrant` (cap + 4 KB page at a time), this resolves `path` through
    /// the VFS mount table (so it reaches the `/bin` cell-store overlay) and copies
    /// the ENTIRE file into the caller's pre-shared grant in one shot, replying
    /// `GrantDone { bytes }`. `grant` must be owned by the caller and GrantShare'd
    /// RW to VFS. Short grants are valid: VFS copies `min(file_len, max, grant_len)`
    /// bytes, so a file that grew after the caller's Stat cannot overflow the grant.
    /// Used to read a cell ELF for `sys_spawn_from_elf`.
    ReadFileGrant {
        path: &'a str,
        grant: usize,
        max: usize,
    },

    // ── Directory capabilities ────────────────────────────────────────────────
    //
    // Appended at the END and never reordered. Postcard encodes the variant
    // index as a varint, so inserting anywhere above would renumber every
    // variant after the insertion point: an old sender and a new receiver would
    // then agree on the bytes and disagree on the meaning, which is a silently
    // wrong operation rather than a decode error. This system has shipped that
    // bug once already.
    //
    // Each of these names a directory the service issued to this caller and a
    // single entry inside it. The name cannot express a path outside the
    // directory, so there is no path for the service to authorize — see
    // `docs/specs/09c-vfs-directory-capabilities-adr.md`.
    /// Acquire a handle to an absolute directory path.
    ///
    /// The one remaining operation that names a path, and the bootstrap for
    /// every other handle: a cell acquires the directories it needs and then
    /// sends `SealPaths`, after which no path string from it is served.
    OpenRootDir { path: &'a str },
    /// Derive a handle to `name` inside `dir`, narrower than `dir` by
    /// construction. Revoking `dir` revokes it.
    OpenDir {
        dir: crate::dir_handles::ViDirHandle,
        name: &'a str,
    },
    /// Read the whole of `name` inside `dir`.
    ReadAt {
        dir: crate::dir_handles::ViDirHandle,
        name: &'a str,
    },
    /// Create or overwrite `name` inside `dir`.
    WriteAt {
        dir: crate::dir_handles::ViDirHandle,
        name: &'a str,
        content: &'a [u8],
    },
    /// Size and kind of `name` inside `dir`.
    StatAt {
        dir: crate::dir_handles::ViDirHandle,
        name: &'a str,
    },
    /// Entries of `dir` itself.
    ///
    /// Takes no name: listing a subdirectory means holding a handle to it, which
    /// is what `OpenDir` is for. A name here would be a second resolution path
    /// to keep in step with the first.
    ListAt {
        dir: crate::dir_handles::ViDirHandle,
    },
    /// Delete `name` inside `dir`.
    UnlinkAt {
        dir: crate::dir_handles::ViDirHandle,
        name: &'a str,
    },
    /// Give up `dir`, and with it every handle derived from `dir`.
    ///
    /// Revocation is transitive on purpose: a derived handle that outlived the
    /// handle it came from would let a cell keep access forever by deriving a
    /// subdirectory and dropping the original.
    CloseDir {
        dir: crate::dir_handles::ViDirHandle,
    },
    /// Give up the ability to name a path, permanently, for this cell.
    ///
    /// Every path-addressed request from the sender is refused from here on. The
    /// transition is one-way and survives for the life of the cell: an operation
    /// that could undo it would make the guarantee advisory.
    SealPaths,
    /// Open the file `name` inside `dir` and receive a service-local file
    /// handle for bounded inline reads.
    OpenFileAt {
        dir: crate::dir_handles::ViDirHandle,
        name: &'a str,
    },
    /// Read at most `max` bytes from `file`, starting at `offset`.
    ReadFileHandle {
        file: crate::vfs_file_handles::ViVfsFileHandle,
        offset: u64,
        max: u32,
    },
    /// Give up `file`.
    CloseFile {
        file: crate::vfs_file_handles::ViVfsFileHandle,
    },
    /// Bounded zero-copy read into a grant buffer using a VFS handle.
    ReadHandleGrant {
        file: crate::vfs_file_handles::ViVfsFileHandle,
        offset: u64,
        size: usize,
        grant: usize,
    },
    /// Bounded zero-copy write from a grant buffer using a VFS handle.
    WriteHandleGrant {
        file: crate::vfs_file_handles::ViVfsFileHandle,
        offset: u64,
        grant: usize,
        bytes: usize,
    },
    /// Flush dirty pages for a file handle to the block device.
    SyncHandle {
        file: crate::vfs_file_handles::ViVfsFileHandle,
    },
    /// Atomic rename of a file or directory from `old` to `new`.
    Rename { old: &'a str, new: &'a str },
    /// Owner-authenticated prepared-cwd protocol; no filesystem mutation rights.
    PreparedChildV1(&'a [u8]),
    /// One page of the entries of `dir` itself, starting at `cursor`.
    ///
    /// `cursor` 0 starts a listing; every later page passes the `next` value of
    /// the previous [`VfsResponse::DirPage`] unchanged (it is opaque). Answered
    /// with `DirPage`, whose `entries` are packed per [`vfs_dir_page`]. Unlike
    /// `ListAt`, a listing that does not fit one reply is never cut short
    /// silently: only the page with [`vfs_dir_page::COMPLETE`] ends it.
    ListAtPage {
        dir: crate::dir_handles::ViDirHandle,
        cursor: u32,
    },
    /// Current size of `file`, and whether a content or binding mutation reached
    /// its path after it was opened. Answered with `FileState`.
    FileHandleState {
        file: crate::vfs_file_handles::ViVfsFileHandle,
    },

    // ── D03 conditional save (Phase 08). Appended; never reorder. ─────────────
    /// Observe `path` for a later conditional publication. Answered with
    /// `Observed`, or `Err(6)` on a backend without conditional save.
    Observe { path: &'a str },
    /// Open a staging transaction that can only publish at `dst`. Answered with
    /// `SaveId`. Nothing visible at `dst` changes until `SaveCommit`.
    SaveBegin { dst: &'a str },
    /// Append staged bytes; `offset` must equal the bytes staged so far and
    /// `bytes` is at most [`VFS_SAVE_CHUNK`].
    SaveChunk { id: u32, offset: u64, bytes: &'a [u8] },
    /// Seal the staged bytes: the service reads them back and requires exactly
    /// `len` bytes with this SHA-256. No further chunks are accepted.
    SaveSeal { id: u32, len: u64, sha256: [u8; 32] },
    /// Publish the sealed bytes at `dst` only if it still matches `expect`.
    /// Answered with `SaveOutcome`; a repeated commit or query answers the
    /// recorded outcome and never runs the publication again.
    SaveCommit { id: u32, expect: SaveExpect },
    /// Outcome of this transaction's commit, if one was attempted.
    SaveQuery { id: u32 },
    /// Forget the transaction, removing any unpublished staged bytes.
    SaveAbort { id: u32 },
    /// Phase 09 (abi-vfs-telemetry §4, read-only part): look up the configured SSH
    /// host identifier and port in the protected host-trust store and compare the
    /// complete public key. Answered with [`VfsResponse::Trust`]. Needs the caller's
    /// attested outbound-connect right; never reveals store contents.
    TrustLookup { host: &'a str, port: u16, algorithm: &'a str, key: &'a [u8] },
    /// Phase 09 S9-4c first trust: for a host and port that [`Self::TrustLookup`] reports
    /// Unknown, the service registers a 60-second challenge with the input owner bound to
    /// the caller, the exact key and the store's current state, and answers
    /// [`VfsResponse::TrustChallenged`] (or [`VfsResponse::Trust`] with the refusal). The
    /// caller then runs `BeginTrustConfirmation` with `confirmation` on `session` at
    /// `epoch`, its own terminal session. Needs the attested outbound-connect right.
    TrustChallenge { host: &'a str, port: u16, algorithm: &'a str, key: &'a [u8], session: ObjectRefV1, epoch: u64 },
    /// Phase 09 S9-4c: complete the caller's own `challenge`. Answered with
    /// [`VfsResponse::Trust`]: `PENDING` while undecided; after an accepted confirmation
    /// the store is updated and the answer is `KNOWN_EXACT` (or `MISMATCH` when another
    /// writer stored a different key first); `REFUSED`, `EXPIRED`, `DENIED` or
    /// `UNAVAILABLE` otherwise. Any final answer ends the challenge.
    TrustCommit { challenge: ObjectRefV1 },
}

/// Largest `SaveChunk` payload.
pub const VFS_SAVE_CHUNK: usize = 512;
/// Largest sealed save.
pub const VFS_SAVE_MAX: u64 = 4 * 1024 * 1024;

/// The destination state a conditional save was confirmed against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveExpect {
    Absent,
    /// Exact file identity: `inode` is never reused on the volume and
    /// `generation` advances on every mutation, even a byte-identical one.
    Present { inode: u64, generation: u64 },
}

/// `Observe` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Observation {
    Absent,
    Present { inode: u64, generation: u64, size: u64, regular: bool },
}

impl Observation {
    /// The expectation that binds a save to exactly this observation; `None`
    /// for a non-regular object, which a save never replaces.
    pub fn expect(self) -> Option<SaveExpect> {
        match self {
            Self::Absent => Some(SaveExpect::Absent),
            Self::Present { inode, generation, regular: true, .. } =>
                Some(SaveExpect::Present { inode, generation }),
            Self::Present { .. } => None,
        }
    }
}

/// `VfsResponse::Trust` values (abi-vfs-telemetry §4 lookup outcomes).
pub mod ssh_trust {
    /// The store holds this host and port with exactly this algorithm and key.
    pub const KNOWN_EXACT: u8 = 0;
    /// The store holds nothing for this host and port.
    pub const UNKNOWN: u8 = 1;
    /// The store holds this host and port with a different key.
    pub const MISMATCH: u8 = 2;
    /// The caller may not consult the store.
    pub const DENIED: u8 = 3;
    /// The store cannot be read or is malformed.
    pub const UNAVAILABLE: u8 = 4;
    /// `TrustCommit`: the user refused (or the confirmation was cancelled, or its
    /// owner-attested bindings did not match the challenge). Nothing was stored.
    pub const REFUSED: u8 = 5;
    /// `TrustCommit`: the challenge's 60 seconds passed. Nothing was stored.
    pub const EXPIRED: u8 = 6;
    /// `TrustCommit`: not decided yet; ask again.
    pub const PENDING: u8 = 7;
}

/// Why a commit proved nothing was published.
pub mod save_refusal {
    /// The staged object changed after sealing.
    pub const STAGE_CHANGED: u8 = 1;
    /// The transaction was not sealed.
    pub const NOT_SEALED: u8 = 2;
    /// Quota does not cover the new size.
    pub const QUOTA: u8 = 3;
    /// The backend refused before any mutation (bad destination, no space).
    pub const REFUSED: u8 = 4;
}

/// Result of `SaveCommit` / `SaveQuery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveOutcome {
    /// Published and synced through the backend's barriers. This is not a
    /// power-loss guarantee.
    Committed { inode: u64, generation: u64 },
    /// `dst` no longer matched the expectation; nothing changed.
    Conflict,
    /// Proven not published; `dst` is unchanged. Reason per [`save_refusal`].
    NotPublished(u8),
    /// The publication may or may not have reached storage.
    Indeterminate,
    /// Published, and the backend's durability protocol proved the new bytes survive
    /// power loss (before this reply: old or new whole, never torn). No backend
    /// returns it yet: CellosFS reports durability unavailable until the separate
    /// crash-safety plan (`.agents/261025-cellosfs-crash-safety/plan.md`) lands.
    CommittedDurable { inode: u64, generation: u64 },
}

impl SaveOutcome {
    /// The published object for either successful outcome.
    pub fn published(self) -> Option<(u64, u64)> {
        match self {
            Self::Committed { inode, generation } | Self::CommittedDurable { inode, generation } => {
                Some((inode, generation))
            }
            Self::Conflict | Self::NotPublished(_) | Self::Indeterminate => None,
        }
    }
}

impl VfsRequest<'_> {
    /// Whether this request names its target with a path string.
    ///
    /// A sealed cell is refused every request for which this is true. Matched
    /// exhaustively on purpose: a new variant does not compile until someone has
    /// decided which side of the boundary it falls on, and the answer defaults
    /// to nothing.
    pub fn is_path_addressed(&self) -> bool {
        match self {
            Self::GetFile(_)
            | Self::ListDir(_)
            | Self::Stat(_)
            | Self::Write { .. }
            | Self::Append { .. }
            | Self::Mkdir(_)
            | Self::Rmdir(_)
            | Self::Unlink(_)
            | Self::RmdirRecursive(_)
            | Self::ReadAsync { .. }
            | Self::ReadFileGrant { .. }
            | Self::OpenRootDir { .. }
            | Self::Rename { .. } => true,
            // `Poll`, `ReadGrant` and `WriteGrant` carry a handle the service
            // issued, not a path. They stay reachable so a sealed cell can drain
            // work it started before sealing; the path recorded against the
            // handle is still re-authorized on every use.
            Self::Poll { .. }
            | Self::ReadGrant { .. }
            | Self::WriteGrant { .. }
            | Self::OpenDir { .. }
            | Self::ReadAt { .. }
            | Self::WriteAt { .. }
            | Self::StatAt { .. }
            | Self::ListAt { .. }
            | Self::UnlinkAt { .. }
            | Self::CloseDir { .. }
            | Self::SealPaths
            | Self::OpenFileAt { .. }
            | Self::ReadFileHandle { .. }
            | Self::CloseFile { .. }
            | Self::ReadHandleGrant { .. }
            | Self::WriteHandleGrant { .. }
            | Self::SyncHandle { .. }
            | Self::PreparedChildV1(_)
            | Self::ListAtPage { .. }
            | Self::FileHandleState { .. } => false,
            // A save names its destination once, at `SaveBegin`; the rest carry
            // the service-issued transaction id.
            Self::Observe { .. } | Self::SaveBegin { .. } => true,
            Self::SaveChunk { .. }
            | Self::SaveSeal { .. }
            | Self::SaveCommit { .. }
            | Self::SaveQuery { .. }
            | Self::SaveAbort { .. }
            // Names a host, never a path; the store location is the service's own.
            | Self::TrustLookup { .. }
            | Self::TrustChallenge { .. }
            | Self::TrustCommit { .. } => false,
        }
    }

    /// Whether this request requires the caller to hold VfsMutate authority.
    ///
    /// Matched exhaustively so any future variant must be classified.
    pub fn requires_mutation_authority(&self) -> bool {
        match self {
            Self::Write { .. }
            | Self::Append { .. }
            | Self::Mkdir(_)
            | Self::Rmdir(_)
            | Self::Unlink(_)
            | Self::RmdirRecursive(_)
            | Self::WriteGrant { .. }
            | Self::WriteAt { .. }
            | Self::UnlinkAt { .. }
            | Self::WriteHandleGrant { .. }
            | Self::SyncHandle { .. }
            | Self::Rename { .. }
            | Self::SaveBegin { .. }
            | Self::SaveChunk { .. }
            | Self::SaveSeal { .. }
            | Self::SaveCommit { .. }
            | Self::SaveAbort { .. } => true,

            Self::GetFile(_)
            | Self::ListDir(_)
            | Self::Stat(_)
            | Self::ReadAsync { .. }
            | Self::Poll { .. }
            | Self::ReadGrant { .. }
            | Self::ReadFileGrant { .. }
            | Self::OpenRootDir { .. }
            | Self::OpenDir { .. }
            | Self::ReadAt { .. }
            | Self::StatAt { .. }
            | Self::ListAt { .. }
            | Self::CloseDir { .. }
            | Self::SealPaths
            | Self::OpenFileAt { .. }
            | Self::ReadFileHandle { .. }
            | Self::CloseFile { .. }
            | Self::ReadHandleGrant { .. }
            | Self::PreparedChildV1(_)
            | Self::ListAtPage { .. }
            | Self::FileHandleState { .. }
            | Self::Observe { .. }
            | Self::SaveQuery { .. }
            | Self::TrustLookup { .. }
            // The store is written on the service's own authority after an
            // owner-attested confirmation, never with the caller's.
            | Self::TrustChallenge { .. }
            | Self::TrustCommit { .. } => false,
        }
    }
}

/// Responses from the VFS service.
#[derive(Debug, Serialize, Deserialize)]
pub enum VfsResponse<'a> {
    /// File or directory listing bytes (copied into response buffer).
    Data(&'a [u8]),
    /// Zero-copy file access: raw pointer + length in the shared address space.
    /// Used by binary loaders that need to map the ELF directly from VFS memory.
    DataPtr { ptr: u64, len: u64 },
    /// Stat result: (file_size, is_dir).
    Stat { size: u64, is_dir: bool },
    /// Successful write, mkdir, unlink, etc.
    Ok,
    /// Error — opaque error code (`types::ViError` discriminant).
    Err(u8),
    /// Async read accepted; poll this handle for completion.
    PendingHandle(u32),
    /// Read still in progress — call Poll again after yielding.
    Pending,
    /// Zero-copy I/O complete: `bytes` is the number of bytes transferred.
    GrantDone { bytes: usize },
    /// A directory handle the service has issued to the caller.
    ///
    /// Appended at the END for the same reason as the request variants: an
    /// insertion above would renumber the rest and turn an old reply into a
    /// different one that still decodes.
    DirHandle(crate::dir_handles::ViDirHandle),
    /// A file handle the service has issued to the caller.
    FileHandle(crate::vfs_file_handles::ViVfsFileHandle),
    /// Complete borrowed V1 prepared-cwd result record.
    PreparedChildV1(&'a [u8]),
    /// One `ListAtPage` reply: whole entries packed per [`vfs_dir_page`],
    /// the cursor for the following page, and [`vfs_dir_page`] flag bits.
    /// On the page that carries `COMPLETE`, `next` is 0 and means nothing.
    DirPage {
        next: u32,
        flags: u8,
        entries: &'a [u8],
    },
    /// `FileHandleState` reply.
    FileState { size: u64, modified: bool },
    /// `Observe` reply.
    Observed(Observation),
    /// `SaveBegin` reply: the transaction id.
    SaveId(u32),
    /// `SaveCommit` / `SaveQuery` reply.
    SaveOutcome(SaveOutcome),
    /// `TrustLookup` reply: one of [`ssh_trust`].
    Trust(u8),
    /// `TrustChallenge` reply: the input-issued confirmation to begin, and the
    /// service-issued challenge to commit.
    TrustChallenged { confirmation: ObjectRefV1, challenge: ObjectRefV1 },
}

/// Packed entry format and flag bits of [`VfsResponse::DirPage`].
///
/// Each entry is `kind:u8, len:u8, name[len]`. `name` is one directory
/// component that passes [`crate::dir_name::validate_dir_component`], so it is
/// valid UTF-8, 1–255 bytes, and can be passed back to `OpenDir`/`OpenFileAt`
/// unchanged. A page holds only whole entries.
pub mod vfs_dir_page {
    use crate::dir_name::{validate_dir_component, DirNameError};

    /// Set on the final page of a listing.
    pub const COMPLETE: u8 = 1 << 0;
    /// A child of the listed directory was created, removed or renamed after the
    /// directory handle was opened, so pages may not form one consistent view.
    /// Sticky for the life of the handle.
    pub const CHANGED: u8 = 1 << 1;
    /// Bytes before the name in one packed entry.
    pub const ENTRY_HEADER_LEN: usize = 2;

    /// What a directory entry is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum EntryKind {
        File,
        Dir,
        /// The root of another mount: listing or reading it leaves this mount.
        Mount,
        /// Neither a regular file nor a directory.
        Other,
    }

    impl EntryKind {
        pub const fn wire(self) -> u8 {
            match self {
                Self::File => 0,
                Self::Dir => 1,
                Self::Mount => 2,
                Self::Other => 3,
            }
        }

        pub const fn from_wire(byte: u8) -> Option<Self> {
            match byte {
                0 => Some(Self::File),
                1 => Some(Self::Dir),
                2 => Some(Self::Mount),
                3 => Some(Self::Other),
                _ => None,
            }
        }
    }

    /// Append one entry to `out` at `pos`.
    ///
    /// Returns the position after the entry, or `Ok(None)` when it does not fit
    /// (nothing is written then).
    ///
    /// # Errors
    /// The name is not a valid directory component.
    pub fn push_entry(
        out: &mut [u8],
        pos: usize,
        kind: EntryKind,
        name: &[u8],
    ) -> Result<Option<usize>, DirNameError> {
        let name = validate_dir_component(name)?.as_bytes();
        let end = pos + ENTRY_HEADER_LEN + name.len();
        if end > out.len() {
            return Ok(None);
        }
        out[pos] = kind.wire();
        // validate_dir_component bounds the name to MAX_DIR_NAME_LEN (255).
        out[pos + 1] = name.len() as u8;
        out[pos + ENTRY_HEADER_LEN..end].copy_from_slice(name);
        Ok(Some(end))
    }

    /// One decoded entry, borrowing the page bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Entry<'a> {
        pub kind: EntryKind,
        pub name: &'a str,
    }

    /// The page bytes do not follow the packed format.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MalformedPage;

    /// Decode the entries of one page in order.
    pub fn entries(page: &[u8]) -> Entries<'_> {
        Entries { rest: page }
    }

    /// Iterator over packed entries; yields one error and then stops.
    pub struct Entries<'a> {
        rest: &'a [u8],
    }

    impl<'a> Iterator for Entries<'a> {
        type Item = Result<Entry<'a>, MalformedPage>;

        fn next(&mut self) -> Option<Self::Item> {
            let rest = self.rest;
            if rest.is_empty() {
                return None;
            }
            self.rest = &[];
            let [kind, len, tail @ ..] = rest else {
                return Some(Err(MalformedPage));
            };
            let len = usize::from(*len);
            let (Some(kind), Some(name)) = (EntryKind::from_wire(*kind), tail.get(..len)) else {
                return Some(Err(MalformedPage));
            };
            let Ok(name) = validate_dir_component(name) else {
                return Some(Err(MalformedPage));
            };
            self.rest = &tail[len..];
            Some(Ok(Entry { kind, name }))
        }
    }
}

// ── Network service ───────────────────────────────────────────────────────────

/// Readiness interests and returned readiness bits. EOF and ERROR are always
/// reported, even when they were not explicitly armed by the caller.
pub mod net_ready {
    pub const ACCEPT: u8 = 1;
    pub const READ: u8 = 2;
    pub const WRITE: u8 = 4;
    pub const EOF: u8 = 8;
    pub const ERROR: u8 = 16;
}

/// Raw readiness opcode emitted by net service towards interest owners: `[0x11, events: u8, cap_id: u32 LE]`.
pub const NET_READY_OPCODE: u8 = 0x11;

/// Raw reactor wakeup opcode emitted within the same cell: `[0x12]`.
pub const REACTOR_WAKE_OPCODE: u8 = 0x12;

/// Fixed length of a raw `NET_READY` frame in bytes.
pub const NET_READY_FRAME_BYTES: usize = 6;

/// Packed TCP readiness record: cap_id (u32 LE) followed by one flags byte.
/// Unlike borrowed structured slices, `&[u8]` deserializes without allocating.
pub const NET_READY_RECORD_BYTES: usize = 5;

/// Attested-owner-scoped TCP interest; cap ids are validated by the net cell.
#[derive(Debug, Clone, Copy)]
pub struct TcpInterest {
    pub cap_id: u32,
    pub mask: u8,
}

/// One level-triggered socket result within a paginated readiness batch.
#[derive(Debug, Clone, Copy)]
pub struct TcpReadyEvent {
    pub cap_id: u32,
    pub ready: u8,
}

impl TcpInterest {
    pub fn decode(record: &[u8]) -> Option<Self> {
        if record.len() != NET_READY_RECORD_BYTES { return None; }
        Some(Self {
            cap_id: u32::from_le_bytes(record[..4].try_into().ok()?),
            mask: record[4],
        })
    }
}

impl TcpReadyEvent {
    pub fn decode(record: &[u8]) -> Option<Self> {
        if record.len() != NET_READY_RECORD_BYTES { return None; }
        Some(Self {
            cap_id: u32::from_le_bytes(record[..4].try_into().ok()?),
            ready: record[4],
        })
    }
}

/// Pack interests into caller-owned storage for `NetRequest::TcpReady`.
pub fn encode_tcp_interests<'a>(
    interests: &[TcpInterest],
    output: &'a mut [u8],
) -> Option<&'a [u8]> {
    let len = interests.len().checked_mul(NET_READY_RECORD_BYTES)?;
    if output.len() < len { return None; }
    for (interest, chunk) in interests.iter().zip(output[..len].chunks_exact_mut(NET_READY_RECORD_BYTES)) {
        chunk[..4].copy_from_slice(&interest.cap_id.to_le_bytes());
        chunk[4] = interest.mask;
    }
    Some(&output[..len])
}

/// Requests sent to the network service (`/bin/net`).
#[derive(Debug, Serialize, Deserialize)]
pub enum NetRequest<'a> {
    TcpConnect {
        addr: [u8; 4],
        port: u16,
    },
    TcpSend {
        cap_id: u32,
        data: &'a [u8],
    },
    TcpRecv {
        cap_id: u32,
        buf_len: u32,
    },
    TcpClose {
        cap_id: u32,
    },
    TcpListen {
        port: u16,
    },
    TcpAccept {
        cap_id: u32,
    },
    UdpCreate,
    UdpSend {
        cap_id: u32,
        addr: [u8; 4],
        port: u16,
        data: &'a [u8],
    },
    UdpRecv {
        cap_id: u32,
        buf_len: u32,
    },
    Resolve {
        hostname: &'a str,
    },
    SocketState {
        cap_id: u32,
    },
    /// Bind a UDP socket to a local port.  Port 0 = auto-assign ephemeral.
    UdpBind {
        cap_id: u32,
        port: u16,
    },
    /// Return the DHCP-assigned local IPv4 address.
    GetLocalIp,
    /// Join an IPv4 multicast group (IGMP); `cap_id` is unused (iface-level).
    MulticastJoin {
        cap_id: u32,
        group: [u8; 4],
    },
    /// Leave a previously joined IPv4 multicast group.
    MulticastLeave {
        cap_id: u32,
        group: [u8; 4],
    },
    /// Forward a raw L2 Ethernet frame to the NIC TX.
    /// `data` is the raw frame bytes (up to 1514 B, no virtio_net_hdr prefix).
    /// The Net Cell calls `sys_net_tx(data)` directly — smoltcp is bypassed.
    L2Send {
        data: &'a [u8],
    },
    /// Poll for one inbound L2 frame from the NIC addressed to `guest_mac`.
    /// The Net Cell splits RX frames by dst MAC; returns `Data(frame)` or `Ok` (empty).
    L2Recv {
        guest_mac: [u8; 6],
    },
    /// Return up to one bounded page of level-triggered readiness. `cursor`
    /// rotates the scan start across `interests`; 0 is a valid initial cursor.
    /// `interests` is a packed sequence of five-byte records
    /// `[cap_id:u32 LE, mask:u8]`; at most 256 records per request.
    /// `wait=true` retains one owner-generation-scoped batch until ready or
    /// a bounded deadline; `wait=false` returns an immediate level snapshot.
    TcpReady {
        interests: &'a [u8],
        cursor: u16,
        wait: bool,
    },
    /// Explicit no-data / EOF TCP read, unlike legacy TcpRecv's empty Data.
    TcpRecvReady { cap_id: u32, buf_len: u32 },
    /// Explicit partial progress / not-ready / EOF TCP send.
    TcpSendReady { cap_id: u32, data: &'a [u8] },
    /// Queue FIN after accepted TCP output; net reclaims at terminal/deadline.
    /// Legacy TcpClose remains an immediate abort and is unchanged.
    TcpCloseGraceful { cap_id: u32 },
}

/// Responses from the network service.
///
/// `Err(code)` codes: `0xFF` generic failure, `0xFE` accept not ready, `0xFD`
/// busy/capacity, [`NET_ERR_DENIED`] (`0xFC`) the caller's Cell lacks the
/// network right the request needs. A deferred `L2Send` reply may forward the
/// NIC driver's own status byte.
#[derive(Debug, Serialize, Deserialize)]
pub enum NetResponse<'a> {
    CapId(u32),
    Data(&'a [u8]),
    Addr([u8; 4]),
    State(u8),
    Ok,
    Err(u8),
    TcpReady {
        events: &'a [u8],
        next_cursor: u16,
    },
    NotReady,
    Eof,
    WriteProgress(u32),
}

/// `NetResponse::Err` code: permission denied.
///
/// service-net returns it before any effect (no socket, DNS query, owner watch
/// or frame) when the kernel-attested caller flags lack the right the request
/// needs (`CALLER_FLAG_NET_*` in `abi::caller_identity`). Clients report it as
/// "permission denied", never as a network failure.
pub const NET_ERR_DENIED: u8 = 0xFC;

// ── Input service ─────────────────────────────────────────────────────────────

/// Requests sent to the input service (`/bin/input`).
#[derive(Debug, Serialize, Deserialize)]
pub enum InputRequest<'a> {
    /// Register the caller as the currently focused cell that receives key/mouse events.
    SetFocus { cell_tid: u32 },
    /// Query which cell currently has focus.  Returns `InputResponse::Focus`.
    GetFocus,
    /// Unregister `cell_tid` from receiving events.  No-op if not currently focused.
    ClearFocus { cell_tid: u32 },
    /// Announce the caller as a **producer** of raw input events.
    ///
    /// A driver cell that owns a physical device the kernel cannot see (the
    /// DWC2/LAN9514 USB host behind a HID keyboard or mouse) has no kernel push
    /// path, so it feeds the service the same `[opcode][code][value]` stream the
    /// kernel emits. The service records the kernel-verified sender TID — never
    /// a TID named in the payload — so a cell cannot impersonate another
    /// producer, and it treats that sender's later messages as raw events
    /// rather than as requests.
    ///
    /// `kind` identifies the producer class ([`input_source`]); Input validates
    /// the caller against the kernel service identity for that class.
    RegisterEventSource { kind: u8 },
    /// Complete borrowed owner-authenticated V1 session request record.
    SessionV1(&'a [u8]),
}

/// `RegisterEventSource` producer classes.
pub mod input_source {
    /// The privileged DWC2 transport and HID driver for the embedded profile.
    pub const USB_HID_HOST: u8 = 2;
}

/// Raw frame a USB host sends when one logical HID interface is removed.
///
/// Layout: `[OP_HID_DEVICE_REMOVED][device:u32 LE]`. Input releases only that
/// interface's held keys.
pub const OP_HID_DEVICE_REMOVED: u8 = 0x07;

/// Raw frame the input service sends **to** a registered producer to program the
/// keyboard's lock-key LEDs.
///
/// Layout: `[OP_SET_LEDS][leds]`, where `leds` is a [`led_bits`] bitmap. The
/// service owns lock state — it is the cell that sees the lock keys — and the
/// device is the only place that state is visible to a user, so the producer is
/// told whenever it changes and once when it registers.
///
/// Fire-and-forget: the service has nothing to do with an answer, and a producer
/// that is restarting drops the frame and gets the current state again when it
/// re-registers. The opcode lives above the producer's own protocol space, so a
/// driver can tell this frame from its other traffic by its first byte.
pub const OP_SET_LEDS: u8 = 0x03;

/// Lock-key bits as they travel on the wire, in HID LED page usage order
/// (HID 1.11 Appendix B.1 — Num Lock 0x01, Caps Lock 0x02, Scroll Lock 0x03).
///
/// This is the order a boot-protocol keyboard's LED byte uses, and the order
/// [`OP_SET_LEDS`] carries, so a producer never has to translate between the two.
pub mod led_bits {
    pub const NUM_LOCK: u8 = 1 << 0;
    pub const CAPS_LOCK: u8 = 1 << 1;
    pub const SCROLL_LOCK: u8 = 1 << 2;
}

/// Responses from the input service.
#[derive(Debug, Serialize, Deserialize)]
pub enum InputResponse<'a> {
    /// Currently focused cell tid (0 = none).
    Focus(u32),
    Ok,
    Err(u8),
    /// Complete borrowed V1 session result record.
    SessionV1(&'a [u8]),
}

// ── Config service ────────────────────────────────────────────────────────────

/// Requests sent to the config service (`/bin/config`).
///
/// `Get` and `List` are open to every caller (Config is not a secret store).
/// `Set` and `Delete` are admitted only when the kernel-attested caller holds
/// [`crate::caller_identity::CALLER_FLAG_CONFIG_WRITE`] **and** the key is in
/// the shell namespace: 1..=64 bytes of `[A-Za-z_][A-Za-z0-9_]*`, excluding the
/// system keys `PATH` and `OS`. Anything else is answered with
/// `ConfigResponse::Err(CONFIG_ERR_DENIED)` before any mutation.
#[derive(Debug, Serialize, Deserialize)]
pub enum ConfigRequest<'a> {
    /// Read the value for `key`.  Returns `ConfigResponse::Value` or `NotFound`.
    Get(&'a str),
    /// Write `value` for `key` (insert or overwrite).  Returns `Ok`, or
    /// `Err(CONFIG_ERR_DENIED)` when the write is not admitted.
    Set { key: &'a str, value: &'a str },
    /// Remove `key`.  Returns `Ok` even if absent (idempotent), or
    /// `Err(CONFIG_ERR_DENIED)` when the delete is not admitted.
    Delete(&'a str),
    /// List all registered keys as a newline-separated string.
    List,
}

/// Responses from the config service.
///
/// `Err(code)` codes: `0xFF` malformed request / reply too large,
/// [`CONFIG_ERR_DENIED`] (`0xFC`) write not admitted.
#[derive(Debug, Serialize, Deserialize)]
pub enum ConfigResponse<'a> {
    /// Value associated with a key.
    Value(&'a str),
    /// Newline-separated list of all keys.
    Keys(&'a str),
    Ok,
    NotFound,
    Err(u8),
}

/// `ConfigResponse::Err` code: permission denied.
///
/// service-config returns it for a `Set`/`Delete` whose caller is unattested,
/// lacks `CALLER_FLAG_CONFIG_WRITE`, or names a key outside the shell
/// namespace. The store is untouched. Clients report it as "permission denied",
/// never as "config unavailable". Same value as [`NET_ERR_DENIED`].
pub const CONFIG_ERR_DENIED: u8 = 0xFC;

// ── Serialization helpers ─────────────────────────────────────────────────────

/// Serialize `msg` into `buf`.
///
/// Returns a slice of the bytes actually written.
///
/// # Errors
/// Returns `postcard::Error` if the encoded message exceeds `buf.len()`.
pub fn encode<'a, T: Serialize>(msg: &T, buf: &'a mut [u8]) -> postcard::Result<&'a mut [u8]> {
    postcard::to_slice(msg, buf)
}

/// Deserialize a typed message from the START of `buf`, tolerating trailing bytes.
///
/// The IPC receive buffer is fixed at 512 bytes; the sender may have encoded a
/// shorter message.  `take_from_bytes` reads exactly what the message needs and
/// ignores the rest — unlike `from_bytes` which requires the entire slice to be
/// consumed.
///
/// For types with borrowed fields (`&str`, `&[u8]`), the returned value borrows
/// from `buf` — it must be consumed before `buf` is reused or overwritten.
pub fn decode<'de, T: Deserialize<'de>>(buf: &'de [u8]) -> postcard::Result<T> {
    let (msg, _remaining) = postcard::take_from_bytes(buf)?;
    Ok(msg)
}

#[cfg(test)]
mod session_wire_tests {
    use super::*;

    #[test]
    fn legacy_input_records_keep_their_complete_postcard_bytes() {
        let mut out = [0; 32];
        for (request, expected) in [
            (InputRequest::SetFocus { cell_tid: 7 }, &b"\x00\x07"[..]),
            (InputRequest::GetFocus, &b"\x01"[..]),
            (InputRequest::ClearFocus { cell_tid: 7 }, &b"\x02\x07"[..]),
            (InputRequest::RegisterEventSource { kind: 2 }, &b"\x03\x02"[..]),
        ] {
            assert_eq!(encode(&request, &mut out).unwrap(), expected);
        }
        for (response, expected) in [
            (InputResponse::Focus(7), &b"\x00\x07"[..]),
            (InputResponse::Ok, &b"\x01"[..]),
            (InputResponse::Err(9), &b"\x02\x09"[..]),
        ] {
            assert_eq!(encode(&response, &mut out).unwrap(), expected);
        }
    }

    #[test]
    fn new_envelopes_append_and_borrow_without_consuming_receive_padding() {
        let mut record = [0; crate::cell_session::MAX_SERVICE_RECORD_LEN];
        crate::cell_session::RequestHeaderV1 {
            selector: 1, total_len: record.len() as u32, request_id: 1,
        }.encode(&mut record).unwrap();
        let mut out = [0; IPC_BUF_SIZE - crate::caller_identity::CALLER_IDENTITY_LEN];
        let encoded = encode(&InputRequest::SessionV1(&record), &mut out).unwrap();
        assert_eq!(encoded[0], 4);
        let encoded_len = encoded.len();
        let decoded: InputRequest<'_> = decode(&out).unwrap();
        let InputRequest::SessionV1(inner) = decoded else { panic!("wrong variant"); };
        assert_eq!(inner, record);
        assert_eq!(inner.as_ptr(), out[encoded_len - record.len()..].as_ptr());
        crate::cell_session::RequestHeaderV1::decode(inner).unwrap();
        assert_eq!(encode(&InputResponse::SessionV1(&record), &mut out).unwrap()[0], 3);
        assert_eq!(encode(&VfsRequest::PreparedChildV1(&record), &mut out).unwrap()[0], 30);
        assert_eq!(encode(&VfsResponse::PreparedChildV1(&record), &mut out).unwrap()[0], 10);
        assert!(!VfsRequest::PreparedChildV1(&record).is_path_addressed());
        assert!(!VfsRequest::PreparedChildV1(&record).requires_mutation_authority());
    }

    #[test]
    fn existing_vfs_tail_and_path_semantics_are_unchanged() {
        let mut out = [0; 32];
        assert_eq!(encode(&VfsRequest::GetFile("a"), &mut out).unwrap(), b"\x00\x01a");
        assert_eq!(encode(&VfsRequest::Rename { old: "a", new: "b" }, &mut out).unwrap(), b"\x1d\x01a\x01b");
        assert_eq!(encode(&VfsResponse::Err(7), &mut out).unwrap(), b"\x04\x07");
        assert!(VfsRequest::Rename { old: "a", new: "b" }.is_path_addressed());
        assert!(VfsRequest::Rename { old: "a", new: "b" }.requires_mutation_authority());
    }

    #[test]
    fn read_path_variants_append_at_31_32_and_11_12() {
        use crate::dir_handles::ViDirHandle;
        use crate::vfs_file_handles::ViVfsFileHandle;
        let mut out = [0; 32];
        let page = VfsRequest::ListAtPage { dir: ViDirHandle(5), cursor: 300 };
        assert_eq!(encode(&page, &mut out).unwrap(), b"\x1f\x05\xac\x02");
        let state = VfsRequest::FileHandleState { file: ViVfsFileHandle(7) };
        assert_eq!(encode(&state, &mut out).unwrap(), b"\x20\x07");
        for request in [page, state] {
            assert!(!request.is_path_addressed());
            assert!(!request.requires_mutation_authority());
        }
        let reply = VfsResponse::DirPage { next: 3, flags: vfs_dir_page::COMPLETE, entries: b"\x00\x01a" };
        assert_eq!(encode(&reply, &mut out).unwrap(), b"\x0b\x03\x01\x03\x00\x01a");
        let reply = VfsResponse::FileState { size: 4001, modified: true };
        assert_eq!(encode(&reply, &mut out).unwrap(), b"\x0c\xa1\x1f\x01");
        // Earlier indices keep their bytes.
        assert_eq!(encode(&VfsResponse::PreparedChildV1(b""), &mut out).unwrap(), b"\x0a\x00");
    }

    #[test]
    fn trust_lookup_appends_after_save_abort() {
        let mut out = [0; 64];
        let req = VfsRequest::TrustLookup { host: "h", port: 22, algorithm: "a", key: b"k" };
        assert_eq!(encode(&req, &mut out).unwrap(), b"\x28\x01h\x16\x01a\x01k");
        assert_eq!(encode(&VfsResponse::Trust(ssh_trust::MISMATCH), &mut out).unwrap(), b"\x10\x02");
        // Read-only and host-addressed: neither mutation authority nor a path.
        assert!(!req.requires_mutation_authority() && !req.is_path_addressed());
    }

    #[test]
    fn first_trust_variants_append_after_trust_lookup() {
        let mut out = [0; 64];
        let r = |id, generation| ObjectRefV1 { id, generation };
        let challenge = VfsRequest::TrustChallenge { host: "h", port: 22, algorithm: "a", key: b"k", session: r(5, 300), epoch: 2 };
        assert_eq!(encode(&challenge, &mut out).unwrap(), b"\x29\x01h\x16\x01a\x01k\x05\xac\x02\x02");
        let commit = VfsRequest::TrustCommit { challenge: r(1, 2) };
        assert_eq!(encode(&commit, &mut out).unwrap(), b"\x2a\x01\x02");
        let reply = VfsResponse::TrustChallenged { confirmation: r(3, 1), challenge: r(1, 2) };
        assert_eq!(encode(&reply, &mut out).unwrap(), b"\x11\x03\x01\x01\x02");
        // Earlier indices keep their bytes; the commit answers are new `Trust` codes.
        assert_eq!(encode(&VfsResponse::Trust(ssh_trust::PENDING), &mut out).unwrap(), b"\x10\x07");
        assert_eq!((ssh_trust::REFUSED, ssh_trust::EXPIRED, ssh_trust::PENDING), (5, 6, 7));
        // Host-addressed, and the caller's mutation authority is never what writes the store.
        for req in [challenge, commit] {
            assert!(!req.requires_mutation_authority() && !req.is_path_addressed());
        }
    }

    #[test]
    fn conditional_save_variants_append_after_file_handle_state() {
        let mut out = [0; 64];
        let expect = SaveExpect::Present { inode: 9, generation: 300 };
        assert_eq!(encode(&VfsRequest::Observe { path: "a" }, &mut out).unwrap(), b"\x21\x01a");
        assert_eq!(encode(&VfsRequest::SaveBegin { dst: "a" }, &mut out).unwrap(), b"\x22\x01a");
        assert_eq!(encode(&VfsRequest::SaveAbort { id: 1 }, &mut out).unwrap(), b"\x27\x01");
        assert_eq!(encode(&VfsRequest::SaveCommit { id: 1, expect }, &mut out).unwrap(),
            b"\x25\x01\x01\x09\xac\x02");
        assert_eq!(encode(&VfsResponse::SaveOutcome(SaveOutcome::Conflict), &mut out).unwrap(), b"\x0f\x01");
        assert_eq!(encode(&VfsResponse::Observed(Observation::Absent), &mut out).unwrap(), b"\x0d\x00");
        // Every step that changes storage needs mutation authority; only the
        // destination-naming steps are path-addressed.
        let seal = VfsRequest::SaveSeal { id: 1, len: 0, sha256: [0; 32] };
        assert!(seal.requires_mutation_authority() && !seal.is_path_addressed());
        let begin = VfsRequest::SaveBegin { dst: "a" };
        assert!(begin.requires_mutation_authority() && begin.is_path_addressed());
        let observe = VfsRequest::Observe { path: "a" };
        assert!(!observe.requires_mutation_authority() && observe.is_path_addressed());
        assert!(!VfsRequest::SaveQuery { id: 1 }.requires_mutation_authority());
        // A directory observation never yields a replaceable expectation.
        let dir = Observation::Present { inode: 2, generation: 1, size: 0, regular: false };
        assert_eq!(dir.expect(), None);
    }

    #[test]
    fn dir_page_entries_round_trip_and_reject_what_a_handle_cannot_name() {
        use vfs_dir_page::{entries, push_entry, Entry, EntryKind, MalformedPage};
        let mut page = [0u8; 16];
        let pos = push_entry(&mut page, 0, EntryKind::Dir, b"src").unwrap().unwrap();
        let pos = push_entry(&mut page, pos, EntryKind::Mount, b"tmp").unwrap().unwrap();
        assert_eq!(pos, 10);
        // Does not fit: nothing written, position unchanged by the caller.
        assert_eq!(push_entry(&mut page, pos, EntryKind::File, b"too-long"), Ok(None));
        assert_eq!(page[pos..], [0; 6]);
        for bad in [&b""[..], b".", b"..", b"a/b", b"a\\b", b"a\nb", b"\xff"] {
            assert!(push_entry(&mut page, 0, EntryKind::File, bad).is_err(), "{bad:?}");
        }
        let decoded: Vec<_> = entries(&page[..pos]).collect();
        assert_eq!(decoded, [
            Ok(Entry { kind: EntryKind::Dir, name: "src" }),
            Ok(Entry { kind: EntryKind::Mount, name: "tmp" }),
        ]);
        assert_eq!(entries(&[]).count(), 0);
        // Unknown kind, truncated name, a bad name: one error, then the end.
        for malformed in [&b"\x04\x01a"[..], b"\x00\x05ab", b"\x00", b"\x00\x02.."] {
            let decoded: Vec<_> = entries(malformed).collect();
            assert_eq!(decoded, [Err(MalformedPage)], "{malformed:?}");
        }
        for kind in [EntryKind::File, EntryKind::Dir, EntryKind::Mount, EntryKind::Other] {
            assert_eq!(EntryKind::from_wire(kind.wire()), Some(kind));
        }
    }

    #[test]
    fn a_full_inline_page_fits_one_reply_frame() {
        let entries = [b'x'; IPC_BUF_SIZE - 96];
        let mut out = [0; IPC_BUF_SIZE];
        let reply = VfsResponse::DirPage { next: u32::MAX, flags: 0xff, entries: &entries };
        assert!(encode(&reply, &mut out).is_ok());
    }
}
