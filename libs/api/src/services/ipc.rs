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

/// IPC payload buffer size.  Must be large enough for the largest serialized
/// message in the system — currently a VFS Write with ~900-byte content
/// (~916 bytes encoded).  4 KiB gives comfortable headroom.
pub const IPC_BUF_SIZE: usize = 4096;
/// Maximum inline TCP data that leaves conservative postcard framing headroom.
pub const NET_TCP_INLINE_DATA_MAX: usize = IPC_BUF_SIZE - 256;

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
            | Self::SyncHandle { .. } => false,
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
            | Self::Rename { .. } => true,

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
            | Self::ReadHandleGrant { .. } => false,
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
}

// ── Network service ───────────────────────────────────────────────────────────

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
}

/// Responses from the network service.
#[derive(Debug, Serialize, Deserialize)]
pub enum NetResponse<'a> {
    CapId(u32),
    Data(&'a [u8]),
    Addr([u8; 4]),
    State(u8),
    Ok,
    Err(u8),
}

// ── Input service ─────────────────────────────────────────────────────────────

/// Requests sent to the input service (`/bin/input`).
#[derive(Debug, Serialize, Deserialize)]
pub enum InputRequest {
    /// Register the caller as the currently focused cell that receives key/mouse events.
    SetFocus { cell_tid: u32 },
    /// Query which cell currently has focus.  Returns `InputResponse::Focus`.
    GetFocus,
    /// Unregister `cell_tid` from receiving events.  No-op if not currently focused.
    ClearFocus { cell_tid: u32 },
}

/// Responses from the input service.
#[derive(Debug, Serialize, Deserialize)]
pub enum InputResponse {
    /// Currently focused cell tid (0 = none).
    Focus(u32),
    Ok,
    Err(u8),
}

// ── Config service ────────────────────────────────────────────────────────────

/// Requests sent to the config service (`/bin/config`).
#[derive(Debug, Serialize, Deserialize)]
pub enum ConfigRequest<'a> {
    /// Read the value for `key`.  Returns `ConfigResponse::Value` or `NotFound`.
    Get(&'a str),
    /// Write `value` for `key` (insert or overwrite).  Returns `Ok`.
    Set { key: &'a str, value: &'a str },
    /// Remove `key`.  Returns `Ok` even if absent (idempotent).
    Delete(&'a str),
    /// List all registered keys as a newline-separated string.
    List,
}

/// Responses from the config service.
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
