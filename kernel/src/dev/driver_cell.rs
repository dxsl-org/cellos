//! Driver Cell registration — tracks the per-device block and NIC routes and
//! which cells own the GPU and USB HID producer roles.
//!
//! A Tier-1 block Driver Cell binds the route of the device it owns with
//! `sys_register_block_device` (416). A NIC Driver Cell binds the route of the
//! one network device it owns with `sys_register_nic_driver` (417); the kernel
//! derives that route's key from the caller's owned device, so the call keeps
//! no arguments. `sys_register_gpu_driver` and `sys_register_usb_hid_producer`
//! record the caller's TID here. These registrations support service routing
//! and interrupt ownership checks until the owning Cell exits.
//!
//! A role TID of `0` means "no driver cell registered". NIC has no kernel
//! fallback and is always a Driver Cell. GPU has no kernel fallback; compositor
//! refuses to init until a GPU Cell registers. The USB HID producer role exists
//! so a USB host cell can authorize raw HID events into the input service
//! without taking a NIC route.

use crate::sync::Spinlock;
use abi::block_route::{
    DeviceKey, ViBlockRouteV1, BLOCK_DEVICE_PCI, BLOCK_ROUTE_VERSION, FLAG_PROVIDER_LIVE,
    MAX_BLOCK_ROUTES,
};
use core::sync::atomic::{AtomicUsize, Ordering};

/// A live route provider: `(tid, cell_id, generation)`.
pub(crate) type Provider = (usize, u64, u64);

/// One storage device's route: its key is fixed at first bind; only the
/// provider changes.
#[derive(Clone, Copy)]
struct BlockRouteSlot {
    key: DeviceKey,
    provider: Option<Provider>,
}

/// Per-device block routes, append-only for the boot: a slot index never
/// changes owner device and a key is never cleared. Lock order
/// `SCHEDULER -> ROLE_PUBLICATION -> BLOCK_ROUTES` (leaf); no allocation,
/// because exit teardown may run in ISR context.
static BLOCK_ROUTES: Spinlock<[Option<BlockRouteSlot>; MAX_BLOCK_ROUTES]> =
    Spinlock::ranked([None; MAX_BLOCK_ROUTES], crate::sync::rank::BLOCK_ROUTES);

/// Log name of a device: `pci-BB-DD-F` or `mmio-<base hex>`.
pub(crate) struct DeviceName(pub(crate) DeviceKey);

impl core::fmt::Display for DeviceName {
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

/// NIC route slots. Fixed so the interrupt-time reader never allocates.
pub(crate) const MAX_NIC_ROUTES: usize = 8;

/// One network device's route: its key is fixed at first bind; only the
/// provider changes. The route's interrupts are its device record's.
#[derive(Clone, Copy)]
struct NicRoute {
    key: DeviceKey,
    provider: Option<Provider>,
}

#[derive(Clone, Copy)]
struct NicRoutes {
    slots: [Option<NicRoute>; MAX_NIC_ROUTES],
    /// The route whose provider `service::NIC_DRIVER` names. The first live
    /// registration takes it and keeps it while its provider lives. When that
    /// provider dies, the next live route takes it in place
    /// ([`clear_nic_route_provider`]); with none, the next registration does.
    primary: Option<usize>,
}

/// Per-device NIC routes, append-only for the boot like [`BLOCK_ROUTES`].
/// Lock order `SCHEDULER -> ROLE_PUBLICATION -> NIC_ROUTES` (released before
/// the service registry is written); the MSI and VirtIO ISRs copy it out.
static NIC_ROUTES: Spinlock<NicRoutes> = Spinlock::ranked(
    NicRoutes {
        slots: [None; MAX_NIC_ROUTES],
        primary: None,
    },
    crate::sync::rank::NIC_ROUTES,
);

/// TID of the registered GPU Driver Cell (0 = none; no kernel GPU fallback).
pub static GPU_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// TID of the registered USB HID event producer (0 = none).
///
/// Distinct from the NIC routes on purpose: a USB host controller cell
/// (`/bin/xhci` on a PC, `/bin/dwc2-usb` on RPi3) publishes this role to
/// authorize raw HID events into the input service **without** taking a NIC
/// route. The input service's producer gate resolves
/// `service::USB_HID_PRODUCER` to this TID.
pub static USB_HID_PRODUCER_CELL: AtomicUsize = AtomicUsize::new(0);
/// Serializes role state with its matching service-registry publication.
static ROLE_PUBLICATION: Spinlock<()> = Spinlock::ranked((), crate::sync::rank::ROLE_PUBLICATION);

const VIRTIO_DEVICE_ID_OFFSET: usize = 0x8;
const VIRTIO_DEVICE_ID_NETWORK: u32 = 1;
/// PCI class code of an Ethernet controller (`02:00:00`).
const CLASS_ETHERNET: u32 = 0x02_00_00;

/// Why the caller of 417 has no NIC route key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NicDeviceError {
    /// The caller owns no network device.
    None,
    /// The caller owns more than one; 417 names none of them.
    Several,
}

/// The one network device `tid` owns, as a route key: a PCI function of
/// class `02:00:00` it claimed through 418/424, a VirtIO MMIO slot it claimed
/// whose `device_id` is network, or the DWC2 aperture (the RPi3 LAN9514 sits
/// behind it). Read from the device records.
///
/// Syscall context only, without `SCHEDULER` held (`DEVICES` ranks after it).
pub(crate) fn owned_network_device(tid: usize) -> Result<DeviceKey, NicDeviceError> {
    use crate::dev::ownership::DeviceKind;
    let mut found = None;
    let mut count = 0usize;
    for (key, kind) in crate::dev::ownership::devices_of(tid) {
        let network = match kind {
            DeviceKind::Pci { class } => class == Some(CLASS_ETHERNET),
            // SAFETY: syscall context, never an ISR. A VirtIO slot record is
            // keyed by one of the kernel's enumerated, identity-mapped VirtIO
            // MMIO bases (`dev::init`), and this TID owns it, so it claimed
            // the slot via `RequestMmio` before we read the read-only
            // `device_id` register at offset 0x8.
            DeviceKind::VirtioSlot => {
                let device_id = unsafe {
                    core::ptr::read_volatile(
                        (key.id as usize + VIRTIO_DEVICE_ID_OFFSET) as *const u32,
                    )
                };
                device_id == VIRTIO_DEVICE_ID_NETWORK
            }
            DeviceKind::Dwc2 => true,
        };
        if network {
            count += 1;
            found.get_or_insert(key);
        }
    }
    match (count, found) {
        (1, Some(device)) => Ok(device),
        (0, _) | (_, None) => Err(NicDeviceError::None),
        _ => Err(NicDeviceError::Several),
    }
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
pub(crate) fn publish_block_route(key: DeviceKey, provider: Provider) -> Option<usize> {
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
    let name = DeviceName(key);
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

/// Clear the provider of every block route naming exactly `tid` whose device
/// `released` names; keys stay.
fn clear_block_route_provider(tid: usize, released: impl Fn(DeviceKey) -> bool) {
    for slot in BLOCK_ROUTES.lock().iter_mut().flatten() {
        if slot.provider.is_some_and(|(holder, _, _)| holder == tid) && released(slot.key) {
            slot.provider = None;
        }
    }
}

/// Empty the route table; host tests only, under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn reset_block_routes() {
    *BLOCK_ROUTES.lock() = [None; MAX_BLOCK_ROUTES];
}

/// Bind the NIC route of `key` to `provider`.
///
/// The caller holds `SCHEDULER` and has proven that `provider` owns `key`
/// ([`owned_network_device`]). The slot rules are those of
/// [`publish_block_route`]: a known key keeps its slot and rebinds an empty
/// provider or the same tid, a live foreign provider is refused, a new key
/// takes the next free slot. The bound route becomes the primary when no
/// other route is (`NIC_ROUTES.primary`), and only then is
/// `publish_primary` — the `service::NIC_DRIVER` publication — called; if it
/// fails, the bind is rolled back. A secondary route is bound without touching
/// the service. `None` means refused, with the table unchanged.
///
/// `warn!` — see `publish_block_route`. The line keeps the prefix the NIC
/// lanes match and gains the device and route.
pub(crate) fn publish_nic_route(
    key: DeviceKey,
    provider: Provider,
    publish_primary: impl FnOnce() -> bool,
) -> Option<usize> {
    enum Bind {
        Bound { index: usize, primary: bool },
        Held(usize, usize),
        Full,
    }
    let _publication = ROLE_PUBLICATION.lock();
    let tid = provider.0;
    let bound = Some(NicRoute {
        key,
        provider: Some(provider),
    });
    let outcome = {
        let mut routes = NIC_ROUTES.lock();
        let slot = match routes
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|slot| slot.key == key))
        {
            Some(index) => match routes.slots[index].and_then(|slot| slot.provider) {
                Some((holder, _, _)) if holder != tid => Err(Bind::Held(index, holder)),
                _ => Ok(index),
            },
            None => routes
                .slots
                .iter()
                .position(Option::is_none)
                .ok_or(Bind::Full),
        };
        match slot {
            Ok(index) => {
                routes.slots[index] = bound;
                crate::dev::ownership::set_nic_route(key, Some(tid));
                let primary = routes.primary.is_none_or(|primary| primary == index);
                if primary {
                    routes.primary = Some(index);
                }
                Bind::Bound { index, primary }
            }
            Err(refused) => refused,
        }
    };
    let name = DeviceName(key);
    match outcome {
        Bind::Bound { index, primary } => {
            if primary && !publish_primary() {
                {
                    let mut routes = NIC_ROUTES.lock();
                    if let Some(route) = routes.slots[index].as_mut() {
                        route.provider = None;
                    }
                    crate::dev::ownership::set_nic_route(key, None);
                    routes.primary = None;
                }
                log::warn!(
                    "[driver_cell] NIC route {} device {} not published; tid={} refused",
                    index,
                    name,
                    tid
                );
                return None;
            }
            log::warn!(
                "[driver_cell] NIC driver registered: tid={} device={} route={}",
                tid,
                name,
                index
            );
            if !primary {
                log::warn!(
                    "[driver_cell] NIC route {} is secondary: the net service stays on the primary route",
                    index
                );
            }
            Some(index)
        }
        Bind::Held(index, holder) => {
            log::warn!(
                "[driver_cell] NIC route {} device {} held by live tid={}; tid={} refused",
                index,
                name,
                holder,
                tid
            );
            None
        }
        Bind::Full => {
            log::warn!(
                "[driver_cell] NIC route table full; device {} refused",
                name
            );
            None
        }
    }
}

/// Clear the provider of every NIC route naming exactly `tid` whose device
/// `released` names; keys stay.
///
/// When the primary route's provider is cleared, the lowest-index route that
/// still has a provider becomes the primary, and `service::NIC_DRIVER` is
/// re-pointed in place from `tid` to that provider, before
/// `service_registry::clear_tid` would remove the row. Nothing is allocated
/// (exit teardown may run in an ISR), and no authority is granted: the
/// successor already registered its own route. If the row no longer names
/// `tid` (or is paused for a hot swap), or no route is live, the primary is
/// left vacant and the next registration takes it; `true` then reports that
/// the primary was lost without a hand-over.
///
/// Caller holds `ROLE_PUBLICATION`; `NIC_ROUTES` is released before the
/// service registry is written, as in [`publish_nic_route`].
fn clear_nic_route_provider(tid: usize, released: impl Fn(DeviceKey) -> bool) -> bool {
    let (lost_primary, promoted) = {
        let mut routes = NIC_ROUTES.lock();
        let NicRoutes { slots, primary } = &mut *routes;
        let mut lost_primary = false;
        for (index, route) in slots.iter_mut().enumerate() {
            let Some(route) = route.as_mut() else {
                continue;
            };
            if route.provider.is_some_and(|(holder, _, _)| holder == tid) && released(route.key) {
                route.provider = None;
                crate::dev::ownership::set_nic_route(route.key, None);
                if *primary == Some(index) {
                    *primary = None;
                    lost_primary = true;
                }
            }
        }
        let successor =
            slots
                .iter()
                .enumerate()
                .filter(|_| lost_primary)
                .find_map(|(index, route)| {
                    let route = (*route)?;
                    Some((index, route.key, route.provider?))
                });
        if let Some((index, _, _)) = successor {
            *primary = Some(index);
        }
        (lost_primary, successor)
    };
    let Some((index, key, provider)) = promoted else {
        return lost_primary;
    };
    if crate::cell::service_registry::repoint(abi::syscall::service::NIC_DRIVER, tid, provider) {
        log::warn!(
            "[driver_cell] NIC route {} promoted to primary: tid={} device={}",
            index,
            provider.0,
            DeviceName(key)
        );
        false
    } else {
        let mut routes = NIC_ROUTES.lock();
        if routes.primary == Some(index) {
            routes.primary = None;
        }
        true
    }
}

/// Unbind the NIC and block routes of `tid` whose device it no longer owns:
/// a runtime capability revoke freed those devices
/// ([`ownership::release_for`](crate::dev::ownership::release_for),
/// [`ownership::release_mmio_devices_in`](crate::dev::ownership::release_mmio_devices_in))
/// while the task lives on, so a replacement's 416/417 must find the route
/// vacant. A route on a device it still owns (the DWC2 aperture, held by
/// `UsbDriverCap`) stays. A lost NIC primary passes to the next live route as
/// at exit; with none, the active `service::NIC_DRIVER` row naming `tid` is
/// removed, so the net service does not keep resolving to a driver without
/// a device, and the next registration takes the primary.
///
/// Syscall context, without `SCHEDULER` held; same `ROLE_PUBLICATION`
/// transaction as registration and exit teardown.
pub(crate) fn release_unowned_routes(tid: usize) {
    let _publication = ROLE_PUBLICATION.lock();
    let released = |key| !crate::dev::ownership::owns(tid, key);
    if clear_nic_route_provider(tid, released) {
        crate::cell::service_registry::remove_active(abi::syscall::service::NIC_DRIVER, tid);
    }
    clear_block_route_provider(tid, released);
}

/// Empty the NIC route table; host tests only, under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn reset_nic_routes() {
    let mut routes = NIC_ROUTES.lock();
    routes.slots = [None; MAX_NIC_ROUTES];
    routes.primary = None;
    crate::dev::ownership::test_clear_nic_routes();
}

/// Whether `irq` belongs to the device of a live NIC route: a line of the
/// route's device record that the route's provider owns (its VirtIO slot's
/// line, or on x86_64 an MSI vector 435 allocated for its PCI function). Each
/// route proves its own lines, so a provider's interrupt for any other device
/// never signals `NET_RX`.
///
/// ISR-safe and lock-free: one atomic load of the NIC line set, which
/// `dev::ownership` rewrites under `DEVICES` whenever a route provider, an
/// owner or a line changes ([`ownership::is_nic_line`](crate::dev::ownership::is_nic_line)).
pub fn owns_registered_nic_irq(irq: u32) -> bool {
    u8::try_from(irq).is_ok_and(|irq| irq != 0 && crate::dev::ownership::is_nic_line(irq))
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
/// This never touches the NIC routes: on a PC that already has
/// e1000/virtio-net, a USB cell taking a NIC route would steal the network.
/// `warn!` — see `publish_block_route`; this is the boot-integrity
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

/// Clear service routes, block and NIC route providers, and every Driver Cell
/// role owned by an exact dead TID.
///
/// Sharing `ROLE_PUBLICATION` with registration prevents teardown from
/// interleaving between role publication and service publication.
pub fn deregister_all_for(tid: usize) {
    let _publication = ROLE_PUBLICATION.lock();
    // First: a NIC primary is re-pointed while its service row still names
    // `tid`, so `clear_tid` below leaves the successor's row.
    clear_nic_route_provider(tid, |_| true);
    crate::cell::service_registry::clear_tid(tid);
    clear_input_cell_if(tid);
    clear_block_route_provider(tid, |_| true);
    deregister_gpu_driver(tid);
    deregister_usb_hid_producer(tid);
}
#[cfg(test)]
mod tests {
    use super::{
        block_route_record, deregister_all_for, owns_registered_nic_irq, publish_block_route,
        publish_nic_route, publish_role_or_rollback, reset_block_routes, reset_nic_routes,
        DeviceName, Provider, GPU_DRIVER_CELL, INPUT_CELL_TID, MAX_NIC_ROUTES, NIC_ROUTES,
        USB_HID_PRODUCER_CELL,
    };
    use abi::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI, MAX_BLOCK_ROUTES};
    use core::sync::atomic::{AtomicUsize, Ordering};

    fn pci(bdf: u64) -> DeviceKey {
        DeviceKey::new(BLOCK_DEVICE_PCI, bdf).expect("valid BDF")
    }

    fn provider_of(index: usize) -> Option<(u64, u64, u64)> {
        block_route_record(index).expect("bound slot").provider()
    }

    fn mmio(base: u64) -> DeviceKey {
        DeviceKey::new(BLOCK_DEVICE_MMIO, base).expect("valid base")
    }

    fn nic_route(index: usize) -> Option<Provider> {
        NIC_ROUTES.lock().slots[index]
            .expect("keyed NIC route")
            .provider
    }

    fn nic_primary() -> Option<usize> {
        NIC_ROUTES.lock().primary
    }

    /// A VirtIO slot at `base` with line `line`, owned by `tid`.
    fn owned_slot(tid: usize, base: u64, line: u8) -> DeviceKey {
        crate::dev::ownership::test_own_mmio(tid, base as usize, line);
        mmio(base)
    }

    fn seed_roles(owner_tid: usize, nic_irq: u8) {
        reset_block_routes();
        reset_nic_routes();
        if owner_tid != 0 {
            assert_eq!(publish_block_route(pci(0xF8), (owner_tid, 9, 1)), Some(0));
            let slot = owned_slot(owner_tid, 0x1000_8000, nic_irq);
            assert_eq!(publish_nic_route(slot, (owner_tid, 9, 1), || true), Some(0));
        }
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
        assert_eq!(nic_route(0), Some((41, 9, 1)));
        assert!(owns_registered_nic_irq(29));
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
        assert_eq!(nic_route(0), None);
        assert_eq!(nic_primary(), None);
        assert!(!owns_registered_nic_irq(29));
        assert_eq!(GPU_DRIVER_CELL.load(Ordering::Acquire), 0);
        assert_eq!(INPUT_CELL_TID.load(Ordering::Acquire), 0);
        assert_eq!(USB_HID_PRODUCER_CELL.load(Ordering::Acquire), 0);

        assert_eq!(
            publish_nic_route(mmio(0x1000_8000), (22, 9, 2), || true),
            Some(0),
            "the replacement rebinds the same route"
        );
        assert_eq!(nic_route(0), Some((22, 9, 2)));
        assert!(
            !owns_registered_nic_irq(29),
            "the dead owner's line is not the replacement's until it owns the device"
        );

        reset_roles();
    }

    /// Two NICs keep their own routes: the first live registration is the
    /// primary and publishes the service, the second binds without it, a
    /// foreign rebind is refused, and the death of one provider leaves the
    /// other bound; the next registration takes the primary.
    #[test]
    fn nic_routes_are_per_device_and_one_death_leaves_the_other_bound() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_nic_routes();
        let published = core::cell::Cell::new(0usize);
        let published = &published;
        let publish = |tid: usize| {
            move || {
                published.set(tid);
                true
            }
        };
        let second = owned_slot(32, 0x1000_9000, 7);
        assert_eq!(
            publish_nic_route(pci(0x18), (31, 3, 1), publish(31)),
            Some(0)
        );
        assert_eq!(publish_nic_route(second, (32, 4, 1), publish(32)), Some(1));
        assert_eq!(published.get(), 31, "a secondary does not take the service");
        assert_eq!(nic_primary(), Some(0));
        assert_eq!(
            publish_nic_route(pci(0x18), (33, 5, 1), publish(33)),
            None,
            "live foreign provider"
        );
        assert_eq!(nic_route(0), Some((31, 3, 1)));
        assert!(owns_registered_nic_irq(7), "the secondary's line is proven");

        deregister_all_for(31);
        assert_eq!(nic_route(0), None);
        assert_eq!(nic_route(1), Some((32, 4, 1)), "the other NIC stays bound");
        assert!(owns_registered_nic_irq(7));
        assert_eq!(
            nic_primary(),
            None,
            "no service row names tid 31 here, so none is re-pointed"
        );

        assert_eq!(
            publish_nic_route(pci(0x18), (34, 3, 2), publish(34)),
            Some(0),
            "the replacement regains its own route"
        );
        assert_eq!(published.get(), 34, "and the vacant primary");
        assert_eq!(nic_primary(), Some(0));
        assert_eq!(nic_route(1), Some((32, 4, 1)));

        deregister_all_for(34);
        deregister_all_for(32);
        reset_nic_routes();
    }

    /// The primary's death re-points `service::NIC_DRIVER` in place to the
    /// next live route; a secondary's death moves nothing; with no live route
    /// left the primary is vacant and the row is gone.
    #[test]
    fn a_dead_primary_hands_the_nic_service_to_the_next_live_route() {
        use crate::cell::service_registry::{lookup_bound, register};
        use abi::syscall::service::NIC_DRIVER;
        const A: Provider = (0x5A01, 51, 1);
        const B: Provider = (0x5A02, 52, 1);
        const C: Provider = (0x5A03, 53, 1);
        const A2: Provider = (0x5A04, 51, 2);
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_nic_routes();
        let publish =
            |(tid, cell, generation): Provider| move || register(NIC_DRIVER, tid, cell, generation);
        assert_eq!(publish_nic_route(pci(0x5A0), A, publish(A)), Some(0));
        assert_eq!(publish_nic_route(pci(0x5A1), B, publish(B)), Some(1));
        assert_eq!(publish_nic_route(pci(0x5A2), C, publish(C)), Some(2));
        assert_eq!(lookup_bound(NIC_DRIVER), Some(A));

        deregister_all_for(B.0);
        assert_eq!(
            (nic_primary(), lookup_bound(NIC_DRIVER)),
            (Some(0), Some(A))
        );

        deregister_all_for(A.0);
        assert_eq!(nic_route(0), None);
        assert_eq!(nic_primary(), Some(2), "the next live route");
        assert_eq!(lookup_bound(NIC_DRIVER), Some(C), "networking survives");

        assert_eq!(
            publish_nic_route(pci(0x5A0), A2, publish(A2)),
            Some(0),
            "the replacement rebinds its own route as a secondary"
        );
        assert_eq!(lookup_bound(NIC_DRIVER), Some(C));

        deregister_all_for(C.0);
        assert_eq!(
            (nic_primary(), lookup_bound(NIC_DRIVER)),
            (Some(0), Some(A2))
        );
        deregister_all_for(A2.0);
        assert_eq!((nic_primary(), lookup_bound(NIC_DRIVER)), (None, None));
        reset_nic_routes();
    }

    /// A primary whose service publication fails leaves no provider and no
    /// primary; the table is full at `MAX_NIC_ROUTES` keys.
    #[test]
    fn nic_route_rolls_back_a_refused_publication_and_refuses_a_full_table() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_nic_routes();
        let slot = owned_slot(41, 0x1000_A000, 9);
        assert_eq!(publish_nic_route(slot, (41, 4, 1), || false), None);
        assert_eq!(nic_route(0), None);
        assert_eq!(nic_primary(), None);
        assert!(!owns_registered_nic_irq(9));

        for (n, bdf) in (1..MAX_NIC_ROUTES).zip(0x20u64..) {
            assert_eq!(publish_nic_route(pci(bdf), (42, 4, 1), || true), Some(n));
        }
        assert_eq!(
            publish_nic_route(pci(0x100), (43, 4, 1), || true),
            None,
            "table full"
        );
        deregister_all_for(42);
        reset_nic_routes();
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn owns_registered_nic_irq_recognizes_only_the_routes_own_msi_on_x86() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_roles();
        const NIC_OWNER: usize = 88_001;
        const OTHER_OWNER: usize = 88_002;
        let nic_alloc = crate::arch::dev::pcie_msi::allocate_for_test(NIC_OWNER, 0x0010, 2);
        // The same provider's vector for a function that is not its route.
        let side_alloc = crate::arch::dev::pcie_msi::allocate_for_test(NIC_OWNER, 0x0030, 1);
        let other_alloc = crate::arch::dev::pcie_msi::allocate_for_test(OTHER_OWNER, 0x0020, 2);

        // Before registration, neither is recognized.
        assert!(!owns_registered_nic_irq(nic_alloc.first_irq as u32));
        assert!(!owns_registered_nic_irq(other_alloc.first_irq as u32));

        assert_eq!(
            publish_nic_route(pci(0x10), (NIC_OWNER, 8, 1), || true),
            Some(0)
        );

        // The route's MSI vectors are recognized.
        assert!(owns_registered_nic_irq(nic_alloc.first_irq as u32));
        assert!(owns_registered_nic_irq((nic_alloc.first_irq + 1) as u32));
        // Other functions' vectors or unallocated vectors are refused.
        assert!(!owns_registered_nic_irq(side_alloc.first_irq as u32));
        assert!(!owns_registered_nic_irq(other_alloc.first_irq as u32));
        assert!(!owns_registered_nic_irq(0xFE));
        assert!(!owns_registered_nic_irq(
            0x1_0000 + nic_alloc.first_irq as u32
        ));

        // Teardown clears recognition.
        deregister_all_for(NIC_OWNER);
        assert!(!owns_registered_nic_irq(nic_alloc.first_irq as u32));

        crate::arch::dev::pcie_msi::release_for(NIC_OWNER);
        crate::arch::dev::pcie_msi::release_for(OTHER_OWNER);
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
    fn device_names_follow_the_pci_location_and_mmio_base() {
        assert_eq!(alloc::format!("{}", DeviceName(pci(0xFA))), "pci-00-1f-2");
        assert_eq!(alloc::format!("{}", DeviceName(pci(0x0300))), "pci-03-00-0");
        assert_eq!(
            alloc::format!("{}", DeviceName(mmio(0x1000_1000))),
            "mmio-10001000"
        );
    }

    /// 417's key is the caller's one owned network device, from its record:
    /// a non-network function does not count, a second network one refuses.
    #[test]
    fn the_nic_route_key_is_the_one_owned_network_device() {
        use super::{owned_network_device, NicDeviceError, CLASS_ETHERNET};
        use crate::dev::ownership::{claim_pci, register_pci_function, release_for, Scope};
        const TID: usize = 0x5E01;
        let _guard = crate::TEST_STATE_LOCK.lock();
        register_pci_function(0x5E0, CLASS_ETHERNET);
        register_pci_function(0x5E1, 0x01_06_01);
        register_pci_function(0x5E2, CLASS_ETHERNET);
        assert_eq!(owned_network_device(TID), Err(NicDeviceError::None));
        assert!(claim_pci(0x5E1, TID));
        assert_eq!(owned_network_device(TID), Err(NicDeviceError::None));
        assert!(claim_pci(0x5E0, TID));
        assert_eq!(owned_network_device(TID), Ok(pci(0x5E0)));
        assert!(claim_pci(0x5E2, TID));
        assert_eq!(owned_network_device(TID), Err(NicDeviceError::Several));
        release_for(TID, Scope::Retired);
        assert_eq!(owned_network_device(TID), Err(NicDeviceError::None));
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

    /// Review finding: a runtime `PcieDriverCap` revoke frees the device, so
    /// the (live) revoked task's NIC and block routes on it are unbound — the
    /// replacement's 417 is not refused as Held — while a route on a device
    /// it still owns stays. The lost primary passes to the next live route;
    /// with none, the `NIC_DRIVER` row naming the revoked task is removed and
    /// the next registration takes the primary.
    #[test]
    fn a_revoke_unbinds_the_routes_of_the_devices_it_freed() {
        use super::release_unowned_routes;
        use crate::cell::service_registry::{lookup_bound, register};
        use crate::dev::ownership::{
            claim_pci, claim_window, register_mmio_device, release_for, DeviceKind, Scope,
            WindowClaim,
        };
        use abi::syscall::service::NIC_DRIVER;
        const A: Provider = (0x5D01, 61, 1);
        const B: Provider = (0x5D02, 62, 1);
        const C: Provider = (0x5D03, 63, 1);
        const D: Provider = (0x5D04, 64, 1);
        const DWC2: usize = 0x5D00_0000;
        let _guard = crate::TEST_STATE_LOCK.lock();
        reset_nic_routes();
        reset_block_routes();
        let publish =
            |(tid, cell, generation): Provider| move || register(NIC_DRIVER, tid, cell, generation);
        assert!(claim_pci(0x5D0, A.0) && claim_pci(0x5D2, A.0));
        register_mmio_device(DWC2, 0x1_0000, DeviceKind::Dwc2, None);
        assert_eq!(
            claim_window(A.0, DWC2, 0x1000, true),
            WindowClaim::Claimed(mmio(DWC2 as u64))
        );
        assert!(claim_pci(0x5D1, B.0));
        assert_eq!(publish_nic_route(pci(0x5D0), A, publish(A)), Some(0));
        assert_eq!(publish_nic_route(pci(0x5D1), B, publish(B)), Some(1));
        assert_eq!(publish_block_route(pci(0x5D2), A), Some(0));
        assert_eq!(publish_block_route(mmio(DWC2 as u64), A), Some(1));
        assert_eq!(lookup_bound(NIC_DRIVER), Some(A));

        release_for(A.0, Scope::PcieDriver);
        release_unowned_routes(A.0);
        assert_eq!(nic_route(0), None, "freed device's route unbound");
        assert_eq!(
            (nic_primary(), lookup_bound(NIC_DRIVER)),
            (Some(1), Some(B))
        );
        assert_eq!(provider_of(0), None, "freed block device unbound");
        assert_eq!(provider_of(1), Some((A.0 as u64, A.1, A.2)), "DWC2 kept");

        assert!(claim_pci(0x5D0, C.0));
        assert_eq!(
            publish_nic_route(pci(0x5D0), C, publish(C)),
            Some(0),
            "the replacement is not refused as Held"
        );
        assert_eq!(lookup_bound(NIC_DRIVER), Some(B), "a secondary");

        release_for(B.0, Scope::PcieDriver);
        release_unowned_routes(B.0);
        assert_eq!(
            (nic_primary(), lookup_bound(NIC_DRIVER)),
            (Some(0), Some(C))
        );
        release_for(C.0, Scope::PcieDriver);
        release_unowned_routes(C.0);
        assert_eq!(
            (nic_primary(), lookup_bound(NIC_DRIVER)),
            (None, None),
            "no live route: the revoked task's row is removed"
        );

        assert!(claim_pci(0x5D1, D.0));
        assert_eq!(publish_nic_route(pci(0x5D1), D, publish(D)), Some(1));
        assert_eq!(
            (nic_primary(), lookup_bound(NIC_DRIVER)),
            (Some(1), Some(D))
        );

        for (tid, _, _) in [A, B, C, D] {
            deregister_all_for(tid);
            release_for(tid, Scope::Retired);
        }
        reset_nic_routes();
        reset_block_routes();
    }
}
