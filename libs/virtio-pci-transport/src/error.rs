// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Construction errors and post-construction initialisation failures.

use crate::caps::CfgType;
use core::fmt::{self, Display, Formatter};

/// Why [`crate::VirtioPciTransport::new`] (or [`crate::parse_caps`] /
/// [`crate::device_type_for`]) refused a PCI function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    /// The PCI vendor ID is not the virtio vendor ID `0x1af4`.
    InvalidVendor(u16),
    /// The PCI device ID is outside the transitional and modern virtio ranges,
    /// or names a virtio device type the driver library does not know.
    UnknownDevice(u16),
    /// No virtio structure capability at all: the function only offers the
    /// legacy I/O-port interface, which this transport does not drive.
    LegacyOnly,
    /// Some virtio capabilities exist but this mandatory structure is absent.
    MissingCap(CfgType),
    /// `notify_off_multiplier` is odd, so queue notify addresses would not be
    /// 16-bit aligned.
    InvalidNotifyOffMultiplier(u32),
    /// A structure names a BAR that is absent, I/O, or the upper half of a
    /// 64-bit BAR.
    NotMemoryBar(u8),
    /// A structure names a memory BAR with no address assigned.
    BarNotAllocated(u8),
    /// The structure's `offset + length` runs past the end of its BAR.
    RegionOutOfBar(CfgType),
    /// The structure is shorter than the registers the transport touches.
    RegionTooSmall(CfgType),
    /// The [`crate::BarMapper`] could not map this BAR.
    MapFailed(u8),
    /// The mapped structure is not aligned for its widest register access.
    MisalignedRegion(CfgType),
    /// [`crate::AccessPlatform::Required`] was requested but the device does
    /// not offer `VIRTIO_F_ACCESS_PLATFORM`.
    AccessPlatformMissing,
    /// The device did not complete the reset `new` performs within
    /// [`crate::RESET_SPIN_BUDGET`] status reads, or read back as gone
    /// (`0xFF`). A device that cannot be quiesced is not handed to a driver.
    ResetFailed,
}

impl Display for TransportError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::InvalidVendor(id) => write!(f, "vendor {id:#06x} is not virtio"),
            Self::UnknownDevice(id) => write!(f, "device {id:#06x} is not a known virtio device"),
            Self::LegacyOnly => write!(f, "legacy-only virtio device"),
            Self::MissingCap(kind) => write!(f, "missing virtio {kind:?} capability"),
            Self::InvalidNotifyOffMultiplier(m) => write!(f, "odd notify_off_multiplier {m}"),
            Self::NotMemoryBar(bar) => write!(f, "BAR {bar} is not a memory BAR"),
            Self::BarNotAllocated(bar) => write!(f, "BAR {bar} has no address"),
            Self::RegionOutOfBar(kind) => write!(f, "virtio {kind:?} region exceeds its BAR"),
            Self::RegionTooSmall(kind) => write!(f, "virtio {kind:?} region too small"),
            Self::MapFailed(bar) => write!(f, "mapping BAR {bar} failed"),
            Self::MisalignedRegion(kind) => write!(f, "virtio {kind:?} region misaligned"),
            Self::AccessPlatformMissing => write!(f, "device lacks VIRTIO_F_ACCESS_PLATFORM"),
            Self::ResetFailed => write!(f, "device did not complete reset"),
        }
    }
}

/// A failure detected after construction, while the generic
/// `Transport::begin_init`/`queue_set` flow runs. Those trait methods cannot
/// return errors, so the transport records the first failure, sets the
/// device's `FAILED` status bit, and exposes it through
/// [`crate::VirtioPciTransport::init_failure`]. While a failure is recorded no
/// queue is programmed or notified; only a successful reset clears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitFailure {
    /// The device does not offer `VIRTIO_F_VERSION_1`.
    Version1NotOffered,
    /// [`crate::AccessPlatform::Required`] is in force and the device no
    /// longer offers `VIRTIO_F_ACCESS_PLATFORM`.
    AccessPlatformMissing,
    /// The device cleared `FEATURES_OK` on read-back: it rejected the subset.
    FeaturesRejected,
    /// The queue's `queue_notify_off * notify_off_multiplier` lies outside the
    /// notify region, so it can never be notified; it was neither programmed
    /// nor enabled.
    NotifyOutOfRange(u16),
    /// A requested reset did not complete within [`crate::RESET_SPIN_BUDGET`]
    /// status reads, or the device read back as gone (`0xFF`). `FAILED` is
    /// not written: the device is not responding to status writes.
    ResetFailed,
}

impl Display for InitFailure {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Version1NotOffered => write!(f, "device lacks VIRTIO_F_VERSION_1"),
            Self::AccessPlatformMissing => write!(f, "device lacks VIRTIO_F_ACCESS_PLATFORM"),
            Self::FeaturesRejected => write!(f, "device rejected the negotiated features"),
            Self::NotifyOutOfRange(q) => write!(f, "queue {q} notify address outside region"),
            Self::ResetFailed => write!(f, "device did not complete reset"),
        }
    }
}
