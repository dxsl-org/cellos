//! Host tests for the DMA export lease state machine (ADR-0033 CP1 §7).
//!
//! A fake [`Platform`] stands in for the scheduler, grant tables, quota and the
//! VT-d remapper (whose drain acknowledgement can be withheld); the owner-range
//! lock and the quarantine are the real `memory::pin` registry, so frame custody
//! is exercised end to end. Each test uses its own physical range and holds
//! `TEST_STATE_LOCK` (the pin tests do too) so the shared
//! registry never sees two tests' locks overlap.

use super::*;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

const CLIENT: usize = 70;
const DRIVER: usize = 80;
const STRANGER: usize = 90;
const BDF: u32 = 0x0010;
const OWNER: Incarnation = Incarnation {
    cell: 7,
    generation: 1,
};
const HOLDER: Incarnation = Incarnation {
    cell: 8,
    generation: 1,
};
const XE: usize = abi::syscall::service::XE_GPU as usize;

#[derive(Default)]
struct Fake {
    bindings: BTreeMap<usize, Incarnation>,
    enabled: bool,
    providers: BTreeMap<u16, Provider>,
    owned: Vec<(usize, usize, usize)>,
    pcie: Vec<usize>,
    bdf_owner: BTreeMap<u32, usize>,
    translating: bool,
    quota_left: usize,
    quota_used: usize,
    next_map: Option<MapOutcome>,
    ack: bool,
    /// Live window leaves: `(holder, iova) → (phys, len, writable)`.
    leaves: BTreeMap<(usize, u64), (u64, usize, bool)>,
    maps: usize,
    unmaps: usize,
    bme: Vec<u32>,
    released: Vec<(usize, usize)>,
}

impl Fake {
    /// A client that owns `[base, base + len)` and a live XE_GPU driver owning BDF.
    fn new(base: usize, len: usize) -> Self {
        let mut f = Fake {
            enabled: true,
            translating: true,
            quota_left: usize::MAX,
            ack: true,
            ..Fake::default()
        };
        f.bindings.insert(CLIENT, OWNER);
        f.bindings.insert(DRIVER, HOLDER);
        f.bindings.insert(
            STRANGER,
            Incarnation {
                cell: 9,
                generation: 1,
            },
        );
        f.providers.insert(
            abi::syscall::service::XE_GPU,
            Provider {
                tid: DRIVER,
                who: HOLDER,
                pcie_driver: true,
            },
        );
        f.owned.push((CLIENT, base, len));
        f.pcie.push(DRIVER);
        f.bdf_owner.insert(BDF, DRIVER);
        f
    }

    /// Count of every observable effect a refusal must not have.
    fn effects(&self) -> (usize, usize, usize, usize) {
        (self.maps, self.unmaps, self.bme.len(), self.quota_used)
    }
}

impl Platform for Fake {
    fn binding(&self, tid: usize) -> Option<Incarnation> {
        self.bindings.get(&tid).copied()
    }
    fn incarnation_live(&self, who: Incarnation) -> bool {
        self.bindings.values().any(|b| *b == who)
    }
    fn exports_enabled(&self) -> bool {
        self.enabled
    }
    fn provider(&self, service: u16) -> Option<Provider> {
        self.providers.get(&service).copied()
    }
    fn lock_owned_range(&mut self, req: &RangeLock) -> Result<(), SyscallError> {
        let owned = self.owned.iter().any(|&(tid, base, len)| {
            tid == req.tid && base <= req.base && req.base + req.len <= base + len
        });
        if !owned || self.bindings.get(&req.tid) != Some(&req.owner) {
            return Err(SyscallError::PermissionDenied);
        }
        crate::memory::pin::lock_dma_export(
            req.base,
            req.len,
            req.tid,
            req.holder_cell,
            req.export_id,
        )
        .map_err(|_| SyscallError::OutOfMemory)
    }
    fn unlock_range(&mut self, export_id: u64) -> usize {
        let mut pages = 0;
        for r in crate::memory::pin::unlock_dma_export(export_id) {
            pages += r.pages();
            self.released.push((r.base(), r.pages()));
        }
        pages
    }
    fn has_pcie_driver(&self, tid: usize) -> bool {
        self.pcie.contains(&tid)
    }
    fn owns_bdf(&self, tid: usize, bdf: u32) -> bool {
        self.bdf_owner.get(&bdf) == Some(&tid)
    }
    fn translating(&self, _bdf: u32) -> bool {
        self.translating
    }
    fn reserve_quota(&mut self, _holder: Incarnation, bytes: usize) -> bool {
        if bytes > self.quota_left {
            return false;
        }
        self.quota_left -= bytes;
        self.quota_used += bytes;
        true
    }
    fn release_quota(&mut self, _holder: Incarnation, bytes: usize) {
        self.quota_left = self.quota_left.saturating_add(bytes);
        self.quota_used -= bytes;
    }
    fn map_window(&mut self, w: &WindowMap) -> MapOutcome {
        self.maps += 1;
        let outcome = self.next_map.take().unwrap_or(MapOutcome::Mapped);
        if outcome != MapOutcome::Rejected {
            self.leaves
                .insert((w.holder_tid, w.iova), (w.phys, w.len, w.writable));
        }
        outcome
    }
    fn unmap_window(&mut self, holder_tid: usize, iova: u64, _len: usize) -> bool {
        self.unmaps += 1;
        // Leaves are zeroed either way; only the drain may go unacknowledged.
        self.leaves.remove(&(holder_tid, iova));
        self.ack
    }
    fn enable_bus_master(&mut self, bdf: u32) {
        self.bme.push(bdf);
    }
}

fn locked(base: usize) -> bool {
    crate::memory::pin::holder_of(base, PAGE).is_some()
}

#[test]
fn export_map_revoke_happy_path() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_0000_0000;
    let mut f = Fake::new(BASE, 0x4000);
    let mut t = ExportTable::new();

    let id = t
        .export(&mut f, CLIENT, BASE, 0x2000, XE, 0)
        .expect("export");
    assert!(id >= 1);
    assert!(locked(BASE), "a Live lease locks the owner range");
    assert_eq!(f.effects(), (0, 0, 0, 0), "437 maps nothing");

    let rec = t.map(&mut f, DRIVER, id, BDF as usize).expect("map");
    assert_eq!(rec.version, 1);
    assert_eq!(rec.export_id, id);
    assert_eq!((rec.owner_cell_id, rec.owner_generation), (7, 1));
    assert_eq!(rec.flags, 0);
    assert_eq!(rec.len, 0x2000);
    assert!((WINDOW_BASE..WINDOW_END).contains(&rec.iova));
    assert_eq!(rec.iova % 4096, 0);
    assert_eq!(
        f.leaves.get(&(DRIVER, rec.iova)),
        Some(&(BASE as u64, 0x2000, false)),
        "R-only, non-identity"
    );
    assert_eq!(f.bme, [BDF]);
    assert!(f.quota_used > 0, "SLPT pages charged to the holder");

    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Ok(()));
    assert!(f.leaves.is_empty());
    assert_eq!(f.quota_used, 0);
    assert!(!locked(BASE));
    assert_eq!(t.state(id), None);
    assert_eq!(
        t.revoke_by(&mut f, CLIENT, id),
        Err(SyscallError::FileNotFound)
    );
}

#[test]
fn device_write_flag_maps_rw_and_repeat_map_is_idempotent() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_1000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    f.bdf_owner.insert(0x0018, DRIVER);
    let mut t = ExportTable::new();
    let id = t
        .export(
            &mut f,
            CLIENT,
            BASE,
            0x1000,
            XE,
            EXPORT_DEVICE_WRITE as usize,
        )
        .unwrap();
    let first = t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    assert_eq!(first.flags, EXPORT_DEVICE_WRITE);
    assert_eq!(f.leaves[&(DRIVER, first.iova)].2, true);
    let again = t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    assert_eq!(again, first);
    assert_eq!(f.maps, 1, "repeat 438 does not remap");
    // Another function of the same holder joins the same window.
    let other = t.map(&mut f, DRIVER, id, 0x0018).unwrap();
    assert_eq!(other, first);
    assert_eq!(f.bme, [BDF, 0x0018]);
    assert_eq!(
        t.revoke_by(&mut f, DRIVER, id),
        Ok(()),
        "the holder may revoke too"
    );
}

#[test]
fn owner_death_with_ack_frees_frames_normally() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_2000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    f.bindings.remove(&CLIENT);
    t.owner_died(&mut f, CLIENT);
    assert_eq!(t.state(id), None);
    assert!(
        f.leaves.is_empty(),
        "window cleared before any frame release"
    );
    // The grant reaper then finds no protector and frees the frames itself.
    assert_eq!(crate::memory::pin::test_withhold(BASE, 1), "free");
}

#[test]
fn owner_death_without_ack_quarantines_until_retry() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_3000_0000;
    let mut f = Fake::new(BASE, 0x2000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x2000, XE, 0).unwrap();
    t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    f.ack = false;
    f.bindings.remove(&CLIENT);
    t.owner_died(&mut f, CLIENT);
    assert_eq!(t.state(id).map(|s| s.0), Some(LeaseState::Revoking));
    assert!(locked(BASE), "unacknowledged: range stays locked");
    // Grant reaper: frames withheld under the export's key, never freed.
    assert_eq!(crate::memory::pin::test_withhold(BASE, 2), "withheld");
    assert_eq!(crate::memory::pin::dma_export_quarantined_pages(id), 2);
    // A retry that is still unacknowledged releases nothing.
    t.retry(&mut f);
    assert!(f.released.is_empty());
    // Acknowledged retry: lease dropped, quarantine released to the allocator.
    f.ack = true;
    t.retry(&mut f);
    assert_eq!(t.state(id), None);
    assert_eq!(f.released, [(BASE, 2)]);
    assert_eq!(crate::memory::pin::dma_export_quarantined_pages(id), 0);
    assert!(!locked(BASE));
}

#[test]
fn unexport_without_ack_is_try_again_and_idempotent() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_4000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    f.ack = false;
    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Err(SyscallError::TryAgain));
    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Err(SyscallError::TryAgain));
    assert!(locked(BASE));
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::FileNotFound),
        "a Revoking lease cannot be mapped"
    );
    f.ack = true;
    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Ok(()));
    assert!(!locked(BASE));
}

#[test]
fn holder_death_drops_leases_only_after_domain_teardown() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_5000_0000;
    let mut f = Fake::new(BASE, 0x2000);
    let mut t = ExportTable::new();
    let a = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    let b = t
        .export(&mut f, CLIENT, BASE + 0x1000, 0x1000, XE, 0)
        .unwrap();
    t.map(&mut f, DRIVER, a, BDF as usize).unwrap();
    // The holder dies; until its domain teardown is acknowledged nothing drops.
    f.bindings.remove(&DRIVER);
    f.providers.clear();
    assert!(t.state(a).is_some() && t.state(b).is_some());
    assert!(locked(BASE));
    t.holder_domain_revoked(&mut f, DRIVER);
    assert_eq!(t.state(a), None, "mapped lease dropped with the domain");
    assert_eq!(
        t.state(b),
        None,
        "unmapped lease of the dead incarnation dropped"
    );
    assert_eq!(f.quota_used, 0);
    assert!(
        !locked(BASE) && !locked(BASE + 0x1000),
        "owner range reusable"
    );
    assert_eq!(
        f.unmaps, 0,
        "the whole-domain teardown already drained the IOTLB"
    );
}

#[test]
fn holder_losing_its_domain_while_alive_keeps_lease_unmapped() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_6000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    let first = t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    // CapRevoke(PCIE_DRIVER): teardown_dma_authority on a live holder.
    t.holder_domain_revoked(&mut f, DRIVER);
    assert_eq!(t.state(id), Some((LeaseState::Live, None)));
    assert_eq!(f.quota_used, 0);
    // Remapping allocates a fresh window in the new domain.
    let again = t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    assert_eq!(
        again.iova, first.iova,
        "fresh window region starts at the base again"
    );
    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Ok(()));
}

#[test]
fn stale_holder_generation_is_refused() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_7000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    // The provider restarts: same Cell id, next generation, new task.
    const NEW_DRIVER: usize = 81;
    let restarted = Incarnation {
        cell: HOLDER.cell,
        generation: 2,
    };
    f.bindings.insert(NEW_DRIVER, restarted);
    f.pcie.push(NEW_DRIVER);
    f.bdf_owner.insert(BDF, NEW_DRIVER);
    assert_eq!(
        t.map(&mut f, NEW_DRIVER, id, BDF as usize),
        Err(SyscallError::PermissionDenied)
    );
    assert_eq!(f.effects(), (0, 0, 0, 0));
    assert_eq!(
        t.revoke_by(&mut f, NEW_DRIVER, id),
        Err(SyscallError::FileNotFound)
    );
    assert_eq!(
        t.revoke_by(&mut f, STRANGER, id),
        Err(SyscallError::FileNotFound)
    );
    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Ok(()));
}

#[test]
fn export_ids_are_never_reused() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_8000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let a = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    t.revoke_by(&mut f, CLIENT, a).unwrap();
    let b = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    assert!(b > a);
    t.revoke_by(&mut f, CLIENT, b).unwrap();
}

#[test]
fn export_refusals_have_no_effect() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_9000_0000;
    let mut f = Fake::new(BASE, 0x2000);
    let mut t = ExportTable::new();
    let cases: [(usize, usize, usize, usize, SyscallError); 9] = [
        (BASE + 1, 0x1000, XE, 0, SyscallError::InvalidInput),
        (BASE, 0, XE, 0, SyscallError::InvalidInput),
        (BASE, 0x1001, XE, 0, SyscallError::InvalidInput),
        (
            BASE,
            (EXPORT_MAX_BYTES as usize) + 4096,
            XE,
            0,
            SyscallError::InvalidInput,
        ),
        (BASE, 0x1000, XE | 0x1_0000, 0, SyscallError::InvalidInput),
        (BASE, 0x1000, XE, 2, SyscallError::InvalidInput),
        (
            BASE,
            0x1000,
            abi::syscall::service::NET as usize,
            0,
            SyscallError::PermissionDenied,
        ),
        (BASE + 0x2000, 0x1000, XE, 0, SyscallError::PermissionDenied),
        (BASE, 0x3000, XE, 0, SyscallError::PermissionDenied),
    ];
    for (va, len, service, flags, want) in cases {
        assert_eq!(
            t.export(&mut f, CLIENT, va, len, service, flags),
            Err(want),
            "{va:#x}+{len:#x}"
        );
    }
    // Caller as holder: the driver exporting to its own service.
    f.owned.push((DRIVER, BASE, 0x1000));
    assert_eq!(
        t.export(&mut f, DRIVER, BASE, 0x1000, XE, 0),
        Err(SyscallError::PermissionDenied)
    );
    // Received memory is not owned memory: STRANGER does not own BASE.
    assert_eq!(
        t.export(&mut f, STRANGER, BASE, 0x1000, XE, 0),
        Err(SyscallError::PermissionDenied)
    );
    // Holder without PcieDriverCap.
    f.providers
        .get_mut(&abi::syscall::service::XE_GPU)
        .unwrap()
        .pcie_driver = false;
    assert_eq!(
        t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0),
        Err(SyscallError::PermissionDenied)
    );
    // No live provider.
    f.providers.clear();
    assert_eq!(
        t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0),
        Err(SyscallError::FileNotFound)
    );
    // No translating remapper: never an identity fallback.
    f.enabled = false;
    assert_eq!(
        t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0),
        Err(SyscallError::NotSupported)
    );
    assert!(t.leases.is_empty());
    assert!(!locked(BASE), "no refusal left a lock");
    assert_eq!(f.effects(), (0, 0, 0, 0));
}

#[test]
fn per_owner_lease_limit_is_out_of_memory() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x71_0000_0000;
    let mut f = Fake::new(BASE, (MAX_LEASES_PER_OWNER + 1) * PAGE);
    let mut t = ExportTable::new();
    let mut ids = Vec::new();
    for i in 0..MAX_LEASES_PER_OWNER {
        ids.push(
            t.export(&mut f, CLIENT, BASE + i * PAGE, PAGE, XE, 0)
                .unwrap(),
        );
    }
    let last = BASE + MAX_LEASES_PER_OWNER * PAGE;
    assert_eq!(
        t.export(&mut f, CLIENT, last, PAGE, XE, 0),
        Err(SyscallError::OutOfMemory)
    );
    assert!(!locked(last));
    for id in ids {
        t.revoke_by(&mut f, CLIENT, id).unwrap();
    }
}

#[test]
fn map_refusals_have_no_effect() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_a000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    assert_eq!(
        t.map(&mut f, DRIVER, id, 0x1_0000),
        Err(SyscallError::InvalidInput)
    );
    assert_eq!(
        t.map(&mut f, STRANGER, id, BDF as usize),
        Err(SyscallError::PermissionDenied)
    );
    assert_eq!(
        t.map(&mut f, DRIVER, id, 0x0020),
        Err(SyscallError::PermissionDenied),
        "BDF not owned"
    );
    assert_eq!(
        t.map(&mut f, DRIVER, id + 1000, BDF as usize),
        Err(SyscallError::FileNotFound)
    );
    f.translating = false;
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::NotSupported)
    );
    f.translating = true;
    f.quota_left = 0;
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::OutOfMemory)
    );
    f.quota_left = usize::MAX;
    f.next_map = Some(MapOutcome::Rejected);
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::OutOfMemory)
    );
    assert_eq!(
        f.effects(),
        (1, 0, 0, 0),
        "only the rejected publication was attempted"
    );
    assert_eq!(t.state(id), Some((LeaseState::Live, None)));
    // The rejected window went back: the next map gets the region base.
    let rec = t.map(&mut f, DRIVER, id, BDF as usize).unwrap();
    assert_eq!(rec.iova, WINDOW_BASE);
    f.pcie.clear();
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::PermissionDenied)
    );
    t.revoke_by(&mut f, CLIENT, id).unwrap();
}

#[test]
fn unconfirmed_map_is_unknown_until_revoke() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x70_b000_0000;
    let mut f = Fake::new(BASE, 0x1000);
    let mut t = ExportTable::new();
    let id = t.export(&mut f, CLIENT, BASE, 0x1000, XE, 0).unwrap();
    f.next_map = Some(MapOutcome::Unconfirmed);
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::Unknown)
    );
    assert!(
        f.bme.is_empty(),
        "no bus mastering on an unconfirmed window"
    );
    assert_eq!(
        t.map(&mut f, DRIVER, id, BDF as usize),
        Err(SyscallError::Unknown)
    );
    assert_eq!(t.revoke_by(&mut f, CLIENT, id), Ok(()));
    assert!(f.leaves.is_empty() && f.quota_used == 0);
}

#[test]
fn windows_are_disjoint_aligned_and_reused_only_after_release() {
    let mut w = Windows::new();
    let a = w.allocate(0x3000).unwrap();
    let b = w.allocate(4 << 20).unwrap();
    let c = w.allocate(0x1000).unwrap();
    assert_eq!(a, WINDOW_BASE);
    assert_eq!(b % (2 << 20), 0, "large windows are 2 MiB aligned");
    assert!(b >= a + 0x3000);
    assert_eq!(c, a + 0x3000, "first fit fills the alignment gap");
    w.release(a, 0x3000);
    assert_eq!(w.allocate(0x2000), Some(a), "released space is reused");
    assert_eq!(
        w.allocate(WINDOW_END - WINDOW_BASE),
        None,
        "exhaustion is refused"
    );
    // Everything back: one region again.
    w.release(a, 0x2000);
    w.release(b, 4 << 20);
    w.release(c, 0x1000);
    assert_eq!(w.free.len(), 1);
    assert_eq!(w.allocate(WINDOW_END - WINDOW_BASE), Some(WINDOW_BASE));
}

#[test]
fn export_services_are_an_explicit_table() {
    assert_eq!(DMA_EXPORT_SERVICES, [abi::syscall::service::XE_GPU]);
    assert!(holder_service_allowed(abi::syscall::service::XE_GPU));
    assert!(!holder_service_allowed(abi::syscall::service::GPU_DRIVER));
    assert_eq!(
        holder_service_allowed(TEST_EXPORT_SERVICE),
        cfg!(feature = "test-hooks")
    );
}
