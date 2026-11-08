//! Driver Cell registration — tracks the per-device block routes and which
//! cells own the NIC, GPU, and USB HID producer roles.
//!
//! A Tier-1 block Driver Cell binds the route of the device it owns with
//! `sys_register_block_device` (416); `sys_register_nic_driver`,
//! `sys_register_gpu_driver`, and `sys_register_usb_hid_producer` record the
//! caller's TID here. These registrations support service routing and interrupt
//! ownership checks until the owning Cell exits.
//!
//! A role TID of `0` means "no driver cell registered". NIC has no kernel
//! fallback and is always a Driver Cell. GPU has no kernel fallback; compositor
//! refuses to init until a GPU Cell registers. The USB HID producer role exists
//! so a USB host cell can authorize raw HID events into the input service
//! without overwriting the singleton NIC owner.

use crate::sync::Spinlock;
use api::block_route::{
    DeviceKey, ViBlockRouteV1, BLOCK_DEVICE_PCI, BLOCK_ROUTE_VERSION, FLAG_PROVIDER_LIVE,
    MAX_BLOCK_ROUTES,
};
use core::sync::atomic::{AtomicUsize, Ordering};

/// A live block route provider: `(tid, cell_id, generation)`.
type BlockProvider = (usize, u64, u64);

/// One storage device's route: its key is fixed at first bind; only the
/// provider changes.
#[derive(Clone, Copy)]
struct BlockRouteSlot {
    key: DeviceKey,
    provider: Option<BlockProvider>,
}

/// Per-device block routes, append-only for the boot: a slot index never
/// changes owner device and a key is never cleared. Lock order
/// `SCHEDULER -> ROLE_PUBLICATION -> BLOCK_ROUTES` (leaf); no allocation,
/// because exit teardown may run in ISR context.
static BLOCK_ROUTES: Spinlock<[Option<BlockRouteSlot>; MAX_BLOCK_ROUTES]> =
    Spinlock::new([None; MAX_BLOCK_ROUTES]);

/// Log name of a block device: `pci-BB-DD-F` or `mmio-<base hex>`.
struct BlockDeviceName(DeviceKey);

impl core::fmt::Display for BlockDeviceName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let id = self.0.id;
        if self.0.kind == BLOCK_DEVICE_PCI {
            write!(
                f,
                "pci-{:02x}-{:02x}-{:x}",
                (id >> 8) & 0xff,
                (id >> 3) & 0x1f,
                id & 7
            )
        } else {
            write!(f, "mmio-{:x}", id)
        }
    }
}

#[derive(Clone, Copy)]
struct NicDriverState {
    owner_tid: usize,
    virtio_irq: u32,
}

impl NicDriverState {
    const fn empty() -> Self {
        Self {
            owner_tid: 0,
            virtio_irq: 0,
        }
    }
}

/// Registered NIC owner paired with the proven VirtIO IRQ, if any.
static NIC_DRIVER_STATE: Spinlock<NicDriverState> = Spinlock::new(NicDriverState::empty());

/// TID of the registered GPU Driver Cell (0 = none; no kernel GPU fallback).
pub static GPU_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// TID of the registered USB HID event producer (0 = none).
///
/// Distinct from `NIC_DRIVER_STATE` on purpose: a USB host controller cell
/// (`/bin/xhci` on a PC, `/bin/dwc2-usb` on RPi3) publishes this role to
/// authorize raw HID events into the input service **without** taking over the
/// singleton NIC route. The input service's producer gate resolves
/// `service::USB_HID_PRODUCER` to this TID.
pub static USB_HID_PRODUCER_CELL: AtomicUsize = AtomicUsize::new(0);
/// Serializes role state with its matching service-registry publication.
static ROLE_PUBLICATION: Spinlock<()> = Spinlock::new(());

const VIRTIO_DEVICE_ID_OFFSET: usize = 0x8;
const VIRTIO_DEVICE_ID_NETWORK: u32 = 1;

fn registered_virtio_irq_for_owner(tid: usize) -> Option<u32> {
    crate::task::drivers::virtio_common::virtio_slots()
        .find(|slot| {
            if crate::resource_registry::lookup_mmio_owner(slot.base) != Some(tid) {
                return false;
            }
            // SAFETY: this runs only in NIC registration context, never from ISR.
            // `virtio_slots()` yields the kernel's enumerated, identity-mapped
            // VirtIO MMIO bases, and the ownership check above proves this TID
            // already claimed the slot via `RequestMmio` before we inspect the
            // read-only `device_id` register at offset 0x8.
            let device_id = unsafe {
                core::ptr::read_volatile((slot.base + VIRTIO_DEVICE_ID_OFFSET) as *const u32)
            };
            device_id == VIRTIO_DEVICE_ID_NETWORK
        })
        .map(|slot| slot.irq)
}

/// Publish an authorized driver's role and matching service route.
///
/// The syscall caller holds `SCHEDULER`, establishing lock order
/// `SCHEDULER -> ROLE_PUBLICATION -> service/role leaves`. Task teardown uses
/// the same order, so authorization, liveness, and publication cannot race.
pub(crate) fn publish_role_or_rollback(
    tid: usize,
    publish_role: impl FnOnce(usize),
    publish_service: impl FnOnce() -> bool,
    rollback_role: impl FnOnce(usize),
) -> bool {
    let _publication = ROLE_PUBLICATION.lock();
    publish_role(tid);
    if publish_service() {
        true
    } else {
        rollback_role(tid);
        false
    }
}

/// Bind `key`'s route to `provider` and return its slot index.
///
/// The caller holds `SCHEDULER` and has proven that `provider` owns `key`, so
/// publication shares the role transaction with exit teardown. A key already
/// in the table keeps its slot: an empty provider or the same tid is (re)bound,
/// a different live provider is refused. A new key takes the next free slot.
/// `None` means refused (live foreign provider or full table) and leaves the
/// table unchanged.
///
/// Logged at `warn!` for the same reason as `set_input_cell`: Driver Cells are
/// spawned by init AFTER the kernel drops its log level to Warn, and the
/// registration line is the marker the block-driver integration tests assert.
pub(crate) fn publish_block_route(key: DeviceKey, provider: BlockProvider) -> Option<usize> {
    enum Bind {
        Bound(usize),
        Held(usize, usize),
        Full,
    }
    let _publication = ROLE_PUBLICATION.lock();
    let tid = provider.0;
    let bound = Some(BlockRouteSlot {
        key,
        provider: Some(provider),
    });
    let outcome = {
        let mut routes = BLOCK_ROUTES.lock();
        match routes
            .iter()
            .position(|slot| slot.is_some_and(|slot| slot.key == key))
        {
            Some(index) => match routes[index].and_then(|slot| slot.provider) {
                Some((holder, _, _)) if holder != tid => Bind::Held(index, holder),
                _ => {
                    routes[index] = bound;
                    Bind::Bound(index)
                }
            },
            None => match routes.iter().position(Option::is_none) {
                Some(index) => {
                    routes[index] = bound;
                    Bind::Bound(index)
                }
                None => Bind::Full,
            },
        }
    };
    let name = BlockDeviceName(key);
    match outcome {
        Bind::Bound(index) => {
            log::warn!(
                "[driver_cell] block driver registered: tid={} device={} route={}",
                tid,
                name,
                index
            );
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::milestone("block-ready", tid);
            Some(index)
        }
        Bind::Held(index, holder) => {
            log::warn!(
                "[driver_cell] block route {} device {} held by live tid={}; tid={} refused",
                index,
                name,
                holder,
                tid
            );
            None
        }
        Bind::Full => {
            log::warn!(
                "[driver_cell] block route table full; device {} refused",
                name
            );
            None
        }
    }
}

/// Snapshot of route slot `index` as a V1 record; `None` when no device has
/// bound that slot (end of enumeration, or `index >= MAX_BLOCK_ROUTES`).
pub(crate) fn block_route_record(index: usize) -> Option<ViBlockRouteV1> {
    let slot = (*BLOCK_ROUTES.lock().get(index)?)?;
    let (flags, (provider_tid, cell_id, generation)) = match slot.provider {
        Some((tid, cell_id, generation)) => (FLAG_PROVIDER_LIVE, (tid as u64, cell_id, generation)),
        None => (0, (0, 0, 0)),
    };
    Some(ViBlockRouteV1 {
        version: BLOCK_ROUTE_VERSION,
        kind: slot.key.kind,
        flags,
        id: slot.key.id,
        provider_tid,
        cell_id,
        generation,
    })
}

/// Clear the provider of every block route naming exactly `tid`; keys stay.
fn clear_block_route_provider(tid: usize) {
    for slot in BLOCK_ROUTES.lock().iter_mut().flatten() {
        if slot.provider.is_some_and(|(holder, _, _)| holder == tid) {
            slot.provider = None;
        }
    }
}

/// Empty the route table; host tests only, under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn reset_block_routes() {
    *BLOCK_ROUTES.lock() = [None; MAX_BLOCK_ROUTES];
}

/// Record `tid` as the active NIC driver.  Overwrites any previous registration.
/// `warn!` — see `publish_block_route`.
pub fn register_nic_driver(tid: usize) {
    let proven_irq = registered_virtio_irq_for_owner(tid).unwrap_or(0);

    // Fail closed during replacement: an IRQ only becomes network-owned again
    // after the replacement publishes both the owner and its proven IRQ together.
    *NIC_DRIVER_STATE.lock() = NicDriverState {
        owner_tid: tid,
        virtio_irq: proven_irq,
    };
    log::warn!("[driver_cell] NIC driver registered: tid={}", tid);
}

/// Clear the NIC driver registration (called on cell exit/kill).
pub fn deregister_nic_driver(tid: usize) {
    let mut state = NIC_DRIVER_STATE.lock();
    if state.owner_tid == tid {
        *state = NicDriverState::empty();
    }
}

/// Returns true only for the cached VirtIO IRQ proven to belong to the active NIC,
/// or (on x86_64) a PCIe MSI vector allocated to the registered NIC driver.
pub fn owns_registered_nic_irq(irq: u32) -> bool {
    let (owner_tid, virtio_irq) = {
        let state = NIC_DRIVER_STATE.lock();
        (state.owner_tid, state.virtio_irq)
    };
    if owner_tid == 0 || irq == 0 {
        return false;
    }
    if virtio_irq == irq {
        return true;
    }
    #[cfg(target_arch = "x86_64")]
    if irq <= 0xFF && crate::task::drivers::pcie_msi::irq_owned_by(irq as u8, owner_tid) {
        return true;
    }
    false
}

/// Record `tid` as the active GPU driver.  Overwrites any previous registration.
pub fn register_gpu_driver(tid: usize) {
    GPU_DRIVER_CELL.store(tid, Ordering::Release);
    log::info!("[driver_cell] GPU driver registered: tid={}", tid);
}

/// Clear the GPU driver registration (called on cell exit/kill).
pub fn deregister_gpu_driver(tid: usize) {
    GPU_DRIVER_CELL
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// Record `tid` as the active USB HID event producer.
///
/// This never touches [`NIC_DRIVER_STATE`]: on a PC that already has
/// e1000/virtio-net, a USB cell claiming the NIC role would steal the network
/// route. `warn!` — see `publish_block_route`; this is the boot-integrity
/// marker the xhci integration test asserts and the identity the input
/// service's producer gate verifies.
pub fn register_usb_hid_producer(tid: usize) {
    USB_HID_PRODUCER_CELL.store(tid, Ordering::Release);
    log::warn!("[driver_cell] USB HID producer registered: tid={}", tid);
}

/// Clear the USB HID producer registration (called on cell exit/kill).
pub fn deregister_usb_hid_producer(tid: usize) {
    USB_HID_PRODUCER_CELL
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// TID of the registered input service Cell (0 = unregistered).
/// Set by the loader after spawning `/bin/input`; cleared on its death.
pub static INPUT_CELL_TID: AtomicUsize = AtomicUsize::new(0);

/// Register the input service cell.  Called by the loader after spawning `/bin/input`.
///
/// Logged at `warn!`, not `info!`: `/bin/input` spawns after the kernel drops
/// its log level to Warn at end-of-early-boot, so an info line here is
/// suppressed. This is a one-time boot-integrity event (the kernel now trusts
/// this TID as the sole keyboard-event sink) — worth surfacing, and the marker
/// the input-registration integration test asserts on.
pub fn set_input_cell(tid: usize) {
    INPUT_CELL_TID.store(tid, Ordering::Release);
    log::warn!("[input] registered input service TID {}", tid);
}

#[cfg(feature = "test-hooks")]
pub(crate) fn input_cell_snapshot() -> usize {
    INPUT_CELL_TID.load(Ordering::Acquire)
}

/// Clear the input service registration if it matches `tid` (called on cell death).
pub fn clear_input_cell_if(tid: usize) {
    INPUT_CELL_TID
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// Clear service routes, block route providers, and every Driver Cell role
/// owned by an exact dead TID.
///
/// Sharing `ROLE_PUBLICATION` with registration prevents teardown from
/// interleaving between role publication and service publication.
pub fn deregister_all_for(tid: usize) {
    let _publication = ROLE_PUBLICATION.lock();
    crate::cell::service_registry::clear_tid(tid);
    clear_input_cell_if(tid);
    clear_block_route_provider(tid);
    deregister_nic_driver(tid);
    deregister_gpu_driver(tid);
    deregister_usb_hid_producer(tid);
}
#[cfg(test)]
mod tests {
    use super::{
        block_route_record, deregister_all_for, owns_registered_nic_irq, publish_block_route,
        publish_role_or_rollback, reset_block_routes, BlockDeviceName, NicDriverState,
        GPU_DRIVER_CELL, INPUT_CELL_TID, NIC_DRIVER_STATE, USB_HID_PRODUCER_CELL,
    };
    use api::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI, MAX_BLOCK_ROUTES};
    use core::sync::atomic::{AtomicUsize, Ordering};

    fn pci(bdf: u64) -> DeviceKey {
        DeviceKey::new(BLOCK_DEVICE_PCI, bdf).expect("valid BDF")
    }

    fn provider_of(index: usize) -> Option<(u64, u64, u64)> {
        block_route_record(index).expect("bound slot").provider()
    }

    fn seed_roles(owner_tid: usize, nic_irq: u32) {
        reset_block_routes();
        if owner_tid != 0 {
            assert_eq!(publish_block_route(pci(0xF8), (owner_tid, 9, 1)), Some(0));
        }
        *NIC_DRIVER_STATE.lock() = NicDriverState {
            owner_tid,
            virtio_irq: nic_irq,
        };
        GPU_DRIVER_CELL.store(owner_tid, Ordering::Release);
        INPUT_CELL_TID.store(owner_tid, Ordering::Release);
        USB_HID_PRODUCER_CELL.store(owner_tid, Ordering::Release);
    }

    fn reset_roles() {
        seed_roles(0, 0);
    }

    #[test]
    fn deregister_all_for_respects_tid_matches_and_clears_stale_nic_irq_cache() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_roles();
        seed_roles(41, 29);

        deregister_all_for(7);

        assert_eq!(provider_of(0), Some((41, 9, 1)));
        let state = *NIC_DRIVER_STATE.lock();
        assert_eq!(state.owner_tid, 41);
        assert_eq!(state.virtio_irq, 29);
        assert_eq!(GPU_DRIVER_CELL.load(Ordering::Acquire), 41);
        assert_eq!(INPUT_CELL_TID.load(Ordering::Acquire), 41);
        assert_eq!(USB_HID_PRODUCER_CELL.load(Ordering::Acquire), 41);

        deregister_all_for(41);

        assert_eq!(provider_of(0), None, "exit clears the provider");
        assert_eq!(
            block_route_record(0).map(|r| r.device()),
            Some(pci(0xF8)),
            "key stays"
        );
        let state = *NIC_DRIVER_STATE.lock();
        assert_eq!(state.owner_tid, 0);
        assert_eq!(state.virtio_irq, 0);
        assert_eq!(GPU_DRIVER_CELL.load(Ordering::Acquire), 0);
        assert_eq!(INPUT_CELL_TID.load(Ordering::Acquire), 0);
        assert_eq!(USB_HID_PRODUCER_CELL.load(Ordering::Acquire), 0);

        seed_roles(22, 0);

        let state = *NIC_DRIVER_STATE.lock();
        assert_eq!(state.owner_tid, 22);
        assert_eq!(
            state.virtio_irq, 0,
            "stale NIC IRQ cache must not survive owner teardown"
        );

        reset_roles();
    }
    #[test]
    #[cfg(target_arch = "x86_64")]
    fn owns_registered_nic_irq_recognizes_pcie_msi_on_x86() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_roles();
        const NIC_OWNER: usize = 88_001;
        const OTHER_OWNER: usize = 88_002;
        let nic_alloc = crate::task::drivers::pcie_msi::allocate_for_test(NIC_OWNER, 0x0010, 2);
        let other_alloc = crate::task::drivers::pcie_msi::allocate_for_test(OTHER_OWNER, 0x0020, 2);

        // Before registration, neither is recognized.
        assert!(!owns_registered_nic_irq(nic_alloc.first_irq as u32));
        assert!(!owns_registered_nic_irq(other_alloc.first_irq as u32));

        // Register NIC owner.
        *NIC_DRIVER_STATE.lock() = NicDriverState {
            owner_tid: NIC_OWNER,
            virtio_irq: 0,
        };

        // NIC owner's MSI vectors are recognized.
        assert!(owns_registered_nic_irq(nic_alloc.first_irq as u32));
        assert!(owns_registered_nic_irq((nic_alloc.first_irq + 1) as u32));
        // Other owner's vectors or unallocated vectors are refused.
        assert!(!owns_registered_nic_irq(other_alloc.first_irq as u32));
        assert!(!owns_registered_nic_irq(0xFE));

        // Teardown clears recognition.
        deregister_all_for(NIC_OWNER);
        assert!(!owns_registered_nic_irq(nic_alloc.first_irq as u32));

        crate::task::drivers::pcie_msi::release_for(NIC_OWNER);
        crate::task::drivers::pcie_msi::release_for(OTHER_OWNER);
        reset_roles();
    }

    /// The table is append-only and keyed by device: a rebind keeps the slot,
    /// a new device appends, a live foreign provider and a full table are
    /// refused without touching the table.
    #[test]
    fn block_routes_append_by_key_and_refuse_without_mutation() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_block_routes();
        let mmio = DeviceKey::new(BLOCK_DEVICE_MMIO, 0x1000_1000).expect("valid base");
        assert_eq!(publish_block_route(pci(0xF8), (5, 50, 1)), Some(0));
        assert_eq!(publish_block_route(mmio, (6, 60, 1)), Some(1));
        assert_eq!(
            publish_block_route(pci(0xF8), (5, 50, 1)),
            Some(0),
            "idempotent"
        );

        assert_eq!(
            publish_block_route(pci(0xF8), (7, 50, 1)),
            None,
            "live foreign provider"
        );
        assert_eq!(provider_of(0), Some((5, 50, 1)));

        deregister_all_for(5);
        assert_eq!(
            publish_block_route(pci(0xF8), (7, 70, 2)),
            Some(0),
            "same slot, new provider"
        );
        assert_eq!(provider_of(0), Some((7, 70, 2)));
        assert_eq!(provider_of(1), Some((6, 60, 1)), "sibling untouched");

        for (n, bdf) in (2..MAX_BLOCK_ROUTES).zip(0x100u64..) {
            assert_eq!(publish_block_route(pci(bdf), (8, 80, 1)), Some(n));
        }
        assert_eq!(
            publish_block_route(pci(0x200), (9, 90, 1)),
            None,
            "table full"
        );
        assert!(block_route_record(MAX_BLOCK_ROUTES).is_none());
        assert!((0..MAX_BLOCK_ROUTES).all(|i| block_route_record(i).is_some_and(|r| r.id != 0x200)));
        reset_block_routes();
    }

    #[test]
    fn block_device_names_follow_the_pci_location_and_mmio_base() {
        assert_eq!(
            alloc::format!("{}", BlockDeviceName(pci(0xFA))),
            "pci-00-1f-2"
        );
        assert_eq!(
            alloc::format!("{}", BlockDeviceName(pci(0x0300))),
            "pci-03-00-0"
        );
        let mmio = DeviceKey::new(BLOCK_DEVICE_MMIO, 0x1000_1000).expect("valid base");
        assert_eq!(alloc::format!("{}", BlockDeviceName(mmio)), "mmio-10001000");
    }

    #[test]
    fn rejected_service_publication_rolls_back_only_the_exact_owner() {
        let owner = AtomicUsize::new(0);
        let publish = |tid| owner.store(tid, Ordering::Release);
        let rollback = |tid| {
            owner
                .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
                .ok();
        };

        assert!(!publish_role_or_rollback(41, publish, || false, rollback,));
        assert_eq!(owner.load(Ordering::Acquire), 0);

        assert!(!publish_role_or_rollback(
            41,
            |tid| owner.store(tid, Ordering::Release),
            || {
                owner.store(52, Ordering::Release);
                false
            },
            |tid| {
                owner
                    .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
                    .ok();
            },
        ));
        assert_eq!(
            owner.load(Ordering::Acquire),
            52,
            "rollback must not erase a concurrently published replacement"
        );
    }
}
