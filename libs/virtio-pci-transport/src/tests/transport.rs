// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::fake::*;
use crate::{AccessPlatform, BarMapper, CfgType, TransportError, VirtioPciTransport};
use core::ptr::NonNull;
use virtio_drivers::transport::{DeviceType, Transport};
use virtio_drivers::Error;

#[test]
fn new_maps_each_bar_once() {
    let mut caps = qemu_caps();
    caps[4] = vcap(2, 2, 0x100, 0x100, Some(MULTIPLIER));
    let rec = record_with(&caps);
    let mut fake = FakeBars::new(&rec);
    let t = build(&mut fake, &rec);
    assert_eq!(t.device_type(), DeviceType::Block);
    assert_eq!(
        fake.maps,
        [(BAR, BAR_BASE, BAR_LEN), (2, 0xfd00_0000, 0x1000)]
    );
}

#[test]
fn map_failure_reported() {
    struct Refuse;
    impl BarMapper for Refuse {
        fn map(&mut self, _: u8, _: u64, _: u64) -> Option<NonNull<u8>> {
            None
        }
    }
    let err = VirtioPciTransport::new(&record(), &mut Refuse, AccessPlatform::Accept).err();
    assert_eq!(err, Some(TransportError::MapFailed(BAR)));
}

#[test]
fn misaligned_device_region_refused() {
    let mut caps = qemu_caps();
    caps[3] = vcap(4, BAR, DEVICE as u32 + 2, 0x10, None);
    let rec = record_with(&caps);
    let mut fake = FakeBars::new(&rec);
    let err = VirtioPciTransport::new(&rec, &mut fake, AccessPlatform::Accept).err();
    assert_eq!(err, Some(TransportError::MisalignedRegion(CfgType::Device)));
}

#[test]
fn access_platform_required_but_missing_refused() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b01u32);
    let err = VirtioPciTransport::new(&rec, &mut fake, AccessPlatform::Required).err();
    assert_eq!(err, Some(TransportError::AccessPlatformMissing));
    assert_eq!(fake.common::<u32>(0), 1, "upper feature word selected");
}

#[test]
fn access_platform_offered_is_negotiated() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b11u32);
    let mut t = VirtioPciTransport::new(&rec, &mut fake, AccessPlatform::Required).unwrap();
    t.write_driver_features(0);
    assert_eq!(fake.common::<u32>(DRIVER_FEATURE_SELECT), 1);
    assert_eq!(fake.common::<u32>(DRIVER_FEATURE), 0b11);
}

#[test]
fn version_1_forced_into_written_features() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b01u32);
    let mut t = build(&mut fake, &rec);
    t.write_driver_features(0);
    assert_eq!(fake.common::<u32>(DRIVER_FEATURE), 0b01);
    assert_eq!(t.init_failure(), None);
}

#[test]
fn config_space_bounds_checked() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    let t = build(&mut fake, &rec);
    let ptr = t.config_space::<[u32; 4]>().unwrap();
    assert_eq!(ptr.cast::<u8>().as_ptr(), fake.addr(BAR, DEVICE));

    let mut caps = qemu_caps();
    caps[3] = vcap(4, BAR, DEVICE as u32, 8, None);
    let rec = record_with(&caps);
    let mut fake = FakeBars::new(&rec);
    let t = build(&mut fake, &rec);
    assert!(t.config_space::<u64>().is_ok());
    assert_eq!(t.config_space::<[u8; 9]>(), Err(Error::ConfigSpaceTooSmall));
}

#[test]
fn config_space_alignment_checked() {
    let mut caps = qemu_caps();
    caps[3] = vcap(4, BAR, DEVICE as u32 + 4, 0x10, None);
    let rec = record_with(&caps);
    let mut fake = FakeBars::new(&rec);
    let t = build(&mut fake, &rec);
    assert!(t.config_space::<u32>().is_ok());
    assert_eq!(t.config_space::<u64>(), Err(Error::InvalidParam));
}

#[test]
fn config_space_missing_without_device_cap() {
    let caps = qemu_caps();
    let rec = record_with(&[caps[1], caps[2], caps[4]]);
    let mut fake = FakeBars::new(&rec);
    let t = build(&mut fake, &rec);
    assert_eq!(t.config_space::<u32>(), Err(Error::ConfigSpaceMissing));
}

#[test]
fn ack_interrupt_reads_isr() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    let mut t = build(&mut fake, &rec);
    for (isr, pending) in [(0u8, false), (1, true), (2, true), (4, false)] {
        fake.write(BAR, ISR, isr);
        assert_eq!(t.ack_interrupt(), pending, "isr {isr:#x}");
    }
}

#[test]
fn new_resets_device_left_running() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    // A killed previous owner left the device live on its rings.
    fake.write(BAR, COMMON + DEVICE_STATUS, 0x0Fu8);
    fake.write(BAR, COMMON + QUEUE_ENABLE, 1u16);
    let t = build(&mut fake, &rec);
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), 0);
    assert!(!t.negotiated_ok());
}
