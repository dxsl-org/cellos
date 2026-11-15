// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: dev (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, fs::*, grant::*, ipc::*, misc::*, spawn::*, vm::*};

/// 434/435 results: the typed sentinels of [`encode_ipc_result`] plus
/// `NotSupported` and `OutOfMemory` (vectors exhausted), which a driver must
/// tell apart to fall back to polling (`abi::pcie_function::ERR_*`).
#[cfg(not(target_arch = "riscv32"))]
pub(super) fn encode_pcie_function_result(result: SyscallResult) -> usize {
    match result {
        Err(SyscallError::NotSupported) => abi::pcie_function::ERR_NOT_SUPPORTED,
        Err(SyscallError::OutOfMemory) => abi::pcie_function::ERR_NO_VECTORS,
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

/// 436 results: each refusal keeps its own `abi::syscall::system_power`
/// sentinel. Success never returns to the caller.
#[cfg(not(target_arch = "riscv32"))]
pub(super) fn encode_system_power_result(result: SyscallResult) -> usize {
    use abi::syscall::system_power as sp;
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

    let published = crate::dev::driver_cell::publish_role_or_rollback(
        caller_id,
        crate::dev::driver_cell::register_usb_hid_producer,
        || {
            crate::cell::service_registry::register(
                abi::syscall::service::USB_HID_PRODUCER,
                caller_id,
                cell_id,
                generation,
            )
        },
        crate::dev::driver_cell::deregister_usb_hid_producer,
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
    error: crate::arch::dev::pcie_msi::PcieFunctionError,
) -> SyscallError {
    use crate::arch::dev::pcie_msi::PcieFunctionError as E;
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
    crate::arch::dev::pcie_msi::allocate(caller_id, bdf, want, mode)
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
    use abi::syscall::system_power as sp;
    match action {
        sp::ACTION_REBOOT => {
            log::warn!(
                "[kernel] SystemPower: reboot requested by tid {}",
                caller_id
            );
            crate::arch::acpi::power::reboot()
        }
        sp::ACTION_POWEROFF => {
            log::warn!(
                "[kernel] SystemPower: poweroff requested by tid {}",
                caller_id
            );
            crate::arch::acpi::power::poweroff();
            Err(SyscallError::NotSupported)
        }
        _ => Err(SyscallError::InvalidInput),
    }
}

/// Reset and S5 come from ACPI, which only x86_64 parses (CP1 §5).
#[cfg(not(target_arch = "x86_64"))]
pub(super) fn system_power(_caller_id: usize, action: usize) -> SyscallResult {
    use abi::syscall::system_power as sp;
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

    let published = crate::dev::driver_cell::publish_role_or_rollback(
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
    use abi::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI};
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
    let published =
        crate::dev::driver_cell::publish_block_route(key, (caller_id, cell_id, generation));
    drop(scheduler);
    published.map(|_| 0).ok_or(SyscallError::PermissionDenied)
}

/// 434 record bytes for an owned BDF.
#[cfg(target_arch = "x86_64")]
pub(super) fn query_pcie_function(
    caller_id: usize,
    bdf: usize,
) -> Result<[u8; abi::pcie_function::PCIE_FUNCTION_RECORD_LEN], SyscallError> {
    let bdf = u32::try_from(bdf).map_err(|_| SyscallError::InvalidInput)?;
    crate::arch::dev::pcie_msi::query(caller_id, bdf)
        .map(|record| record.encode())
        .map_err(pcie_function_error)
}

/// 434 has no producer off x86: no function is reported.
#[cfg(not(target_arch = "x86_64"))]
pub(super) fn query_pcie_function(
    _caller_id: usize,
    _bdf: usize,
) -> Result<[u8; abi::pcie_function::PCIE_FUNCTION_RECORD_LEN], SyscallError> {
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
    dev: crate::dev::pcie_ecam::PciDevice,
) -> Result<usize, SyscallError> {
    let bdf = dev.canonical_bdf();
    let bar0_base = dev.bars[0].base_addr();
    let bar0_len: u64 = match dev.bars[0] {
        crate::dev::pcie_ecam::Bar::Memory32 { size, .. } => size as u64,
        crate::dev::pcie_ecam::Bar::Memory64 { size, .. } => size,
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
        crate::dev::pcie_ecam::first_memory_bar(&dev.bars)
            .map(|(base, size)| (base, if size == 0 { 0x4000 } else { size }))
            .unwrap_or((bar0_base, bar0_len));
    // Under VT-d, a virtio function without ACCESS_PLATFORM or a PVSCSI
    // adapter would DMA around the translation; no cell may own it (ADR-0031
    // phase 05 review HIGH; H-S3-13). Judged only before the first claim: the
    // virtio probe remaps the common-config page kernel-only, which must not
    // happen under a live owner's mapping.
    #[cfg(target_arch = "x86_64")]
    if crate::resource_registry::owner_of_bdf(bdf).is_none()
        && crate::arch::dev::pcie_msi::dma_bypasses_iommu(&dev)
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
    if crate::dev::pcie_ecam::memory_bars_assigned(&dev.bars) {
        crate::dev::pcie_ecam::enable_memory_decode(bdf);
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

/// Handler for `Syscall::LookupBlockRoute` (extracted from the dispatcher).
pub(super) fn lookup_block_route(
    caller_id: usize,
    index: usize,
    out_ptr: usize,
    out_len: usize,
    reserved: usize,
) -> SyscallResult {
    // Admitted past the allowlist; the VFS (BlockIoCap) and init
    // (SpawnCap) are the route readers.
    if !(caller_has_block_io(caller_id) || caller_has_spawn(caller_id)) {
        return Err(SyscallError::PermissionDenied);
    }
    let len = abi::block_route::BLOCK_ROUTE_RECORD_LEN;
    if out_len != len || reserved != 0 {
        return Err(SyscallError::InvalidInput);
    }
    let record =
        crate::dev::driver_cell::block_route_record(index).ok_or(SyscallError::FileNotFound)?;
    write_user_slice(caller_id, out_ptr, &record.encode(), len)?;
    Ok(len)
}

/// Handler for `Syscall::QueryMachineFacts` (extracted from the dispatcher).
pub(super) fn query_machine_facts(
    caller_id: usize,
    out_ptr: usize,
    out_len: usize,
    a2: usize,
    a3: usize,
) -> SyscallResult {
    // Admitted past the allowlist. The only reader is the live `MACHINE_PIN`
    // provider (ADR-0029 §3, Phase 13 amendment): the Cell trusted init
    // launched as `machine-pin` and registered (`register_service`).
    let provider = crate::cell::service_registry::lookup_bound(abi::syscall::service::MACHINE_PIN)
        .map(|(_, cell_id, generation)| (cell_id, generation));
    if provider.is_none() || live_cell_identity(caller_id) != provider {
        return Err(SyscallError::PermissionDenied);
    }
    if out_len != abi::machine_facts::MACHINE_FACTS_CAPACITY || a2 != 0 || a3 != 0 {
        return Err(SyscallError::InvalidInput);
    }
    let record = crate::machine_facts::query().ok_or(SyscallError::FileNotFound)?;
    write_user_slice(caller_id, out_ptr, &record, record.len())?;
    Ok(record.len())
}

/// Handler for `Syscall::GetCpuCount` (extracted from the dispatcher).
pub(super) fn get_cpu_count() -> SyscallResult {
    Ok(crate::sched::smp::online_hart_count())
}

/// Handler for `Syscall::GpuFlush` (extracted from the dispatcher).
pub(super) fn gpu_flush(
    caller_id: usize,
    data_ptr: usize,
    data_len: usize,
    xy: usize,
    wh: usize,
) -> SyscallResult {
    crate::task::gpu_scanout::flush(caller_id, data_ptr, data_len, xy, wh)
}

/// Handler for `Syscall::GpuCursor` (extracted from the dispatcher).
pub(super) fn gpu_cursor(
    caller_id: usize,
    op: usize,
    data_ptr: usize,
    xy: usize,
    hot: usize,
) -> SyscallResult {
    crate::task::gpu_scanout::cursor(caller_id, op, data_ptr, xy, hot)
}

/// Handler for `Syscall::GpuGetResolution` (extracted from the dispatcher).
pub(super) fn gpu_get_resolution() -> SyscallResult {
    // Firmware scanout registration is authoritative when present;
    // generic VirtIO retains the legacy fallback before its driver
    // publishes a query IPC path.
    // Packed as (width << 32) | height, which only fits a 64-bit
    // register; RV32 Nano has no GPU cell so this path never fires there.
    #[cfg(not(target_arch = "riscv32"))]
    {
        if let Some((width, height)) = crate::resource_registry::display_framebuffer_resolution() {
            return Ok(((width as usize) << 32) | height as usize);
        }
        Ok(((1280usize) << 32) | 800usize)
    }
    #[cfg(target_arch = "riscv32")]
    {
        Ok(0)
    }
}

/// Handler for `Syscall::RegisterBlockDriver` (extracted from the dispatcher).
pub(super) fn register_block_driver(
    caller_id: usize,
    kind: usize,
    id: usize,
    reserved: (usize, usize),
) -> SyscallResult {
    register_block_device_route(caller_id, kind, id, reserved)
}

/// Handler for `Syscall::RegisterNicDriver` (extracted from the dispatcher).
pub(super) fn register_nic_driver(caller_id: usize) -> SyscallResult {
    register_driver_service(
        caller_id,
        abi::syscall::service::NIC_DRIVER,
        crate::dev::driver_cell::register_nic_driver,
        crate::dev::driver_cell::deregister_nic_driver,
    )
}

/// Handler for `Syscall::RegisterUsbHidProducer` (extracted from the dispatcher).
pub(super) fn register_usb_hid_producer(caller_id: usize) -> SyscallResult {
    register_usb_hid_producer_role(caller_id)
}

/// Handler for `Syscall::FindPcieDevice` (extracted from the dispatcher).
pub(super) fn find_pcie_device(
    caller_id: usize,
    class: u8,
    subclass: u8,
    prog_if: u8,
    out_ptr: usize,
) -> SyscallResult {
    if !caller_has_pcie_driver(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if !caller_pci_query_allowed(caller_id, |program| {
        crate::catalog::pci_class_allowed(program, class, subclass, prog_if)
    }) {
        return Err(SyscallError::PermissionDenied);
    }
    let usable = |bdf| pcie_usable_by(caller_id, bdf);
    match crate::dev::pcie_ecam::find_class_where(class, subclass, prog_if, usable) {
        None => Ok(0), // device not present → VirtIO fallback
        Some(dev) => offer_pcie_device(caller_id, out_ptr, dev),
    }
}

/// Handler for `Syscall::FindPcieDeviceByVendor` (extracted from the dispatcher).
pub(super) fn find_pcie_device_by_vendor(
    caller_id: usize,
    vendor_id: u16,
    device_id: u16,
    out_ptr: usize,
) -> SyscallResult {
    if !caller_has_pcie_driver(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if !caller_pci_query_allowed(caller_id, |program| {
        crate::catalog::pci_id_allowed(program, vendor_id, device_id)
    }) {
        return Err(SyscallError::PermissionDenied);
    }
    let usable = |bdf| pcie_usable_by(caller_id, bdf);
    match crate::dev::pcie_ecam::find_vendor_device(vendor_id, device_id, usable) {
        None => Ok(0),
        Some(dev) => offer_pcie_device(caller_id, out_ptr, dev),
    }
}

/// Handler for `Syscall::QueryPcieFunction` (extracted from the dispatcher).
pub(super) fn query_pcie_function_syscall(
    caller_id: usize,
    bdf: usize,
    out_ptr: usize,
    out_len: usize,
    reserved: usize,
) -> SyscallResult {
    if !caller_has_pcie_driver(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if out_len != abi::pcie_function::PCIE_FUNCTION_RECORD_LEN || reserved != 0 {
        return Err(SyscallError::InvalidInput);
    }
    let record = query_pcie_function(caller_id, bdf)?;
    write_user_slice(caller_id, out_ptr, &record, record.len())
        .map_err(|_| SyscallError::InvalidInput)?;
    Ok(0)
}

/// Handler for `Syscall::AllocPcieIrq` (extracted from the dispatcher).
pub(super) fn alloc_pcie_irq_syscall(
    caller_id: usize,
    bdf: usize,
    want: usize,
    mode: usize,
    reserved: usize,
) -> SyscallResult {
    if !caller_has_pcie_driver(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if reserved != 0 {
        return Err(SyscallError::InvalidInput);
    }
    alloc_pcie_irq(caller_id, bdf, want, mode)
}

/// Handler for `Syscall::SystemPower` (extracted from the dispatcher).
pub(super) fn system_power_syscall(
    caller_id: usize,
    action: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if a1 != 0 || a2 != 0 || a3 != 0 {
        return Err(SyscallError::InvalidInput);
    }
    system_power(caller_id, action)
}

/// Handler for `Syscall::SerialPortInfo` (extracted from the dispatcher).
pub(super) fn serial_port_info(caller_id: usize, index: u32, out_ptr: usize) -> SyscallResult {
    if !caller_has_serial_port(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    let Some((base, irq)) = serial_port::declared(index) else {
        return Ok(0);
    };
    let present = serial_port::present(index);
    let info = [base.to_le_bytes(), [irq, present as u8]].concat();
    write_user_slice(caller_id, out_ptr, &info, info.len())?;
    Ok(present as usize)
}

/// Handler for `Syscall::SerialWrite` (extracted from the dispatcher).
pub(super) fn serial_write(caller_id: usize, index: u32, ptr: usize, len: usize) -> SyscallResult {
    if !caller_has_serial_port(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if !serial_port::present(index) {
        return Ok(usize::MAX);
    }
    // Bound one call: a cell cannot hold the console path for an
    // unbounded burst through a blocking port.
    const SERIAL_BURST_MAX: usize = 4096;
    let len = len.min(SERIAL_BURST_MAX);
    if len == 0 {
        return Ok(0);
    }
    let view = caller_copy_view(caller_id)?;
    let mut buf = [0u8; SERIAL_BURST_MAX];
    let buf = &mut buf[..len];
    view.read_into(ptr, buf)
        .map_err(|_| SyscallError::InvalidInput)?;
    let mut written = 0usize;
    for byte in buf.iter() {
        if !serial_port::write_byte(index, *byte) {
            break;
        }
        written += 1;
    }
    Ok(written)
}

/// Handler for `Syscall::SerialRead` (extracted from the dispatcher).
pub(super) fn serial_read(caller_id: usize, index: u32, ptr: usize, len: usize) -> SyscallResult {
    if !caller_has_serial_port(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if !serial_port::present(index) {
        return Ok(usize::MAX);
    }
    const SERIAL_BURST_MAX: usize = 4096;
    let len = len.min(SERIAL_BURST_MAX);
    if len == 0 {
        return Ok(0);
    }
    let mut buf = [0u8; SERIAL_BURST_MAX];
    let mut read = 0usize;
    while read < len {
        let Some(byte) = serial_port::read_byte(index) else {
            break;
        };
        buf[read] = byte;
        read += 1;
    }
    write_user_slice(caller_id, ptr, &buf[..read], len)?;
    Ok(read)
}

/// Handler for `Syscall::SerialConfigure` (extracted from the dispatcher).
pub(super) fn serial_configure(caller_id: usize, index: u32, baud: u32) -> SyscallResult {
    if !caller_has_serial_port(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if !serial_port::present(index) {
        return Ok(2);
    }
    if serial_port::configure_baud(index, baud) {
        Ok(0)
    } else {
        Ok(3)
    }
}

/// Handler for `Syscall::WaitIrq` (extracted from the dispatcher).
pub(super) fn wait_irq(caller_id: usize, irq: u8, mmio_base: usize) -> SyscallResult {
    if !caller_has_pcie_driver(caller_id)
        && !caller_has_platform(caller_id)
        && !caller_has_usb_driver(caller_id)
    {
        return Err(SyscallError::PermissionDenied);
    }
    #[cfg(target_arch = "aarch64")]
    {
        use hal::aarch64::backend::{ArmIrqController, Board};
        if let Some(usb_irq) = <Board as ArmIrqController>::USB_ONE_SHOT_IRQ {
            if caller_has_usb_driver(caller_id) && irq == usb_irq as u8 {
                // One-shot unmask: re-enable the USB legacy IRQ after the
                // driver has cleared GINTSTS.
                <Board as ArmIrqController>::unmask_usb_irq();
            }
        }
    }
    // The ISR dereferences a stored nonzero base (VirtIO InterruptACK),
    // so only an MMIO window the caller was granted is accepted.
    if !wait_irq_base_permitted(caller_id, mmio_base) {
        return Err(SyscallError::PermissionDenied);
    }
    // x86 IRQ numbers are MSI vectors from 435; a cell waits only on its
    // own, so it can neither steal nor drain another driver's interrupts.
    #[cfg(target_arch = "x86_64")]
    if !crate::arch::dev::pcie_msi::irq_owned_by(irq, caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // Lost-wakeup guard: if IRQ already fired before this call, return immediately.
    if crate::dev::irq_wait::take_pending(irq) {
        return Ok(0);
    }
    // Single-waiter policy: second caller on same IRQ gets TryAgain.
    if !crate::dev::irq_wait::register_waiter(irq, caller_id, mmio_base) {
        return Err(SyscallError::TryAgain);
    }
    // Park the task — scheduler sweep will wake it when IRQ_PENDING[irq] is set.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&caller_id) {
            task.state = crate::task::tcb::TaskState::Blocked {
                wait: crate::task::tcb::WaitKind::Completion {
                    source: abi::completion::source::IRQ,
                    delivery: crate::task::tcb::Delivery::IrqLine(irq),
                },
                deadline: None,
            };
        }
    }
    crate::task::yield_cpu();
    Ok(0)
}

/// Handler for `Syscall::RegisterPcieBar` (extracted from the dispatcher).
pub(super) fn register_pcie_bar(caller_id: usize, base: usize, len: usize) -> SyscallResult {
    if !caller_has_platform(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    crate::resource_registry::register_pcie_bar(base, len);
    // Any RegisterPcieBar call from the Platform Cell marks it as active;
    // Phase 08 uses this to gate out the kernel ECAM scan.
    crate::dev::pcie_ecam::mark_platform_registered();
    Ok(0)
}

/// Handler for `Syscall::RegisterPciDevice` (extracted from the dispatcher).
pub(super) fn register_pci_device(
    caller_id: usize,
    bdf: u32,
    cls: u32,
    bar0_base: usize,
    bar0_size: usize,
) -> SyscallResult {
    if !caller_has_platform(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    use crate::dev::pcie_ecam::{register_device, Registration};
    let registration = register_device(bdf, cls, bar0_base, bar0_size);
    crate::dev::pcie_ecam::mark_platform_registered();
    crate::dev::iommu::try_deferred_init();
    match registration {
        Registration::Recorded => Ok(0),
        Registration::RecordedWithoutDriver => Ok(1),
        Registration::Absent => Err(SyscallError::FileNotFound),
    }
}

/// Handler for `Syscall::BlkFlush` (extracted from the dispatcher).
pub(super) fn blk_flush(caller_id: usize) -> SyscallResult {
    if !caller_has_block_io(caller_id) {
        log::warn!(
            "BlkFlush denied: task {} lacks block-I/O capability",
            caller_id
        );
        return Err(SyscallError::PermissionDenied);
    }

    match crate::dev::block::flush() {
        Ok(()) => Ok(1),
        Err(_) => Ok(0),
    }
}

/// Handler for `Syscall::Shutdown` (extracted from the dispatcher).
pub(super) fn shutdown(caller_id: usize) -> SyscallResult {
    authorize_raw_shutdown(caller_id)?;
    crate::arch::power::shutdown()
}

/// Handler for `Syscall::BlkRead` (extracted from the dispatcher).
pub(super) fn blk_read(caller_id: usize, sector: u64, buf_ptr: usize) -> SyscallResult {
    if !caller_has_block_io(caller_id) {
        log::warn!(
            "BlkRead denied: task {} lacks block-I/O capability",
            caller_id
        );
        return Err(SyscallError::PermissionDenied);
    }
    // Per-cell partition range gate — a runaway FAT offset must never
    // reach kernel-owned LBAs (P2 cell table, P3 snapshot). Returns 0 = failure.
    if !check_block_access(caller_id, sector, 1) {
        return Ok(0);
    }
    validate_user_buf(buf_ptr, 512, MAX_USER_BUF)?;

    // Bounce buffer: VirtioHal::share() treats the buffer's virtual address
    // as its physical address (identity-map assumption). Stack frames ARE
    // identity-mapped; ELF BSS/heap pages are NOT — DMA would land at the
    // wrong physical address without the bounce. Read into an on-stack buffer
    // (always identity-mapped), then copy to the user buffer under SUM=1.
    let mut bounce = [0u8; 512];
    match crate::dev::block::read_sector(sector, &mut bounce) {
        Ok(()) => {
            write_user_slice(caller_id, buf_ptr, &bounce, MAX_USER_BUF)?;
            Ok(1)
        }
        Err(_) => Ok(0),
    }
}

/// Handler for `Syscall::BlkWrite` (extracted from the dispatcher).
pub(super) fn blk_write(caller_id: usize, sector: u64, buf_ptr: usize) -> SyscallResult {
    if !caller_has_block_io(caller_id) {
        log::warn!(
            "BlkWrite denied: task {} lacks block-I/O capability",
            caller_id
        );
        return Err(SyscallError::PermissionDenied);
    }
    // Per-cell partition range gate — prevents a cell from corrupting
    // the loader's table or the snapshot region. Returns 0 = failure.
    if !check_block_access(caller_id, sector, 1) {
        return Ok(0);
    }
    let user = read_user_slice(caller_id, buf_ptr, 512, MAX_USER_BUF)?;
    let mut bounce = [0u8; 512];
    bounce.copy_from_slice(&user);
    match crate::dev::block::write_sector(sector, &bounce) {
        Ok(()) => Ok(1),
        Err(error) => {
            log::warn!("[blk] write LBA={} failed: {:?}", sector, error);
            Ok(0)
        }
    }
}

/// Handler for `Syscall::RegisterDisplayFramebuffer` (extracted from the dispatcher).
pub(super) fn register_display_framebuffer(
    caller_id: usize,
    base: usize,
    size: usize,
    packed_dimensions: usize,
    pitch: usize,
) -> SyscallResult {
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    {
        const MAX_DIMENSION: usize = 8192;
        const PAGE_SIZE: usize = 4096;
        // AArch64: only a board with a firmware display path registers a
        // framebuffer; any other board refuses before every other check.
        #[cfg(target_arch = "aarch64")]
        let Some(display) =
            <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::DISPLAY
        else {
            let _ = (base, size, packed_dimensions, pitch);
            return Err(SyscallError::Unknown);
        };
        if !caller_has_mmio_device(caller_id, crate::resource_registry::DEV_DISPLAY) {
            return Err(SyscallError::PermissionDenied);
        }
        #[cfg(target_arch = "aarch64")]
        if !crate::resource_registry::owns_exact_mmio(
            caller_id,
            display.mailbox.base,
            display.mailbox.size,
        ) {
            return Err(SyscallError::PermissionDenied);
        }
        if base == 0
            || size == 0
            || base & (PAGE_SIZE - 1) != 0
            || packed_dimensions > u32::MAX as usize
        {
            return Err(SyscallError::InvalidInput);
        }
        let width = packed_dimensions >> 16;
        let height = packed_dimensions & 0xffff;
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(SyscallError::InvalidInput);
        }
        let minimum_pitch = width.checked_mul(4).ok_or(SyscallError::InvalidInput)?;
        if pitch < minimum_pitch || pitch & 3 != 0 {
            return Err(SyscallError::InvalidInput);
        }
        let covered = pitch
            .checked_mul(height)
            .ok_or(SyscallError::InvalidInput)?;
        let end = base.checked_add(size).ok_or(SyscallError::InvalidInput)?;
        #[cfg(target_arch = "aarch64")]
        if covered > size || end > display.framebuffer_limit {
            return Err(SyscallError::InvalidInput);
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            if covered > size {
                return Err(SyscallError::InvalidInput);
            }
        }
        // The framebuffer is MMIO: it must not overlap *any* managed frame,
        // not merely sit above one end of the allocator's memory.
        //
        // `match` rather than `?`: the early return has to happen outside the
        // `guard` borrow, or the RPi3 build refuses the guard's lifetime
        // (E0597) with the borrowed allocator still in scope.
        let overlaps_managed = {
            let overlaps = crate::memory::frame::with(|allocator| {
                allocator.managed_ranges().any(|range| {
                    let range_end = range.start + range.frames * PAGE_SIZE;
                    base < range_end && range.start < end
                })
            });
            match overlaps {
                Some(overlaps) => overlaps,
                None => return Err(SyscallError::Unknown),
            }
        };
        if overlaps_managed {
            return Err(SyscallError::InvalidInput);
        }
        let rounded = size
            .checked_add(PAGE_SIZE - 1)
            .ok_or(SyscallError::InvalidInput)?
            & !(PAGE_SIZE - 1);
        let pages = rounded / PAGE_SIZE;
        let owner = types::CellId(caller_id as u64);
        match crate::resource_registry::reserve_display_framebuffer(
            owner,
            base,
            size,
            width as u16,
            height as u16,
            pitch,
        ) {
            Ok(crate::resource_registry::DisplayFramebufferReservation::ActiveReplay) => {
                return Ok(0);
            }
            Err(crate::resource_registry::DisplayFramebufferReservationError::Conflict) => {
                return Err(SyscallError::PermissionDenied);
            }
            Ok(crate::resource_registry::DisplayFramebufferReservation::Reserved) => {}
        }
        if crate::memory::paging::map_display_framebuffer_user(base, pages).is_err() {
            crate::resource_registry::cancel_display_framebuffer(
                owner,
                base,
                size,
                width as u16,
                height as u16,
                pitch,
            );
            return Err(SyscallError::Unknown);
        }
        if crate::resource_registry::activate_display_framebuffer(
            owner,
            base,
            size,
            width as u16,
            height as u16,
            pitch,
        ) {
            #[cfg(target_arch = "x86_64")]
            crate::arch::dev::fb_console::handover();
            Ok(0)
        } else {
            // This cannot be recovered safely: pages are firmware-owned
            // and remain mapped until reboot, so display stays fail-stop.
            Err(SyscallError::Unknown)
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (caller_id, base, size, packed_dimensions, pitch);
        Err(SyscallError::Unknown)
    }
}

/// Handler for `Syscall::RequestMmio` (extracted from the dispatcher).
pub(super) fn request_mmio(caller_id: usize, base: usize, len: usize) -> SyscallResult {
    // PlatformCap bypass: the Platform Cell may claim only the kernel-admitted
    // ECAM config-space window (not in PCIE_BARS), never managed RAM — the
    // claim is user-mapped. Overlap check still applies.
    // x86 SAS: the boot identity map covers MMIO kernel-only; a granted
    // window must gain PTE_USER (+PCD, NX) before the ring-3 cell touches
    // it. BAR windows may not be mapped at all yet — this also creates
    // the identity mapping.
    #[cfg(target_arch = "x86_64")]
    fn user_map(base: usize, len: usize) {
        crate::memory::paging::map_mmio_user_x86(base, len);
    }
    // RISC-V/AArch64 make an allowlisted window reachable from EL0 with the
    // *boot* identity map, so a window whose access was revoked earlier —
    // by a capability revoke, or by the self-test that exercises one — stays
    // kernel-only unless the grant re-arms it. This is the non-x86 half of
    // the same rule: a granted window is a reachable window.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    fn user_map(base: usize, len: usize) {
        let _ = crate::memory::paging::grant_mmio_user(base, len);
    }
    #[cfg(not(cellos_wide))]
    fn user_map(_base: usize, _len: usize) {}
    // x86: a PCIe BAR is user-mapped without its MSI-X table/PBA pages
    // (ADR-0031 CP1 §4.3), so a cell cannot retarget its device's
    // interrupts by rewriting table entries.
    #[cfg(target_arch = "x86_64")]
    fn user_map_pcie_bar(base: usize, len: usize) {
        for (start, size) in crate::arch::dev::pcie_msi::user_ranges(base, len) {
            crate::memory::paging::map_mmio_user_x86(start, size);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    fn user_map_pcie_bar(base: usize, len: usize) {
        user_map(base, len);
    }

    if caller_has_platform(caller_id) {
        return match crate::resource_registry::request_ecam_mmio(caller_id, base, len) {
            Ok(()) => {
                user_map(base, len);
                Ok(0)
            }
            Err(types::ViError::PermissionDenied) => Ok(1),
            Err(types::ViError::AlreadyExists) => Ok(2),
            Err(_) => Ok(3),
        };
    }
    // PCIe BAR path: cells with PcieDriverCap may claim any BAR
    // discovered during ECAM scan (registered in resource_registry::PCIE_BARS).
    if caller_has_pcie_driver(caller_id) && crate::resource_registry::is_pcie_bar(base, len) {
        return match crate::resource_registry::request_mmio(
            caller_id,
            base,
            len,
            crate::resource_registry::DEV_PCIE,
        ) {
            Ok(()) => {
                user_map_pcie_bar(base, len);
                Ok(0)
            }
            Err(types::ViError::PermissionDenied) => Ok(1),
            Err(types::ViError::AlreadyExists) => Ok(2),
            Err(_) => Ok(3),
        };
    }
    // USB Host path: cells with UsbDriverCap may claim the DWC2 MMIO aperture.
    if caller_has_usb_driver(caller_id) && crate::resource_registry::is_dwc2_mmio(base, len) {
        return match crate::resource_registry::request_dwc2_mmio(caller_id, base, len) {
            Ok(()) => {
                user_map(base, len);
                Ok(0)
            }
            Err(types::ViError::PermissionDenied) => Ok(1),
            Err(types::ViError::AlreadyExists) => Ok(2),
            Err(_) => Ok(3),
        };
    }
    // GPIO/UART path: gate on manifest-declared device classes.
    let allowed_devices = {
        let sched = crate::task::SCHEDULER.lock();
        sched
            .as_ref()
            .map_or(0, |s| s.cell_rights(caller_id).mmio_devices)
    };
    if allowed_devices == 0 {
        return Err(SyscallError::PermissionDenied);
    }
    match crate::resource_registry::request_mmio(caller_id, base, len, allowed_devices) {
        Ok(()) => {
            user_map(base, len);
            Ok(0)
        }
        Err(types::ViError::PermissionDenied) => {
            log::warn!(
                "[mmio] DENY caller={} base={:#x} len={:#x} allowed_devices={:#04x}",
                caller_id,
                base,
                len,
                allowed_devices
            );
            Ok(1)
        }
        Err(types::ViError::AlreadyExists) => Ok(2),
        Err(_) => Ok(3),
    }
}

/// Handler for `Syscall::ProbeVirtioMmio` (extracted from the dispatcher).
pub(super) fn probe_virtio_mmio(
    caller_id: usize,
    base: usize,
    reserved: (usize, usize, usize),
) -> SyscallResult {
    if !caller_has_pcie_driver(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if reserved != (0, 0, 0) {
        return Err(SyscallError::InvalidInput);
    }
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    {
        use crate::dev::virtio_common::{probe_slot, SlotProbe};
        match probe_slot(base) {
            SlotProbe::Identity(identity) => Ok(identity),
            SlotProbe::NoTransport => Err(SyscallError::FileNotFound),
            SlotProbe::NotASlot => Err(SyscallError::InvalidInput),
        }
    }
    // x86_64 reaches VirtIO over PCI: there is no MMIO slot to probe.
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = base;
        Err(SyscallError::NotSupported)
    }
}

/// Handler for `Syscall::BlkReadAsync` (extracted from the dispatcher).
pub(super) fn blk_read_async(caller_id: usize, sector: u64, grant_id: usize) -> SyscallResult {
    if !caller_has_block_io(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if !check_block_access(caller_id, sector, 1) {
        return Ok(0);
    }
    // Validate ownership and minimum size (must hold ≥ 512 bytes). A domain
    // owner's backing is excluded: it is not published at its physical address
    // in the kernel root (no identity alias on x86_64), and the kernel must not
    // write a private-root grant behind its lifecycle record.
    let buf_paddr = {
        let tbl = grant_table_lock().lock();
        tbl.as_ref()
            .and_then(|m| m.get(&grant_id))
            .filter(|g| g.owner.tid == caller_id && g.size >= 512 && g.domain.is_none())
            .map(|g| g.base())
    };
    let buf_paddr = match buf_paddr {
        Some(p) => p,
        None => return Ok(0),
    };
    // Grant pages are identity-mapped (vaddr == paddr), so DMA addresses are correct.
    // SAFETY: buf_paddr is a physically contiguous, identity-mapped grant page; valid
    // for 512 bytes of VirtIO DMA read.

    let buf = unsafe { core::slice::from_raw_parts_mut(buf_paddr as *mut u8, 512) };
    match crate::dev::block::read_sector(sector, buf) {
        Ok(()) => Ok(1), // async_id = 1 means immediately complete (Phase 04 for real async)
        Err(_) => Ok(0),
    }
}
