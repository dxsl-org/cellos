// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::fake::*;
use crate::InitFailure;
use virtio_drivers::transport::Transport;

#[test]
fn queue_set_programs_queue_registers() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    let mut t = build(&mut fake, &rec);
    t.queue_set(1, 256, 0x1_2345_6000, 0x1_2345_7000, 0x1_2345_8000);
    assert_eq!(fake.common::<u16>(QUEUE_SELECT), 1);
    assert_eq!(fake.common::<u16>(QUEUE_SIZE), 256);
    assert_eq!(fake.common::<u64>(QUEUE_DESC), 0x1_2345_6000);
    assert_eq!(fake.common::<u64>(QUEUE_DRIVER), 0x1_2345_7000);
    assert_eq!(fake.common::<u64>(QUEUE_DEVICE), 0x1_2345_8000);
    assert_eq!(fake.common::<u16>(QUEUE_MSIX_VECTOR), 0xFFFF);
    assert_eq!(fake.common::<u16>(QUEUE_ENABLE), 1);
    assert!(t.queue_used(1));
    assert!(!t.requires_legacy_layout());
}

#[test]
fn max_queue_size_reads_selected_queue() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + QUEUE_SIZE, 128u16);
    let mut t = build(&mut fake, &rec);
    assert_eq!(t.max_queue_size(3), 128);
    assert_eq!(fake.common::<u16>(QUEUE_SELECT), 3);
}

#[test]
fn notify_uses_queue_notify_off_times_multiplier() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 3u16);
    let mut t = build(&mut fake, &rec);
    t.queue_set(2, 16, 0x1000, 0x2000, 0x3000);
    // The cached offset must survive the register changing under it.
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 0u16);
    t.notify(2);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY + 3 * MULTIPLIER as usize), 2);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY), 0);
    // Queues beyond the cache read queue_notify_off at notify time.
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 5u16);
    t.notify(40);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY + 5 * MULTIPLIER as usize), 40);
    assert_eq!(fake.common::<u16>(QUEUE_SELECT), 40);
}

#[test]
fn zero_multiplier_shares_one_notify_address() {
    let mut caps = qemu_caps();
    caps[4] = vcap(2, BAR, NOTIFY as u32, 2, Some(0));
    let rec = record_with(&caps);
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 7u16);
    let mut t = build(&mut fake, &rec);
    t.queue_set(1, 16, 0x1000, 0x2000, 0x3000);
    t.notify(1);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY), 1);
    assert!(t.init_failure().is_none());
}

#[test]
fn notify_off_out_of_range_fails_queue() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 0x400u16);
    let mut t = build(&mut fake, &rec);
    t.queue_set(0, 16, 0x1000, 0x2000, 0x3000);
    assert_eq!(t.init_failure(), Some(InitFailure::NotifyOutOfRange(0)));
    assert_eq!(fake.common::<u16>(QUEUE_SIZE), 0);
    assert_eq!(fake.common::<u64>(QUEUE_DESC), 0);
    assert_eq!(fake.common::<u16>(QUEUE_MSIX_VECTOR), 0);
    assert_eq!(fake.common::<u16>(QUEUE_ENABLE), 0);
    assert_eq!(fake.common::<u8>(DEVICE_STATUS) & 0x80, 0x80);
}

#[test]
fn failure_blocks_queue_set() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    let mut t = build(&mut fake, &rec);
    t.begin_init(Features::A);
    assert_eq!(t.init_failure(), Some(InitFailure::Version1NotOffered));
    t.queue_set(1, 16, 0x1000, 0x2000, 0x3000);
    assert_eq!(fake.common::<u16>(QUEUE_SELECT), 0);
    assert_eq!(fake.common::<u16>(QUEUE_SIZE), 0);
    assert_eq!(fake.common::<u64>(QUEUE_DESC), 0);
    assert_eq!(fake.common::<u16>(QUEUE_MSIX_VECTOR), 0);
    assert_eq!(fake.common::<u16>(QUEUE_ENABLE), 0);
    t.notify(1);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY), 0);
}

#[test]
fn failure_blocks_notify_of_previously_set_queue() {
    let rec = record();
    let mut fake = FakeBars::new(&rec);
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 3u16);
    let mut t = build(&mut fake, &rec);
    t.queue_set(2, 16, 0x1000, 0x2000, 0x3000);
    fake.write(BAR, COMMON + QUEUE_NOTIFY_OFF, 0x400u16);
    t.queue_set(0, 16, 0x4000, 0x5000, 0x6000);
    assert_eq!(t.init_failure(), Some(InitFailure::NotifyOutOfRange(0)));
    t.notify(2);
    t.notify(40);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY + 3 * MULTIPLIER as usize), 0);
    assert_eq!(fake.read::<u16>(BAR, NOTIFY), 0);
}
