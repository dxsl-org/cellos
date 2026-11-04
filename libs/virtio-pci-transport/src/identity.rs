// SPDX-License-Identifier: Apache-2.0 OR MIT
//! PCI vendor/device ID → virtio device type (virtio 1.2 §4.1.2).

use crate::error::TransportError;
use virtio_drivers::transport::DeviceType;

/// PCI vendor ID of every virtio function.
pub const VIRTIO_VENDOR_ID: u16 = 0x1af4;

/// Modern device IDs are `0x1040 + virtio device ID`.
const MODERN_FIRST: u16 = 0x1040;
const MODERN_LAST: u16 = 0x107f;

/// Virtio device ID of a transitional PCI device ID. Transitional IDs are not
/// an offset: the spec fixes this table and leaves the rest of
/// `0x1000..=0x103f` unassigned.
fn transitional_virtio_id(device_id: u16) -> Option<u8> {
    match device_id {
        0x1000 => Some(1), // network card
        0x1001 => Some(2), // block device
        0x1002 => Some(5), // memory ballooning (traditional)
        0x1003 => Some(3), // console
        0x1004 => Some(8), // SCSI host
        0x1005 => Some(4), // entropy source
        0x1009 => Some(9), // 9P transport
        _ => None,
    }
}

/// Maps a PCI function's IDs to the virtio device type its driver serves.
///
/// Transitional IDs are accepted because transitional devices also expose the
/// modern capabilities; whether they do is checked by [`crate::parse_caps`].
///
/// # Errors
/// [`TransportError::InvalidVendor`] unless `vendor_id` is
/// [`VIRTIO_VENDOR_ID`]; [`TransportError::UnknownDevice`] for IDs outside
/// both ranges, unassigned transitional IDs, and modern IDs whose virtio type
/// the driver library does not define.
pub fn device_type_for(vendor_id: u16, device_id: u16) -> Result<DeviceType, TransportError> {
    if vendor_id != VIRTIO_VENDOR_ID {
        return Err(TransportError::InvalidVendor(vendor_id));
    }
    let device_type = match device_id {
        MODERN_FIRST..=MODERN_LAST => DeviceType::from(device_id - MODERN_FIRST),
        _ => transitional_virtio_id(device_id).map_or(DeviceType::Invalid, DeviceType::from),
    };
    if device_type == DeviceType::Invalid {
        return Err(TransportError::UnknownDevice(device_id));
    }
    Ok(device_type)
}
