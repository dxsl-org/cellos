// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::features::{driver_features, ACCESS_PLATFORM, VERSION_1};
use crate::{device_type_for, AccessPlatform, InitFailure, TransportError};
use virtio_drivers::transport::DeviceType;

const ACCEPT: AccessPlatform = AccessPlatform::Accept;
const REQUIRED: AccessPlatform = AccessPlatform::Required;

#[test]
fn modern_ids_map_by_offset() {
    let cases = [
        (0x1041, DeviceType::Network),
        (0x1042, DeviceType::Block),
        (0x1048, DeviceType::ScsiHost),
        (0x1050, DeviceType::GPU),
        (0x1059, DeviceType::Sound),
    ];
    for (id, expected) in cases {
        assert_eq!(device_type_for(0x1af4, id), Ok(expected), "{id:#x}");
    }
}

#[test]
fn transitional_ids_follow_spec_table() {
    let cases = [
        (0x1000, DeviceType::Network),
        (0x1001, DeviceType::Block),
        (0x1002, DeviceType::MemoryBalloon),
        (0x1003, DeviceType::Console),
        (0x1004, DeviceType::ScsiHost),
        (0x1005, DeviceType::EntropySource),
        (0x1009, DeviceType::_9P),
    ];
    for (id, expected) in cases {
        assert_eq!(device_type_for(0x1af4, id), Ok(expected), "{id:#x}");
    }
}

#[test]
fn unknown_ids_refused() {
    for id in [0x1006, 0x103f, 0x1040, 0x1060, 0x107f, 0x1080, 0x0fff] {
        assert_eq!(
            device_type_for(0x1af4, id),
            Err(TransportError::UnknownDevice(id)),
            "{id:#x}"
        );
    }
}

#[test]
fn non_virtio_vendor_refused() {
    assert_eq!(
        device_type_for(0x8086, 0x1042),
        Err(TransportError::InvalidVendor(0x8086))
    );
}

#[test]
fn version_1_forced_into_driver_features() {
    let device = VERSION_1 | 0b110;
    assert_eq!(driver_features(device, 0b10, ACCEPT), Ok(VERSION_1 | 0b10));
}

#[test]
fn version_1_not_offered_fails() {
    assert_eq!(
        driver_features(ACCESS_PLATFORM | 1, 1, ACCEPT),
        Err(InitFailure::Version1NotOffered)
    );
}

#[test]
fn access_platform_negotiated_when_offered() {
    let device = VERSION_1 | ACCESS_PLATFORM;
    assert_eq!(driver_features(device, 0, ACCEPT), Ok(device));
    assert_eq!(driver_features(device, 0, REQUIRED), Ok(device));
    assert_eq!(
        driver_features(VERSION_1, ACCESS_PLATFORM, ACCEPT),
        Ok(VERSION_1)
    );
}

#[test]
fn access_platform_required_but_not_offered_fails() {
    assert_eq!(
        driver_features(VERSION_1 | 1, 1, REQUIRED),
        Err(InitFailure::AccessPlatformMissing)
    );
}

#[test]
fn driver_features_masked_to_device_offer() {
    assert_eq!(
        driver_features(VERSION_1 | 1, 0b11, ACCEPT),
        Ok(VERSION_1 | 1)
    );
}
