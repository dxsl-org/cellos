//! Error types for hotswap orchestration.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HotswapError {
    /// Target service name is not recognized or not currently live.
    ServiceNotFound = 0,
    /// Freeze syscall rejected (permission denied or tid invalid).
    FreezeFailed = 1,
    /// Unable to send Snapshot IPC to old cell.
    SnapshotIpcFailed = 2,
    /// Old cell did not stash state within the timeout.
    SnapshotTimeout = 3,
    /// Spawn of new ELF failed (path missing, no SpawnCap, loader error).
    SpawnFailed = 4,
    /// Unable to send Restore IPC to new cell.
    RestoreIpcFailed = 5,
    /// New cell did not call sys_hotswap_ready() within the timeout.
    ReadyTimeout = 6,
    /// Service mapping changed before it could be paused for snapshotting.
    PauseFailed = 7,
    /// Replacement became ready but could not be published as the provider.
    RegisterFailed = 8,
}

impl HotswapError {
    /// Numeric code sent in the StatusReply (phase = 0xFF = error).
    pub fn as_code(self) -> u8 {
        self as u8
    }
}
