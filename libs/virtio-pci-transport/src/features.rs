// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Transport-level feature policy (virtio 1.2 §6).

use crate::error::InitFailure;

/// `VIRTIO_F_VERSION_1`: the device follows the virtio 1.x layout. Without it
/// the modern common configuration semantics this transport relies on (and
/// little-endian device config) are not guaranteed.
pub const VERSION_1: u64 = 1 << 32;

/// `VIRTIO_F_ACCESS_PLATFORM`: the device's DMA goes through the platform's
/// address translation (IOMMU). A device offering it may refuse to work if
/// the driver declines it, so it is always accepted when offered.
pub const ACCESS_PLATFORM: u64 = 1 << 33;

/// Whether the device must offer [`ACCESS_PLATFORM`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessPlatform {
    /// Refuse the device unless it offers it: the cell's DMA addresses are
    /// only meaningful through the platform's translation.
    Required,
    /// Accept the device either way; the feature is still negotiated whenever
    /// it is offered.
    Accept,
}

/// The feature word the driver writes, given what the device offers, what
/// the device-specific driver asked for, and the access-platform policy.
///
/// Returns `requested` restricted to `device` (a driver must never accept an
/// unoffered feature), plus [`VERSION_1`], plus [`ACCESS_PLATFORM`] when the
/// device offers it.
///
/// # Errors
/// [`InitFailure::Version1NotOffered`] when `device` lacks [`VERSION_1`];
/// [`InitFailure::AccessPlatformMissing`] when `policy` is
/// [`AccessPlatform::Required`] and `device` lacks [`ACCESS_PLATFORM`].
pub fn driver_features(
    device: u64,
    requested: u64,
    policy: AccessPlatform,
) -> Result<u64, InitFailure> {
    if device & VERSION_1 == 0 {
        return Err(InitFailure::Version1NotOffered);
    }
    if policy == AccessPlatform::Required && device & ACCESS_PLATFORM == 0 {
        return Err(InitFailure::AccessPlatformMissing);
    }
    Ok((requested & device) | VERSION_1 | (device & ACCESS_PLATFORM))
}
