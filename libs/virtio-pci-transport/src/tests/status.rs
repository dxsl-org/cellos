// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The generic `begin_init`/`finish_init` flow against the transport's
//! status tracking.

use super::fake::*;
use crate::{AccessPlatform, InitFailure, VirtioPciTransport};
use virtio_drivers::transport::Transport;

const ACK_DRIVER: u8 = 0b0011;
const FEATURES_OK: u8 = 0b1000;
const DRIVER_OK: u8 = 0b0100;
const FAILED: u8 = 0x80;

#[test]
fn begin_init_negotiates_and_disables_config_vector() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b11u32);
    let mut t = build(&mut fake, &rec);
    assert!(!t.negotiated_ok());

    assert_eq!(t.begin_init(Features::A), Features::A);
    assert!(t.negotiated_ok());
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), ACK_DRIVER | FEATURES_OK);
    assert_eq!(fake.common::<u16>(MSIX_CONFIG), 0xFFFF);
    // Upper word written last: VERSION_1 forced, ACCESS_PLATFORM accepted.
    assert_eq!(fake.common::<u32>(DRIVER_FEATURE), 0b11);

    t.finish_init();
    let ready = ACK_DRIVER | FEATURES_OK | DRIVER_OK;
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), ready);
}

#[test]
fn version_1_missing_sets_failed_instead_of_features_ok() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    let mut t = build(&mut fake, &rec);

    t.begin_init(Features::A | Features::B);
    assert!(!t.negotiated_ok());
    assert_eq!(t.init_failure(), Some(InitFailure::Version1NotOffered));
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), ACK_DRIVER | FAILED);
    assert_eq!(fake.common::<u16>(MSIX_CONFIG), 0);

    t.finish_init();
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), ACK_DRIVER | FAILED);
}

#[test]
fn reset_clears_recorded_failure() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    let mut t = build(&mut fake, &rec);
    t.begin_init(Features::A);
    assert!(t.init_failure().is_some());

    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b01u32);
    t.begin_init(Features::A);
    assert_eq!(t.init_failure(), None);
    assert!(t.negotiated_ok());
}

#[test]
fn drop_resets_device() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b01u32);
    let mut t = build(&mut fake, &rec);
    t.begin_init(Features::A);
    t.finish_init();
    drop(t);
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), 0);
}

#[test]
fn access_platform_required_rechecked_at_feature_write() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b11u32);
    let mut t = VirtioPciTransport::new(&rec, &mut fake, AccessPlatform::Required).unwrap();
    // The device withdraws the offer between construction and negotiation.
    fake.write(BAR, COMMON + DEVICE_FEATURE, 0b01u32);
    t.begin_init(Features::A);
    assert_eq!(t.init_failure(), Some(InitFailure::AccessPlatformMissing));
    assert_eq!(fake.common::<u8>(DEVICE_STATUS), ACK_DRIVER | FAILED);
    assert_eq!(fake.common::<u32>(DRIVER_FEATURE), 0, "nothing negotiated");
    assert!(!t.negotiated_ok());
}
