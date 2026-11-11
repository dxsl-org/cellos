// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: dev (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, fs::*, grant::*, ipc::*, misc::*, spawn::*, vm::*};

/// 434/435 results: the typed sentinels of [`encode_ipc_result`] plus
/// `NotSupported` and `OutOfMemory` (vectors exhausted), which a driver must
/// tell apart to fall back to polling (`api::pcie_function::ERR_*`).
#[cfg(not(target_arch = "riscv32"))]
pub(super) fn encode_pcie_function_result(result: SyscallResult) -> usize {
    match result {
        Err(SyscallError::NotSupported) => api::pcie_function::ERR_NOT_SUPPORTED,
        Err(SyscallError::OutOfMemory) => api::pcie_function::ERR_NO_VECTORS,
        other => encode_ipc_result(other),
    }
}

#[cfg(not(target_arch = "riscv32"))]
pub(super) fn is_pcie_function_call(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::QueryPcieFunction { .. } | Syscall::AllocPcieIrq { .. }
    )
}

/// 436 results: each refusal keeps its own `api::syscall::system_power`
/// sentinel. Success never returns to the caller.
#[cfg(not(target_arch = "riscv32"))]
pub(super) fn encode_system_power_result(result: SyscallResult) -> usize {
    use api::syscall::system_power as sp;
    match result {
        Ok(value) => value,
        Err(SyscallError::InvalidInput) => sp::ERR_INVALID_INPUT,
        Err(SyscallError::NotSupported) => sp::ERR_NOT_SUPPORTED,
        Err(_) => sp::ERR_PERMISSION_DENIED,
    }
}

/// `WaitIrq` admission for its `mmio_base` argument.
///
/// Zero means "no InterruptACK write". A nonzero base is stored and later
/// dereferenced by the riscv64/aarch64 ISR, so it must be the exact base of an
/// MMIO window the resource registry granted to the caller.
pub(super) fn wait_irq_base_permitted(caller_id: usize, mmio_base: usize) -> bool {
    mmio_base == 0 || crate::resource_registry::lookup_mmio_owner(mmio_base) == Some(caller_id)
}

/// Measured program names allowed to execute raw opcode 502 (platform
/// power-off): the signed `/bin/shutdown` worker only.
pub(super) const RAW_SHUTDOWN_CALLERS: &[&str] = &["shutdown"];

pub(super) fn raw_shutdown_permitted(name: &str) -> bool {
    RAW_SHUTDOWN_CALLERS.contains(&name)
}

/// Gate for raw opcode 502. Runs before any platform shutdown code.
pub(super) fn authorize_raw_shutdown(caller_id: usize) -> Result<(), SyscallError> {
    let (name, _, _) =
        caller_launch_state(caller_id).unwrap_or((String::from("<unknown>"), false, false));
    if raw_shutdown_permitted(&name) {
        return Ok(());
    }
    log::warn!("[kernel] raw opcode 502 denied for {}", name);
    Err(SyscallError::PermissionDenied)
}

/// AP-PZ: allow/deny decision for raw opcode 502 over the reviewed names and
/// the live-task lookup (an unknown caller is denied without side effects).
#[cfg(feature = "test-hooks")]
pub(crate) fn test_raw_shutdown_gate() -> bool {
    raw_shutdown_permitted("shutdown")
        && !raw_shutdown_permitted("cellbox")
        && !raw_shutdown_permitted("init")
        && !raw_shutdown_permitted("supervisor")
        && !raw_shutdown_permitted("")
        && !raw_shutdown_permitted("shutdown2")
        && authorize_raw_shutdown(usize::MAX) == Err(SyscallError::PermissionDenied)
}

pub(super) fn caller_has_pcie_driver(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.pcie_driver)
}

pub(super) fn caller_has_usb_driver(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.usb_driver)
}

pub(super) fn caller_has_serial_port(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.serial_port)
}

pub(super) fn caller_has_mmio_device(caller_id: usize, device: u8) -> bool {
    caller_has_cap(caller_id, |t| t.mmio_devices & device != 0)
}

/// 423: authorize and publish the USB HID event producer role.
///
/// Deliberately separate from [`register_driver_service`]: that helper accepts
/// either `PcieDriverCap` or `UsbDriverCap` and publishes the singleton NIC
/// role. A USB host cell on a PC that already has e1000/virtio-net must hold
/// only `UsbDriverCap` and must never overwrite `service::NIC_DRIVER`, so this
/// path checks `usb_driver_cap` alone and publishes `service::USB_HID_PRODUCER`.
pub(super) fn register_usb_hid_producer_role(caller_id: usize) -> Result<usize, SyscallError> {
    let scheduler = crate::task::SCHEDULER.lock();
    let authorized = scheduler
        .as_ref()
        .and_then(|scheduler| {
            let rights = scheduler.cell_rights(caller_id);
            Some((scheduler.tasks.get(&caller_id)?, rights))
        })
        .is_some_and(|(task, rights)| {
            rights.usb_driver
                && !matches!(
                    &task.state,
                    crate::task::tcb::TaskState::Retiring | crate::task::tcb::TaskState::Terminated
                )
        });
    if !authorized {
        return Err(SyscallError::PermissionDenied);
    }

    let Some((cell_id, generation)) = scheduler
        .as_ref()
        .and_then(|s| s.tasks.get(&caller_id))
        .map(|task| (task.cell_id.0, task.cell_generation))
    else {
        return Err(SyscallError::InvalidInput);
    };

    let published = crate::task::drivers::driver_cell::publish_role_or_rollback(
        caller_id,
        crate::task::drivers::driver_cell::register_usb_hid_producer,
        || {
            crate::cell::service_registry::register(
                api::syscall::service::USB_HID_PRODUCER,
                caller_id,
                cell_id,
                generation,
            )
        },
        crate::task::drivers::driver_cell::deregister_usb_hid_producer,
    );
    drop(scheduler);
    if published {
        Ok(0)
    } else {
        Err(SyscallError::InvalidInput)
    }
}

pub(super) fn caller_has_platform(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.platform)
}

pub(super) fn pcie_usable_by(caller_id: usize, bdf: u32) -> bool {
    crate::resource_registry::owner_of_bdf(bdf).is_none_or(|owner| owner == caller_id)
}

#[cfg(target_arch = "x86_64")]
pub(super) fn pcie_function_error(
    error: crate::task::drivers::pcie_msi::PcieFunctionError,
) -> SyscallError {
    use crate::task::drivers::pcie_msi::PcieFunctionError as E;
    match error {
        E::InvalidInput => SyscallError::InvalidInput,
        E::PermissionDenied => SyscallError::PermissionDenied,
        E::NotFound => SyscallError::FileNotFound,
        E::NotSupported => SyscallError::NotSupported,
        E::NoVectors => SyscallError::OutOfMemory,
    }
}

/// 435 packed result for an owned BDF.
#[cfg(target_arch = "x86_64")]
pub(super) fn alloc_pcie_irq(
    caller_id: usize,
    bdf: usize,
    want: usize,
    mode: usize,
) -> SyscallResult {
    let bdf = u32::try_from(bdf).map_err(|_| SyscallError::InvalidInput)?;
    crate::task::drivers::pcie_msi::allocate(caller_id, bdf, want, mode)
        .map(|irq| irq.pack())
        .map_err(pcie_function_error)
}

/// MSI is x86-only (CP1 §1).
#[cfg(not(target_arch = "x86_64"))]
pub(super) fn alloc_pcie_irq(
    _caller_id: usize,
    _bdf: usize,
    _want: usize,
    _mode: usize,
) -> SyscallResult {
    Err(SyscallError::NotSupported)
}

/// 436 after the `SupervisorCap` and reserved-argument gates. Reboot does not
/// return; poweroff returns only when the machine is still running.
#[cfg(target_arch = "x86_64")]
pub(super) fn system_power(caller_id: usize, action: usize) -> SyscallResult {
    use api::syscall::system_power as sp;
    match action {
        sp::ACTION_REBOOT => {
            log::warn!(
                "[kernel] SystemPower: reboot requested by tid {}",
                caller_id
            );
            crate::acpi::power::reboot()
        }
        sp::ACTION_POWEROFF => {
            log::warn!(
                "[kernel] SystemPower: poweroff requested by tid {}",
                caller_id
            );
            crate::acpi::power::poweroff();
            Err(SyscallError::NotSupported)
        }
        _ => Err(SyscallError::InvalidInput),
    }
}

/// Reset and S5 come from ACPI, which only x86_64 parses (CP1 §5).
#[cfg(not(target_arch = "x86_64"))]
pub(super) fn system_power(_caller_id: usize, action: usize) -> SyscallResult {
    use api::syscall::system_power as sp;
    match action {
        sp::ACTION_REBOOT | sp::ACTION_POWEROFF => Err(SyscallError::NotSupported),
        _ => Err(SyscallError::InvalidInput),
    }
}

pub(super) fn register_driver_service(
    caller_id: usize,
    service_id: u16,
    publish_role: fn(usize),
    rollback_role: fn(usize),
) -> Result<usize, SyscallError> {
    let scheduler = crate::task::SCHEDULER.lock();
    let authorized = scheduler
        .as_ref()
        .and_then(|scheduler| {
            let rights = scheduler.cell_rights(caller_id);
            Some((scheduler.tasks.get(&caller_id)?, rights))
        })
        .is_some_and(|(task, rights)| {
            (rights.pcie_driver || rights.usb_driver)
                && !matches!(
                    &task.state,
                    crate::task::tcb::TaskState::Retiring | crate::task::tcb::TaskState::Terminated
                )
        });
    if !authorized {
        return Err(SyscallError::PermissionDenied);
    }

    // The identity is read under the same scheduler lock that authorized the
    // caller, so the published binding names an incarnation the kernel can stand
    // behind. A task with no live Cell identity is refused rather than recorded
    // as an unbindable route.
    let Some((cell_id, generation)) = scheduler
        .as_ref()
        .and_then(|s| s.tasks.get(&caller_id))
        .map(|task| (task.cell_id.0, task.cell_generation))
    else {
        return Err(SyscallError::InvalidInput);
    };

    let published = crate::task::drivers::driver_cell::publish_role_or_rollback(
        caller_id,
        publish_role,
        || crate::cell::service_registry::register(service_id, caller_id, cell_id, generation),
        rollback_role,
    );
    drop(scheduler);
    if published {
        Ok(0)
    } else {
        Err(SyscallError::InvalidInput)
    }
}

/// 416: bind the route of the block device the caller owns.
///
/// Checks in order, each before any effect: decode (`Malformed` →
/// `InvalidInput`), a live PcieDriverCap/UsbDriverCap caller (`PermissionDenied`),
/// ownership of the named device (`NotOwner` → `FileNotFound`), then the route
/// slot under `ROLE_PUBLICATION` (a refusal is `PermissionDenied`).
///
/// Ownership is read without SCHEDULER held: `request_mmio` takes the MMIO
/// registry before SCHEDULER, so the opposite order here could deadlock. Both
/// owner tables key the claim by the claiming task id (`RequestMmio` records
/// `CellId(tid)`), and a claim is released only at that task's retirement, so
/// it still holds when the caller is found live again under SCHEDULER, where
/// its `(cell, generation)` is read and the slot is published so exit teardown
/// cannot interleave. No service-registry entry is written.
pub(super) fn register_block_device_route(
    caller_id: usize,
    kind: usize,
    id: usize,
    reserved: (usize, usize),
) -> Result<usize, SyscallError> {
    use api::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI};
    let key = u16::try_from(kind)
        .ok()
        .filter(|_| reserved == (0, 0))
        .and_then(|kind| DeviceKey::new(kind, id as u64))
        .ok_or(SyscallError::InvalidInput)?;

    let live_driver = |scheduler: &Option<crate::task::Scheduler>| {
        scheduler.as_ref().and_then(|scheduler| {
            let rights = scheduler.cell_rights(caller_id);
            let task = scheduler.tasks.get(&caller_id)?;
            ((rights.pcie_driver || rights.usb_driver)
                && !matches!(&task.state, TaskState::Retiring | TaskState::Terminated))
            .then_some((task.cell_id.0, task.cell_generation))
        })
    };
    if live_driver(&crate::task::SCHEDULER.lock()).is_none() {
        return Err(SyscallError::PermissionDenied);
    }

    let owned = match key.kind {
        BLOCK_DEVICE_PCI => {
            crate::resource_registry::owner_of_bdf(key.id as u32) == Some(caller_id)
        }
        BLOCK_DEVICE_MMIO => {
            crate::resource_registry::lookup_mmio_owner(key.id as usize) == Some(caller_id)
        }
        _ => false,
    };
    if !owned {
        return Err(SyscallError::FileNotFound);
    }

    let scheduler = crate::task::SCHEDULER.lock();
    let Some((cell_id, generation)) = live_driver(&scheduler) else {
        return Err(SyscallError::PermissionDenied);
    };
    let published = crate::task::drivers::driver_cell::publish_block_route(
        key,
        (caller_id, cell_id, generation),
    );
    drop(scheduler);
    published.map(|_| 0).ok_or(SyscallError::PermissionDenied)
}

/// 434 record bytes for an owned BDF.
#[cfg(target_arch = "x86_64")]
pub(super) fn query_pcie_function(
    caller_id: usize,
    bdf: usize,
) -> Result<[u8; api::pcie_function::PCIE_FUNCTION_RECORD_LEN], SyscallError> {
    let bdf = u32::try_from(bdf).map_err(|_| SyscallError::InvalidInput)?;
    crate::task::drivers::pcie_msi::query(caller_id, bdf)
        .map(|record| record.encode())
        .map_err(pcie_function_error)
}

/// 434 has no producer off x86: no function is reported.
#[cfg(not(target_arch = "x86_64"))]
pub(super) fn query_pcie_function(
    _caller_id: usize,
    _bdf: usize,
) -> Result<[u8; api::pcie_function::PCIE_FUNCTION_RECORD_LEN], SyscallError> {
    Err(SyscallError::FileNotFound)
}

/// Serialize a matched PCIe device into the 48-byte `PcieDeviceInfo` wire record
/// (padded from the 44 meaningful bytes by `vendor_id`/`device_id` alignment) and
/// write it to the caller's `out_ptr`.
///
/// Shared by the class-triple (`FindPcieDevice`) and exact-identity
/// (`FindPcieDeviceByVendor`) queries so the two hand-out paths cannot drift.
/// Claims BDF ownership first and returns 0 without writing when a live Driver
/// Cell already owns the device.
pub(super) fn offer_pcie_device(
    caller_id: usize,
    out_ptr: usize,
    dev: crate::task::drivers::pcie_ecam::PciDevice,
) -> Result<usize, SyscallError> {
    let bdf = dev.canonical_bdf();
    let bar0_base = dev.bars[0].base_addr();
    let bar0_len: u64 = match dev.bars[0] {
        crate::task::drivers::pcie_ecam::Bar::Memory32 { size, .. } => size as u64,
        crate::task::drivers::pcie_ecam::Bar::Memory64 { size, .. } => size,
        _ => 0x4000, // fallback 16 KiB
    };
    // First *memory* BAR with a non-zero base, for devices whose BAR0 is I/O or
    // empty (ICH9's and VMware's AHCI decode their ABAR at BAR5). Every recorded
    // function carries its full BAR set: the boot scan decodes bus 0, and
    // `register_device` decodes a Platform-Cell-registered function (bus ≥ 1,
    // e.g. behind VMware's `pciBridge0`) the same way. `bar0_*` keep their
    // existing first-BAR meaning; NVMe/e1000 are unaffected because their BAR0
    // is memory.
    let (bar_mem_base, bar_mem_len): (u64, u64) =
        crate::task::drivers::pcie_ecam::first_memory_bar(&dev.bars)
            .map(|(base, size)| (base, if size == 0 { 0x4000 } else { size }))
            .unwrap_or((bar0_base, bar0_len));
    // Under VT-d, a virtio function without ACCESS_PLATFORM or a PVSCSI
    // adapter would DMA around the translation; no cell may own it (ADR-0031
    // phase 05 review HIGH; H-S3-13). Judged only before the first claim: the
    // virtio probe remaps the common-config page kernel-only, which must not
    // happen under a live owner's mapping.
    #[cfg(target_arch = "x86_64")]
    if crate::resource_registry::owner_of_bdf(bdf).is_none()
        && crate::task::drivers::pcie_msi::dma_bypasses_iommu(&dev)
    {
        return Ok(0);
    }
    // Claim without displacing a live Driver Cell. A competing instance observes
    // "not found" and may retry after reap.
    if !crate::resource_registry::claim_bdf_owner(bdf, caller_id) {
        return Ok(0);
    }
    // The owner may now reach its BARs: memory decoding on, bus mastering
    // still off until its first GrantDma, after the cell has reset the device.
    // Only when firmware placed every memory BAR: an unassigned one would
    // decode at address 0 (the cell then fails closed on base 0, as before).
    if crate::task::drivers::pcie_ecam::memory_bars_assigned(&dev.bars) {
        crate::task::drivers::pcie_ecam::enable_memory_decode(bdf);
    }
    if out_ptr != 0 {
        let mut dev_bytes = [0u8; 48];
        dev_bytes[0..4].copy_from_slice(&bdf.to_ne_bytes());
        dev_bytes[4..8].copy_from_slice(&1u32.to_ne_bytes());
        dev_bytes[8..16].copy_from_slice(&bar0_base.to_ne_bytes());
        dev_bytes[16..24].copy_from_slice(&bar0_len.to_ne_bytes());
        dev_bytes[24..32].copy_from_slice(&bar_mem_base.to_ne_bytes());
        dev_bytes[32..40].copy_from_slice(&bar_mem_len.to_ne_bytes());
        dev_bytes[40..42].copy_from_slice(&dev.vendor_id.to_ne_bytes());
        dev_bytes[42..44].copy_from_slice(&dev.device_id.to_ne_bytes());
        write_user_slice(caller_id, out_ptr, &dev_bytes, MAX_USER_BUF)?;
    }
    Ok(1)
}
