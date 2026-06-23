//! Error types for hotswap orchestration.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotswapError {
    /// Target service name is not recognized or not currently live.
    ServiceNotFound,
    /// Freeze syscall rejected (permission denied or tid invalid).
    FreezeFailed,
    /// Unable to send Snapshot IPC to old cell.
    SnapshotIpcFailed,
    /// Old cell did not stash state within the timeout.
    SnapshotTimeout,
    /// Spawn of new ELF failed (path missing, no SpawnCap, loader error).
    SpawnFailed,
    /// Unable to send Restore IPC to new cell.
    RestoreIpcFailed,
    /// New cell did not call sys_hotswap_ready() within the timeout.
    ReadyTimeout,
}

impl HotswapError {
    /// Numeric code sent in the StatusReply (phase = 0xFF = error).
    pub fn as_code(self) -> u8 {
        self as u8
    }
}
