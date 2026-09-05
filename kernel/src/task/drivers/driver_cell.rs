//! Driver Cell registration — tracks which cells own the block, NIC, and GPU roles.
//!
//! When a Tier-1 Driver Cell calls `sys_register_block_driver`,
//! `sys_register_nic_driver`, or `sys_register_gpu_driver`, the kernel records
//! its TID here. These registrations support service routing and interrupt
//! ownership checks until the owning Cell exits.
//!
//! `0` means "no driver cell registered". Block can fall back to kernel-resident
//! virtio-blk or MMC; NIC has no kernel fallback and is always a Driver Cell.
//! GPU has no kernel fallback; compositor refuses to init until a GPU Cell registers.

use crate::sync::Spinlock;
use core::sync::atomic::{AtomicUsize, Ordering};

/// TID of the registered block Driver Cell (0 = none; kernel virtio_blk/MMC is the fallback).
pub static BLOCK_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

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

/// Publish a live task's driver role and matching service route.
///
/// Lock order is `SCHEDULER -> ROLE_PUBLICATION -> service/role leaves`, the
/// same order used by task teardown. Holding SCHEDULER through publication
/// prevents a remote kill from completing before a dead TID is registered.
pub(crate) fn publish_role_or_rollback(
    tid: usize,
    publish_role: impl FnOnce(usize),
    publish_service: impl FnOnce() -> bool,
    rollback_role: impl FnOnce(usize),
) -> bool {
    let scheduler = crate::task::SCHEDULER.lock();
    let live = scheduler
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&tid))
        .is_some_and(|task| {
            !matches!(
                &task.state,
                crate::task::tcb::TaskState::Terminated | crate::task::tcb::TaskState::Retiring
            )
        });
    if !live {
        return false;
    }
    let published =
        publish_live_role_or_rollback(tid, publish_role, publish_service, rollback_role);
    drop(scheduler);
    published
}

fn publish_live_role_or_rollback(
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

/// Record `tid` as the active block driver.  Overwrites any previous registration.
///
/// Logged at `warn!` for the same reason as `set_input_cell`: Driver Cells are
/// spawned by init AFTER the kernel drops its log level to Warn, this is a
/// one-time boot-integrity event (the kernel now routes all sector I/O to this
/// TID), and it is the marker the x86 nvme/nic integration tests assert on.
pub fn register_block_driver(tid: usize) {
    BLOCK_DRIVER_CELL.store(tid, Ordering::Release);
    log::warn!("[driver_cell] block driver registered: tid={}", tid);
}

/// Record `tid` as the active NIC driver.  Overwrites any previous registration.
/// `warn!` — see `register_block_driver`.
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

/// Clear the block driver registration (called on cell exit/kill).
pub fn deregister_block_driver(tid: usize) {
    BLOCK_DRIVER_CELL
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// Clear the NIC driver registration (called on cell exit/kill).
pub fn deregister_nic_driver(tid: usize) {
    let mut state = NIC_DRIVER_STATE.lock();
    if state.owner_tid == tid {
        *state = NicDriverState::empty();
    }
}

/// Returns true only for the cached VirtIO IRQ proven to belong to the active NIC.
pub fn owns_registered_nic_irq(irq: u32) -> bool {
    let state = NIC_DRIVER_STATE.lock();
    state.owner_tid != 0 && irq != 0 && state.virtio_irq == irq
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

/// Clear service routes and every Driver Cell role owned by an exact dead TID.
///
/// Sharing `ROLE_PUBLICATION` with registration prevents teardown from
/// interleaving between role publication and service publication.
pub fn deregister_all_for(tid: usize) {
    let _publication = ROLE_PUBLICATION.lock();
    crate::cell::service_registry::clear_tid(tid);
    clear_input_cell_if(tid);
    deregister_block_driver(tid);
    deregister_nic_driver(tid);
    deregister_gpu_driver(tid);
}
#[cfg(test)]
mod tests {
    use super::{
        deregister_all_for, publish_live_role_or_rollback, NicDriverState, BLOCK_DRIVER_CELL,
        GPU_DRIVER_CELL, INPUT_CELL_TID, NIC_DRIVER_STATE,
    };
    use core::sync::atomic::{AtomicUsize, Ordering};

    fn seed_roles(owner_tid: usize, nic_irq: u32) {
        BLOCK_DRIVER_CELL.store(owner_tid, Ordering::Release);
        *NIC_DRIVER_STATE.lock() = NicDriverState {
            owner_tid,
            virtio_irq: nic_irq,
        };
        GPU_DRIVER_CELL.store(owner_tid, Ordering::Release);
        INPUT_CELL_TID.store(owner_tid, Ordering::Release);
    }

    fn reset_roles() {
        seed_roles(0, 0);
    }

    #[test]
    fn deregister_all_for_respects_tid_matches_and_clears_stale_nic_irq_cache() {
        reset_roles();
        seed_roles(41, 29);

        deregister_all_for(7);

        assert_eq!(BLOCK_DRIVER_CELL.load(Ordering::Acquire), 41);
        let state = *NIC_DRIVER_STATE.lock();
        assert_eq!(state.owner_tid, 41);
        assert_eq!(state.virtio_irq, 29);
        assert_eq!(GPU_DRIVER_CELL.load(Ordering::Acquire), 41);
        assert_eq!(INPUT_CELL_TID.load(Ordering::Acquire), 41);

        deregister_all_for(41);

        assert_eq!(BLOCK_DRIVER_CELL.load(Ordering::Acquire), 0);
        let state = *NIC_DRIVER_STATE.lock();
        assert_eq!(state.owner_tid, 0);
        assert_eq!(state.virtio_irq, 0);
        assert_eq!(GPU_DRIVER_CELL.load(Ordering::Acquire), 0);
        assert_eq!(INPUT_CELL_TID.load(Ordering::Acquire), 0);

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
    fn rejected_service_publication_rolls_back_only_the_exact_owner() {
        let owner = AtomicUsize::new(0);
        let publish = |tid| owner.store(tid, Ordering::Release);
        let rollback = |tid| {
            owner
                .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
                .ok();
        };

        assert!(!publish_live_role_or_rollback(
            41,
            publish,
            || false,
            rollback,
        ));
        assert_eq!(owner.load(Ordering::Acquire), 0);

        assert!(!publish_live_role_or_rollback(
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
