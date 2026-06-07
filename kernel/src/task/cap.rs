//! Kernel-internal capability tokens.
//!
//! Each token is a zero-sized type (ZST).  Constructors are `pub(crate)` so
//! only kernel code can create them — Cell crates are separate Rust
//! compilation units and cannot call `pub(crate)` items from this crate.
//!
//! `Option<ZST>` uses Rust's niche optimization: exactly 1 byte on the wire.
//! Three caps together are 3 bytes, smaller than the previous `KernelPerms(u32)`.

/// Permits raw block-device syscalls (BlkRead, BlkWrite, BlkFlush).
/// Granted to `/bin/vfs` at spawn.
#[derive(Copy, Clone, Debug)]
pub struct BlockIoCap(());

/// Permits network transmit and receive syscalls (NetTx, NetRx).
/// Granted to `/bin/net` at spawn.
#[derive(Copy, Clone, Debug)]
pub struct NetworkCap(());

/// Permits spawning new Cells (SpawnFromPath, SpawnPinned) and hot-swapping (HotSwap).
/// Granted to `/bin/init` and `/bin/shell` at spawn.
#[derive(Copy, Clone, Debug)]
pub struct SpawnCap(());

impl BlockIoCap {
    /// Create a `BlockIoCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self { Self(()) }
}

impl NetworkCap {
    /// Create a `NetworkCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self { Self(()) }
}

impl SpawnCap {
    /// Create a `SpawnCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self { Self(()) }
}

/// Permits use of RISC-V H-extension CSRs (`hstatus`, `hgatp`, `vsatp`, etc.).
///
/// Granted only when BOTH the ELF manifest declares `hypervisor = true` AND
/// `cpu_features::has_h_ext()` confirms the firmware reported H-ext at boot.
/// Always absent on non-riscv64 targets.
#[derive(Copy, Clone, Debug)]
pub struct HypervisorCap(());

impl HypervisorCap {
    /// Create a `HypervisorCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self { Self(()) }
}
