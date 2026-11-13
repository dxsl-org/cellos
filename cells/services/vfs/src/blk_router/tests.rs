use super::discovery::discover;
use super::router::Router;
use super::{BlockSys, Call, DeviceRef};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use api::block_route::{ViBlockRouteV1, FLAG_PROVIDER_LIVE};
use types::block_wire::{
    BlockOutcome, ReadRunRequest, OP_READ, OP_READ_RUN, OP_WRITE, SECTOR_SIZE,
};

pub(super) const AHCI: u64 = 0xFA; // 00:1f.2
pub(super) const NVME: u64 = 0x300; // 03:00.0

fn route(kind: u16, id: u64, tid: u64, cell: u64) -> ViBlockRouteV1 {
    let flags = if tid == 0 { 0 } else { FLAG_PROVIDER_LIVE };
    let (cell, generation) = if tid == 0 { (0, 0) } else { (cell, 1) };
    ViBlockRouteV1 {
        version: 1,
        kind,
        flags,
        id,
        provider_tid: tid,
        cell_id: cell,
        generation,
    }
}

/// Kernel route table plus one medium per provider tid.
#[derive(Default)]
pub(super) struct Fake {
    routes: Vec<ViBlockRouteV1>,
    pub media: BTreeMap<usize, BTreeMap<u64, [u8; SECTOR_SIZE]>>,
    /// Provider that exits right after accepting its next request.
    dies_after_send: Option<usize>,
    /// Provider that is alive but never answers: every call runs out its
    /// bound, `dispatched` saying whether it had taken the request.
    frozen: Option<(usize, bool)>,
    /// The scheduler clock `now_ticks` reports.
    now: u64,
    sent: Vec<usize>,
    /// Opcode of every accepted request, in order.
    pub ops: Vec<u16>,
    kernel_calls: usize,
    logs: Vec<String>,
    /// Outcome an `OP_READ_RUN` reports (default `Completed`).
    pub read_run_outcome: Option<BlockOutcome>,
}

impl Fake {
    fn live(&self, tid: usize) -> bool {
        self.routes
            .iter()
            .any(|r| r.provider().is_some_and(|p| p.0 as usize == tid))
    }
    fn kill(&mut self, tid: usize) {
        for r in self
            .routes
            .iter_mut()
            .filter(|r| r.provider_tid as usize == tid)
        {
            *r = route(r.kind, r.id, 0, 0);
        }
    }
    fn logged(&self, needle: &str) -> bool {
        self.logs.iter().any(|l| l.contains(needle))
    }
}

impl BlockSys for Fake {
    fn lookup_route(&mut self, index: u32) -> Option<ViBlockRouteV1> {
        self.routes.get(index as usize).copied()
    }
    fn call(&mut self, tid: usize, req: &[u8], buf: &mut [u8]) -> Call {
        if !self.live(tid) {
            return Call::Refused;
        }
        self.sent.push(tid);
        self.ops.push(u16::from_le_bytes([req[0], req[1]]));
        if let Some((_, dispatched)) = self.frozen.filter(|&(frozen, _)| frozen == tid) {
            return Call::Expired { dispatched };
        }
        if self.dies_after_send == Some(tid) {
            self.kill(tid);
            return Call::PeerGone;
        }
        buf.fill(0);
        let op = u16::from_le_bytes([req[0], req[1]]);
        let lba = req
            .get(2..10)
            .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()));
        let medium = self.media.entry(tid).or_default();
        match op {
            OP_READ => {
                buf[1..].copy_from_slice(&medium.get(&lba).copied().unwrap_or([0; SECTOR_SIZE]))
            }
            OP_WRITE => drop(medium.insert(lba, req[10..].try_into().unwrap())),
            OP_READ_RUN => {
                let request = ReadRunRequest::decode(req).expect("well-formed request");
                assert_eq!(buf.len(), request.reply_len(), "reply buffer fits the run");
                let outcome = self.read_run_outcome.unwrap_or(BlockOutcome::Completed);
                buf[0] = outcome.to_byte();
                if outcome.is_completed() {
                    for (k, out) in buf[1..].chunks_exact_mut(SECTOR_SIZE).enumerate() {
                        let lba = request.sector + k as u64;
                        out.copy_from_slice(&medium.get(&lba).copied().unwrap_or([0; SECTOR_SIZE]));
                    }
                } else {
                    // Garbage a careless client might copy out.
                    buf[1..].fill(0xA5);
                }
            }
            _ => {}
        }
        Call::Replied(buf.len())
    }
    fn now_ticks(&mut self) -> u64 {
        self.now
    }
    fn kernel_read(&mut self, _: u64, _: &mut [u8; SECTOR_SIZE]) -> bool {
        self.kernel_flush()
    }
    fn kernel_write(&mut self, _: u64, _: &[u8; SECTOR_SIZE]) -> bool {
        self.kernel_flush()
    }
    /// Every kernel-path call lands here and is counted.
    fn kernel_flush(&mut self) -> bool {
        self.kernel_calls += 1;
        true
    }
    fn log(&mut self, line: &str) {
        self.logs.push(line.into());
    }
}

pub(super) fn cellos_mbr() -> [u8; SECTOR_SIZE] {
    let mut mbr = [0u8; SECTOR_SIZE];
    mbr[446 + 16 + 4] = 0x7F;
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    mbr
}

pub(super) fn two_disks() -> (Fake, Router) {
    let mut fake = Fake {
        routes: alloc::vec![route(1, AHCI, 7, 70), route(1, NVME, 9, 90)],
        ..Fake::default()
    };
    fake.media.entry(9).or_default().insert(0, cellos_mbr());
    let mut router = Router::new();
    let found = discover(&mut router, &mut fake);
    assert_eq!(found.primary, DeviceRef::Device(1));
    assert_eq!(
        found.secondaries,
        [(DeviceRef::Device(0), String::from("pci-00-1f-2"))]
    );
    (fake, router)
}

#[test]
fn discovery_logs_routes_and_the_cellos_primary() {
    let (fake, _) = two_disks();
    assert!(fake.logged("[vfs] block route 0 pci-00-1f-2 tid=7"));
    assert!(fake.logged("[vfs] block route 1 pci-03-00-0 tid=9"));
    assert!(fake.logged("[vfs] primary disk pci-03-00-0 (cellos mbr)"));
    assert_eq!(fake.kernel_calls, 0);
}

#[test]
fn zero_routes_leave_only_the_kernel_disk() {
    let mut fake = Fake::default();
    let mut router = Router::new();
    let found = discover(&mut router, &mut fake);
    assert_eq!(found.primary, DeviceRef::Kernel);
    assert!(found.secondaries.is_empty());
    assert!(fake.logged("[vfs] primary disk kernel (no block routes)"));
    assert!(router.read(&mut fake, DeviceRef::Kernel, 5, &mut [0; SECTOR_SIZE]));
    assert_eq!(
        router.flush(&mut fake, DeviceRef::Kernel),
        BlockOutcome::Completed
    );
    assert_eq!(fake.kernel_calls, 2);
}

#[test]
fn provider_death_on_one_device_leaves_the_other_untouched() {
    let (mut fake, mut router) = two_disks();
    let (a, b) = (DeviceRef::Device(0), DeviceRef::Device(1));
    fake.dies_after_send = Some(7);
    assert_eq!(
        router.write(&mut fake, a, 2048, &[1; SECTOR_SIZE]),
        BlockOutcome::Indeterminate
    );
    assert!(fake.logged("pci-00-1f-2: driver died before replying; outcome indeterminate"));
    assert!(fake.logged("pci-00-1f-2: write lba=2048 indeterminate; not replayed"));

    fake.sent.clear();
    assert_eq!(
        router.write(&mut fake, b, 2048, &[2; SECTOR_SIZE]),
        BlockOutcome::Completed
    );
    assert_eq!(fake.sent, [9]);
    assert_eq!(router.devices()[1].provider().map(|p| p.tid), Some(9));

    // Sticky: the dead device's requests are refused, never sent to the kernel.
    assert_eq!(
        router.write(&mut fake, a, 2048, &[3; SECTOR_SIZE]),
        BlockOutcome::NotIssued
    );
    assert_eq!(router.flush(&mut fake, a), BlockOutcome::NotIssued);
    assert!(!router.read(&mut fake, a, 2048, &mut [0; SECTOR_SIZE]));
    assert!(fake.logged("[blk-router] pci-00-1f-2: no live provider; not issued"));
    assert_eq!(fake.kernel_calls, 0);

    // The restarted driver rebinds the same slot; only device A re-resolves.
    fake.routes[0] = ViBlockRouteV1 {
        generation: 2,
        ..route(1, AHCI, 11, 71)
    };
    assert_eq!(
        router.write(&mut fake, a, 2048, &[4; SECTOR_SIZE]),
        BlockOutcome::Completed
    );
    assert!(fake.logged("[vfs] block device pci-00-1f-2 provider tid=11 cell=71 gen=2"));
    assert!(!fake
        .logs
        .iter()
        .any(|l| l.contains("[vfs] block device pci-03-00-0")));
    assert_eq!(fake.media[&11][&2048], [4; SECTOR_SIZE]);
    assert_eq!(fake.media[&9][&2048], [2; SECTOR_SIZE]);
}

#[test]
fn death_notification_is_never_read_as_data_and_refused_send_is_not_issued() {
    let (mut fake, mut router) = two_disks();
    fake.dies_after_send = Some(9);
    let mut buf = [0xAA; SECTOR_SIZE];
    assert!(!router.read(&mut fake, DeviceRef::Device(1), 0, &mut buf));
    assert_eq!(buf, [0xAA; SECTOR_SIZE]);
    // A provider that died between requests: the cached tid's send is refused.
    let a = DeviceRef::Device(0);
    fake.kill(7);
    assert_eq!(
        router.write(&mut fake, a, 1, &[0; SECTOR_SIZE]),
        BlockOutcome::NotIssued
    );
    assert_eq!(router.devices()[0].provider(), None);
    assert_eq!(fake.kernel_calls, 0);
}

#[test]
fn route_without_provider_at_discovery_is_sticky() {
    let mut fake = Fake {
        routes: alloc::vec![route(2, 0x1000_1000, 0, 0)],
        ..Fake::default()
    };
    let mut router = Router::new();
    let found = discover(&mut router, &mut fake);
    assert!(fake.logged("[vfs] block route 0 mmio-10001000 tid=0"));
    assert!(fake.logged("[vfs] primary disk mmio-10001000 (lowest key)"));
    assert_eq!(
        router.write(&mut fake, found.primary, 0, &[0; SECTOR_SIZE]),
        BlockOutcome::NotIssued
    );
    assert_eq!(fake.kernel_calls, 0);
    assert!(fake.sent.is_empty());
}

#[test]
fn flush_reaches_only_its_own_device() {
    let (mut fake, mut router) = two_disks();
    fake.sent.clear();
    assert_eq!(
        router.flush(&mut fake, DeviceRef::Device(0)),
        BlockOutcome::Completed
    );
    assert_eq!(fake.sent, [7]);
    assert_eq!(
        router.flush(&mut fake, DeviceRef::Device(1)),
        BlockOutcome::Completed
    );
    assert_eq!(fake.sent, [7, 9]);
}

#[test]
fn a_frozen_provider_costs_one_bound_per_window_and_a_replacement_clears_it() {
    let (mut fake, mut router) = two_disks();
    let (a, b) = (DeviceRef::Device(0), DeviceRef::Device(1));
    // Taken but never answered: the write may have run, so it is never replayed.
    fake.frozen = Some((7, true));
    assert_eq!(
        router.write(&mut fake, a, 4, &[1; SECTOR_SIZE]),
        BlockOutcome::Indeterminate
    );
    assert!(
        fake.logged("pci-00-1f-2: driver tid=7 did not reply within 3500 ticks; device stalled")
    );

    // Stalled: refused at once, nothing else reaches the frozen driver.
    fake.sent.clear();
    assert!(!router.read(&mut fake, a, 4, &mut [0; SECTOR_SIZE]));
    assert_eq!(router.flush(&mut fake, a), BlockOutcome::NotIssued);
    assert!(fake.sent.is_empty());
    // The other device keeps serving.
    assert_eq!(
        router.write(&mut fake, b, 4, &[2; SECTOR_SIZE]),
        BlockOutcome::Completed
    );

    // Within the window the same provider stays refused; after it, one request
    // probes again, and a driver that answers clears the stall.
    fake.sent.clear();
    fake.now += super::STALL_RETRY_TICKS - 1;
    assert_eq!(router.flush(&mut fake, a), BlockOutcome::NotIssued);
    assert!(fake.sent.is_empty());
    fake.now += 1;
    fake.frozen = None;
    assert_eq!(router.flush(&mut fake, a), BlockOutcome::Completed);
    assert_eq!(fake.sent, [7]);
    fake.frozen = Some((7, true));
    assert_eq!(router.flush(&mut fake, a), BlockOutcome::Indeterminate);

    // A replacement provider on the same slot clears the stall at once.
    fake.routes[0] = ViBlockRouteV1 {
        generation: 2,
        ..route(1, AHCI, 11, 71)
    };
    assert_eq!(
        router.write(&mut fake, a, 4, &[3; SECTOR_SIZE]),
        BlockOutcome::Completed
    );
    assert_eq!(fake.media[&11][&4], [3; SECTOR_SIZE]);
}

#[test]
fn a_request_the_frozen_provider_never_took_was_not_issued() {
    let (mut fake, mut router) = two_disks();
    fake.frozen = Some((9, false));
    assert_eq!(
        router.write(&mut fake, DeviceRef::Device(1), 4, &[1; SECTOR_SIZE]),
        BlockOutcome::NotIssued
    );
}
