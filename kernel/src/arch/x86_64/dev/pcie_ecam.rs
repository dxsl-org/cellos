// SPDX-License-Identifier: MPL-2.0
//! x86_64 items of `crate::dev::pcie_ecam`, compiled as its `x86` child module
//! (`#[path]`), so they keep reading that module's private items unchanged.

use super::*;
use core::sync::atomic::AtomicUsize;

/// Whether the legacy class-triple query may hand out `(vendor_id, device_id)`.
///
/// See [`ETHERNET_LEGACY_CLASS_IDS`]: only the pre-vendor-query family.
pub(super) fn ethernet_class_query_serves(vendor_id: u16, device_id: u16) -> bool {
    ETHERNET_LEGACY_CLASS_IDS
        .iter()
        .any(|&(vendor, device)| vendor == vendor_id && device == device_id)
}

/// Runtime ECAM base for x86_64, set from ACPI MCFG parse before `init()`.
///
/// Zero means the MCFG gate is closed.
pub(super) static X86_ECAM_BASE: AtomicUsize = AtomicUsize::new(0);

/// Store the ECAM base parsed from ACPI MCFG.
///
/// Must be called before `pcie_ecam::init()` for the value to take effect.
/// No-op on non-x86_64 targets (they use compile-time constants).
pub fn set_ecam_base_x86(base: usize) {
    X86_ECAM_BASE.store(base, Ordering::Relaxed);
}

/// Return the validated runtime x86_64 ECAM base, or zero when unavailable.
#[inline]
pub(super) fn ecam_base_x86() -> usize {
    X86_ECAM_BASE.load(Ordering::Relaxed)
}

pub(super) static X86_ECAM_BUS_START: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(0);

pub(super) static X86_ECAM_BUS_END: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(0);

pub fn set_ecam_bus_range_x86(start: u8, end: u8) {
    X86_ECAM_BUS_START.store(start, Ordering::Relaxed);
    X86_ECAM_BUS_END.store(end, Ordering::Relaxed);
}

pub fn ecam_bus_range_x86() -> (u8, u8) {
    (
        X86_ECAM_BUS_START.load(Ordering::Relaxed),
        X86_ECAM_BUS_END.load(Ordering::Relaxed),
    )
}

/// [`decode_function`] for `bus:dev.fun` inside the admitted window
/// `bus_start..=bus_end`; `None` outside it or when no function answers.
///
/// # Safety
/// `ecam_base` must map config space for buses `bus_start..=bus_end`.
pub(super) unsafe fn decode_function_in_window(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bus: u8,
    dev: u8,
    fun: u8,
) -> Option<([Bar; 6], Option<MsixCap>, Option<PmCap>)> {
    // The bus's own 1 MiB window, addressed as local bus 0 (as `init` does for
    // a nonzero-start allocation).
    let bus_base = config_addr_in_range(ecam_base, bus_start, bus_end, bus, 0, 0, 0)? as usize;
    if dev >= 32 || fun >= 8 {
        return None;
    }
    // SAFETY: `bus_base` is the checked start of `bus` inside the caller's window.
    if unsafe { read16(bus_base, 0, dev, fun, CFG_VENDOR_ID) } == 0xFFFF {
        return None;
    }
    // SAFETY: as above.
    Some(unsafe { decode_function(bus_base, 0, dev, fun) })
}

/// Record `bdf` in `devices` from config space unless it is already there:
/// `(outcome, whether a record was added)`. The caller holds the
/// `PCI_DEVICES` guard for the whole call, which makes the check, the probe
/// and the insert one step per BDF.
///
/// # Safety
/// The admitted ECAM window must be identity-mapped and remain live.
pub(super) unsafe fn record_in_window(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bdf: (u8, u8, u8),
    reported_cls: u32,
    devices: &mut Vec<PciDevice>,
) -> (Registration, bool) {
    let (bus, dev, fun) = bdf;
    // SAFETY: the caller guarantees the window is live and mapped.
    let identity = unsafe { read_identity_in_range(ecam_base, bus_start, bus_end, bus, dev, fun) };
    let Some((vendor_id, device_id)) = identity.filter(|&(vendor, _)| vendor != 0xFFFF) else {
        return (Registration::Absent, false);
    };
    // SAFETY: as above.
    let Some((class, subclass, prog_if)) =
        (unsafe { class_code_in_range(ecam_base, bus_start, bus_end, bus, dev, fun) })
    else {
        return (Registration::Absent, false);
    };
    let registration = if ethernet_without_driver(class, subclass, prog_if, vendor_id, device_id) {
        Registration::RecordedWithoutDriver
    } else {
        Registration::Recorded
    };
    if devices.iter().any(|d| d.bdf == bdf) {
        return (registration, false);
    }

    // Probed only for a new BDF and under the guard: no Driver Cell can own a
    // function before it is in `PCI_DEVICES`, so the BAR size probe cannot
    // disturb a live owner, and no second registration can probe it meanwhile.
    // SAFETY: as above.
    let Some((bars, msix, pm)) =
        (unsafe { decode_function_in_window(ecam_base, bus_start, bus_end, bus, dev, fun) })
    else {
        return (Registration::Absent, false);
    };
    let class_code = (u32::from(class) << 16) | (u32::from(subclass) << 8) | u32::from(prog_if);
    if reported_cls & 0x00FF_FFFF != class_code {
        log::warn!(
            "[pcie] platform-reg {:02x}:{:02x}.{} reported class {:06x}; config space says {:06x}",
            bus,
            dev,
            fun,
            reported_cls,
            class_code
        );
    }
    let (mem_base, mem_len) = first_memory_bar(&bars).unwrap_or((0, 0));
    log::info!(
        "[pcie] platform-reg {:02x}:{:02x}.{} vendor={:04x} device={:04x} \
         class={:02x}:{:02x}:{:02x} bar0={:#x} mem={:#x}+{:#x}",
        bus,
        dev,
        fun,
        vendor_id,
        device_id,
        class,
        subclass,
        prog_if,
        bars[0].base_addr(),
        mem_base,
        mem_len
    );

    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    devices.push(PciDevice {
        bdf,
        vendor_id,
        device_id,
        class,
        subclass,
        prog_if,
        bars,
        msix,
        pm,
    });
    (registration, true)
}

/// Whether a function of this class and identity is an Ethernet controller
/// (`02:00:00`) that no Driver Cell in this image drives.
pub(super) fn ethernet_without_driver(
    class: u8,
    subclass: u8,
    prog_if: u8,
    vendor_id: u16,
    device_id: u16,
) -> bool {
    (class, subclass, prog_if) == (0x02, 0x00, 0x00)
        && !ethernet_id_has_driver(vendor_id, device_id)
}

/// `(class, subclass, prog_if)` of one checked ECAM function, from the class
/// register dword at 0x08 (revision ID, prog IF, subclass, class code).
///
/// # Safety
/// The admitted ECAM window must be identity-mapped and remain live.
pub(super) unsafe fn class_code_in_range(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bus: u8,
    dev: u8,
    fun: u8,
) -> Option<(u8, u8, u8)> {
    let class_reg = config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, 0x08)?;
    // SAFETY: the checked, 4-byte-aligned class register lies in the live
    // function header the caller guarantees.
    let class_code = unsafe { core::ptr::read_volatile(class_reg.cast::<u32>()) };
    Some((
        (class_code >> 24) as u8,
        (class_code >> 16) as u8,
        (class_code >> 8) as u8,
    ))
}

/// `(vendor, device, class, subclass, prog_if)` of every registered function,
/// for the machine fingerprint (syscall 433). Every field is re-read from
/// config space by BDF, so none comes from a Platform Cell `cls` argument.
/// `None` without an admitted ECAM window. Read-only: `PCI_DEVICES`,
/// registration and `PLATFORM_REGISTERED` are untouched.
pub fn fingerprint_functions() -> Option<Vec<abi::machine_facts::PciFunctionId>> {
    let (ecam_base, bus_start, bus_end) = ecam_window();
    if ecam_base == 0 {
        return None;
    }
    let bdfs: Vec<(u8, u8, u8)> = PCI_DEVICES.lock().iter().map(|device| device.bdf).collect();
    // SAFETY: x86 paging maps the validated MCFG allocation before any Cell
    // runs, and it stays mapped for the whole boot.
    Some(unsafe { fingerprint_in_window(ecam_base, bus_start, bus_end, &bdfs) })
}

/// Config-space identity and class code of each `bdfs` entry inside the
/// window. An entry outside the admitted bus range cannot have been
/// registered (registration reads it through the same window) and is skipped.
///
/// # Safety
/// The admitted ECAM window must be identity-mapped and remain live.
pub(super) unsafe fn fingerprint_in_window(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bdfs: &[(u8, u8, u8)],
) -> Vec<abi::machine_facts::PciFunctionId> {
    bdfs.iter()
        .filter_map(|&(bus, dev, fun)| {
            // SAFETY: the caller guarantees the window is live and mapped.
            let (vendor, device) =
                unsafe { read_identity_in_range(ecam_base, bus_start, bus_end, bus, dev, fun)? };
            // SAFETY: as above.
            let (class, subclass, prog_if) =
                unsafe { class_code_in_range(ecam_base, bus_start, bus_end, bus, dev, fun)? };
            Some(abi::machine_facts::PciFunctionId {
                vendor,
                device,
                class,
                subclass,
                prog_if,
            })
        })
        .collect()
}

/// Range-checked pointer to config byte `off` of canonical `bdf` inside the
/// admitted ECAM window; `None` outside it. The window is identity-mapped for
/// the kernel's lifetime, so the pointer stays valid; accesses must be volatile.
pub(crate) fn config_ptr(bdf: u32, off: usize) -> Option<*mut u8> {
    if bdf > 0xFFFF {
        return None;
    }
    let (ecam_base, bus_start, bus_end) = ecam_window();
    if ecam_base == 0 {
        return None;
    }
    let (bus, dev, fun) = split_bdf(bdf);
    config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, off)
}
