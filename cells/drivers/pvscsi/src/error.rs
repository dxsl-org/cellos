//! Why bring-up stopped, and why a command produced no SAM status.

use core::fmt;
use types::ViError;

/// Why a command produced no SAM status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PvscsiError {
    /// The SAM LUN is not a single-level LUN below 256.
    UnsupportedLun([u8; 8]),
    /// The CDB is longer than the descriptor's 16 bytes.
    CdbTooLong(usize),
    /// The data phase exceeds the bounce buffer.
    DataTooLarge(usize),
    /// Selection timeout: nothing answers at that target.
    NoTarget,
    /// Another non-success host adapter status.
    HostStatus(u16),
    /// No completion within the I/O budget; the adapter is now poisoned.
    Timeout,
    /// A completion that does not describe the request in flight; poisons.
    BadCompletion,
    /// An earlier timeout or bad completion left the adapter unusable.
    Poisoned,
    /// The request ring has no free entry (the device stopped consuming).
    RingFull,
}

impl PvscsiError {
    /// Whether the device was (or may have been) handed the command.
    pub fn issued(&self) -> bool {
        matches!(
            self,
            Self::NoTarget | Self::HostStatus(_) | Self::Timeout | Self::BadCompletion
        )
    }

    /// Whether the adapter refuses every later command after this error.
    pub fn poisons(&self) -> bool {
        matches!(self, Self::Timeout | Self::BadCompletion | Self::Poisoned)
    }
}

/// Why bring-up stopped.
#[derive(Clone, Copy, Debug)]
pub enum InitError {
    Mmio(ViError),
    /// `ADAPTER_RESET` returned this status.
    Reset(u32),
    /// DMA allocation or authorization failed.
    Dma,
    /// `SETUP_RINGS` returned this status.
    SetupRings(u32),
    /// The device reported ring sizes (log2) other than the pages given.
    RingGeometry(u32, u32),
}

impl From<ViError> for InitError {
    fn from(error: ViError) -> Self {
        Self::Mmio(error)
    }
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mmio(error) => write!(f, "register access: {error:?}"),
            Self::Reset(status) => write!(f, "ADAPTER_RESET status {status:#x}"),
            Self::Dma => write!(f, "DMA allocation/authorization failed"),
            Self::SetupRings(status) => write!(f, "SETUP_RINGS status {status:#x}"),
            Self::RingGeometry(req, cmp) => write!(f, "ring sizes 2^{req}/2^{cmp}"),
        }
    }
}
