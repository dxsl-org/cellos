//! `OP_READ_RUN` through the router: multi-sector reads and failed runs.

use super::tests::two_disks;
use super::DeviceRef;
use types::block_wire::{BlockOutcome, MAX_RUN_SECTORS, OP_READ_RUN, SECTOR_SIZE};

const NVME_DEV: DeviceRef = DeviceRef::Device(1);
const NVME_TID: usize = 9;

fn sector(fill: u8) -> [u8; SECTOR_SIZE] {
    [fill; SECTOR_SIZE]
}

#[test]
fn a_serving_provider_moves_a_run_of_sectors_per_request() {
    let (mut fake, mut router) = two_disks();
    let medium = fake.media.entry(NVME_TID).or_default();
    for lba in 100..100 + 16 {
        medium.insert(lba, sector(lba as u8));
    }
    fake.ops.clear();

    let mut buf = alloc::vec![0u8; 16 * SECTOR_SIZE];
    assert!(router.read_sectors(&mut fake, NVME_DEV, 100, &mut buf));

    assert_eq!(MAX_RUN_SECTORS, 7);
    assert_eq!(fake.ops, [OP_READ_RUN; 3], "7 + 7 + 2 sectors");
    for (k, got) in buf.chunks_exact(SECTOR_SIZE).enumerate() {
        assert_eq!(got, sector((100 + k) as u8), "sector {}", 100 + k);
    }
}

#[test]
fn a_provider_refusing_the_run_fails_the_read_without_a_per_sector_retry() {
    let (mut fake, mut router) = two_disks();
    fake.read_run_outcome = Some(BlockOutcome::NotIssued);
    fake.ops.clear();
    let mut buf = alloc::vec![0u8; 4 * SECTOR_SIZE];
    assert!(!router.read_sectors(&mut fake, NVME_DEV, 6, &mut buf));
    assert_eq!(fake.ops, [OP_READ_RUN]);
}

#[test]
fn a_failed_run_is_a_failure_and_copies_nothing() {
    let (mut fake, mut router) = two_disks();
    let mut buf = alloc::vec![0u8; 2 * SECTOR_SIZE];
    assert!(router.read_sectors(&mut fake, NVME_DEV, 0, &mut buf));

    fake.read_run_outcome = Some(BlockOutcome::Indeterminate);
    let mut buf = alloc::vec![0xEEu8; 2 * SECTOR_SIZE];
    assert!(!router.read_sectors(&mut fake, NVME_DEV, 0, &mut buf));
    assert!(buf.iter().all(|&b| b == 0xEE), "no reply bytes copied");
}

#[test]
fn a_partial_sector_buffer_is_refused_before_any_request() {
    let (mut fake, mut router) = two_disks();
    fake.ops.clear();
    let mut buf = alloc::vec![0u8; SECTOR_SIZE + 1];
    assert!(!router.read_sectors(&mut fake, NVME_DEV, 0, &mut buf));
    assert!(fake.ops.is_empty());
}
