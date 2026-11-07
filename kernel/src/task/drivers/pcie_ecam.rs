//! PCIe ECAM (Enhanced Configuration Access Mechanism) walker.
//!
//! The kernel fallback scans one admitted bus, probing functions per the
//! header-type multi-function bit. Exposes a kernel-internal device snapshot.
//!
//! ECAM bases per machine:
//!   x86_64      : runtime ACPI MCFG (no q35 fallback)
//!   RISC-V virt : 0x3000_0000
//!   AArch64     : none. The kernel discovers no config-space window there
//!                 (devices are VirtIO MMIO), so it never touches PCI config space.
//!
//! The Platform Cell may map and enumerate the complete firmware-admitted bus
//! range. Kernel fallback enumeration remains bus-0-only on non-x86 targets.
//!
//! Call ordering: `platform::init()` → `init_kernel_paging*()` → `pcie_ecam::init()`.

use crate::sync::Spinlock;
use alloc::vec::Vec;
#[cfg(target_arch = "x86_64")]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicBool, Ordering};

/// Ethernet controller IDs that have a Driver Cell in this image, each with its
/// owning cell.
///
/// This is the **family gate**: an Ethernet device is offered to a Driver Cell
/// only when a cell drives its family, and every other Ethernet ID is still
/// rejected **by name** at scan time (`[e1000] unsupported Ethernet
/// vvvv:dddd; driver gate closed`). Widened in phase 04a of the x86 PC lane from
/// "82540EM only" to "families that have a driver cell": the 8254x family
/// (`/bin/e1000`) and the igb family (`/bin/igb`) — QEMU's 82576 model plus the
/// datasheet i210/i211 SKUs the cell's `identity.rs` lists. Phase 08 (S3) adds
/// the 82545EM to `/bin/e1000` and the 82574L to the new `/bin/e1000e`: the
/// default NICs of VMware guests, and the latter is also q35's default NIC.
/// Phase 09 (S3) adds VMware's paravirtual vmxnet3 (`/bin/vmxnet3`). PVSCSI
/// (`15AD:07C0`) is a SCSI controller (class `01:00:00`), not gated here.
///
/// The table bounds what the *kernel* offers; each cell names its own IDs
/// through the vendor:device query (424) and repeats the identity check before
/// programming a register model, so no cell can bind a foreign family by
/// accident. Every owner's `identity.rs` lists exactly its rows here.
pub const ETHERNET_DRIVER_IDS: [(u16, u16, &str); 9] = [
    (0x8086, 0x100E, "/bin/e1000"),      // 82540EM
    (0x8086, 0x100F, "/bin/e1000"),      // 82545EM copper (QEMU e1000-82545em, VMware e1000)
    (0x8086, 0x10D3, "/bin/e1000e"),     // 82574L (QEMU e1000e, q35 default, VMware e1000e)
    (0x8086, 0x10C9, "/bin/igb"),        // 82576 — QEMU's igb model
    (0x8086, 0x1533, "/bin/igb"),        // i210 copper with external flash
    (0x8086, 0x1521, "/bin/igb"),        // I350 copper (server 4-port)
    (0x1AF4, 0x1041, "/bin/virtio-net"), // virtio-net (modern), ADR-0031 phase 05
    (0x1AF4, 0x1000, "/bin/virtio-net"), // virtio-net (transitional, modern caps required)
    (0x15AD, 0x07B0, "/bin/vmxnet3"),    // VMware vmxnet3 (QEMU vmxnet3, VMware default PV NIC)
];

// Deliberately refused until the cell implements their prerequisites: flashless
// i210 (0x157B/0x157C) and i211 (0x1539) need an **iNVM** read path, and
// fibre/SerDes/SGMII (0x1536/0x1537/0x1538) need media-specific link setup; the
// 82574LA (0x10F6) and every I219 are `Target` entries of `/bin/e1000e` that no
// lane validates yet (phase 13). Widening this table without that work would
// hand a device to a cell that cannot drive it, which is worse than refusing it
// by name.

/// Ethernet IDs served by the **legacy class-triple** query (`FindPcieDevice`,
/// opcode 418).
///
/// `02:00:00` is one triple for every Ethernet family, so once a second family
/// has a Driver Cell the class query can no longer name a device uniquely: it
/// would hand the igb controller to `/bin/e1000`, which must then decline and
/// release it before `/bin/igb` can claim it — a scheduling-dependent race. The
/// fix is [`find_vendor_device`] (opcode 424), which matches exactly. `find_class`
/// therefore keeps serving only the family that predates the identity query, the
/// 82540EM, so 418's semantics stay byte-identical; every in-tree Ethernet cell,
/// `/bin/e1000` included since phase 08 (H-S3-6), uses the vendor:device query.
/// The family gate above still governs the scan warning and the vendor query.
pub const ETHERNET_LEGACY_CLASS_IDS: [(u16, u16); 1] = [(0x8086, 0x100E)];

/// The Driver Cell that owns Ethernet `(vendor_id, device_id)`, if any.
pub fn ethernet_driver_cell(vendor_id: u16, device_id: u16) -> Option<&'static str> {
    ETHERNET_DRIVER_IDS
        .iter()
        .find(|&&(vendor, device, _)| vendor == vendor_id && device == device_id)
        .map(|&(_, _, cell)| cell)
}

/// Whether `(vendor_id, device_id)` is an Ethernet family some Driver Cell in
/// this image implements. Everything else fails closed.
pub fn ethernet_id_has_driver(vendor_id: u16, device_id: u16) -> bool {
    ethernet_driver_cell(vendor_id, device_id).is_some()
}

/// Whether the legacy class-triple query may hand out `(vendor_id, device_id)`.
///
/// See [`ETHERNET_LEGACY_CLASS_IDS`]: only the pre-vendor-query family.
#[cfg(target_arch = "x86_64")]
fn ethernet_class_query_serves(vendor_id: u16, device_id: u16) -> bool {
    ETHERNET_LEGACY_CLASS_IDS
        .iter()
        .any(|&(vendor, device)| vendor == vendor_id && device == device_id)
}

/// Runtime ECAM base for x86_64, set from ACPI MCFG parse before `init()`.
///
/// Zero means the MCFG gate is closed.
#[cfg(target_arch = "x86_64")]
static X86_ECAM_BASE: AtomicUsize = AtomicUsize::new(0);

/// Store the ECAM base parsed from ACPI MCFG.
///
/// Must be called before `pcie_ecam::init()` for the value to take effect.
/// No-op on non-x86_64 targets (they use compile-time constants).
#[cfg(target_arch = "x86_64")]
pub fn set_ecam_base_x86(base: usize) {
    X86_ECAM_BASE.store(base, Ordering::Relaxed);
}

/// Return the validated runtime x86_64 ECAM base, or zero when unavailable.
#[cfg(target_arch = "x86_64")]
#[inline]
fn ecam_base_x86() -> usize {
    X86_ECAM_BASE.load(Ordering::Relaxed)
}

#[cfg(target_arch = "x86_64")]
static X86_ECAM_BUS_START: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
#[cfg(target_arch = "x86_64")]
static X86_ECAM_BUS_END: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

#[cfg(target_arch = "x86_64")]
pub fn set_ecam_bus_range_x86(start: u8, end: u8) {
    X86_ECAM_BUS_START.store(start, Ordering::Relaxed);
    X86_ECAM_BUS_END.store(end, Ordering::Relaxed);
}

#[cfg(target_arch = "x86_64")]
pub fn ecam_bus_range_x86() -> (u8, u8) {
    (
        X86_ECAM_BUS_START.load(Ordering::Relaxed),
        X86_ECAM_BUS_END.load(Ordering::Relaxed),
    )
}

/// PCIe ECAM config-space base for RISC-V virt gpex.
/// Source: QEMU virt machine DTS — `pci@30000000`.
pub const ECAM_BASE_RISCV: usize = 0x3000_0000;

/// ECAM bytes per PCI bus (32 devices × 8 functions × 4 KiB).
pub const ECAM_BYTES_PER_BUS: usize = 0x10_0000;

/// Checked byte length for an inclusive MCFG bus range.
pub fn ecam_window_size(bus_start: u8, bus_end: u8) -> Option<usize> {
    let buses = (bus_end as usize)
        .checked_sub(bus_start as usize)?
        .checked_add(1)?;
    buses.checked_mul(ECAM_BYTES_PER_BUS)
}

/// Bus 0 ECAM window size retained for fixed-base non-x86 machines.
pub const ECAM_BUS0_SIZE: usize = ECAM_BYTES_PER_BUS;

// ── Config space offsets (PCI 3.0 type-0 header) ─────────────────────────────

const CFG_VENDOR_ID: usize = 0x00;
const CFG_DEVICE_ID: usize = 0x02;
const CFG_COMMAND: usize = 0x04;
const CFG_CLASS_PROG: usize = 0x09; // Prog IF
const CFG_SUBCLASS: usize = 0x0A;
const CFG_CLASS_CODE: usize = 0x0B;
const CFG_HEADER_TYPE: usize = 0x0E;
const CFG_BAR0: usize = 0x10;
const CFG_CAP_PTR: usize = 0x34;
const CFG_STATUS: usize = 0x06;

// Command register bits.
const CMD_MEM_SPACE: u16 = 1 << 1;
const CMD_BUS_MASTER: u16 = 1 << 2;
const CMD_INTX_DISABLE: u16 = 1 << 10;

// Status register: capability list present.
const STATUS_CAP_LIST: u16 = 1 << 4;

// Capability IDs.
const CAP_ID_PM: u8 = 0x01;
const CAP_ID_MSI: u8 = 0x05;
const CAP_ID_MSIX: u8 = 0x11;

// MSI / MSI-X Message Control bits.
const MSI_ENABLE: u16 = 1 << 0;
const MSIX_FUNCTION_MASK: u16 = 1 << 14;
const MSIX_ENABLE: u16 = 1 << 15;

// ── Public types ──────────────────────────────────────────────────────────────

/// A Base Address Register decoded from PCI config space.
#[derive(Clone, Copy, Debug)]
pub enum Bar {
    /// 32-bit MMIO BAR: base address and probed size.
    Memory32 { addr: u32, size: u32 },
    /// 64-bit MMIO BAR: base address (64-bit) and probed size. Consumes two slots.
    Memory64 { addr: u64, size: u64 },
    /// I/O port BAR (not mapped by this driver; skipped).
    Io,
    /// Unused / empty BAR slot.
    None,
}

impl Bar {
    /// Physical base address of this BAR, or 0 for I/O / empty.
    pub fn base_addr(&self) -> u64 {
        match self {
            Bar::Memory32 { addr, .. } => *addr as u64,
            Bar::Memory64 { addr, .. } => *addr,
            _ => 0,
        }
    }
}

/// MSI-X capability record captured during capability-list walk.
#[derive(Clone, Copy, Debug)]
pub struct MsixCap {
    /// Offset of the MSI-X capability structure in config space.
    pub cap_offset: u8,
    /// MSI-X Message Control register (table size - 1 in bits [10:0]).
    pub msg_ctrl: u16,
}

/// PM (Power Management) capability record.
#[derive(Clone, Copy, Debug)]
pub struct PmCap {
    /// Offset of the PM capability structure in config space.
    pub cap_offset: u8,
}

/// A discovered PCI function (device + function).
#[derive(Clone, Debug)]
pub struct PciDevice {
    /// (bus, device, function).
    pub bdf: (u8, u8, u8),
    pub vendor_id: u16,
    pub device_id: u16,
    /// Class code (offset 0x0B).
    pub class: u8,
    /// Subclass (offset 0x0A).
    pub subclass: u8,
    /// Programming interface (offset 0x09).
    pub prog_if: u8,
    /// Up to 6 BARs (64-bit BARs consume two slots; slot N+1 is `Bar::None`).
    pub bars: [Bar; 6],
    /// MSI-X capability, if present in capability list.
    pub msix: Option<MsixCap>,
    /// PM capability, if present.
    pub pm: Option<PmCap>,
}

impl PciDevice {
    /// Wire BDF: `bus[15:8] | dev[7:3] | fun[2:0]`.
    pub fn canonical_bdf(&self) -> u32 {
        (self.bdf.0 as u32) << 8 | (self.bdf.1 as u32) << 3 | self.bdf.2 as u32
    }
}

// ── Global device list ────────────────────────────────────────────────────────

static PCI_DEVICES: Spinlock<Vec<PciDevice>> = Spinlock::new(Vec::new());

/// Set to `true` once the Platform Cell has completed its ECAM scan and called
/// `sys_register_pcie_bar` for all discovered BARs. Phase 08 uses this flag to
/// skip the kernel `init()` scan when the Platform Cell is the authoritative
/// source of BAR information.
static PLATFORM_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Mark the Platform Cell's ECAM registration as complete.
///
/// Called from the `RegisterPcieBar` syscall handler after the Platform Cell
/// issues its final `sys_register_pcie_bar`. Sets `PLATFORM_REGISTERED` so
/// Phase 08 can gate out the kernel ECAM scan.
pub fn mark_platform_registered() {
    PLATFORM_REGISTERED.store(true, Ordering::Release);
}

/// Return `true` if the Platform Cell has completed ECAM enumeration.
pub fn is_platform_registered() -> bool {
    PLATFORM_REGISTERED.load(Ordering::Acquire)
}

// ── ECAM MMIO accessors ───────────────────────────────────────────────────────

/// Compute an MMIO address relative to an admitted inclusive bus range.
#[inline(always)]
fn config_addr_in_range(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bus: u8,
    dev: u8,
    fun: u8,
    off: usize,
) -> Option<*mut u8> {
    if bus < bus_start || bus > bus_end || dev >= 32 || fun >= 8 || off >= 0x1000 {
        return None;
    }
    let bus_offset = (bus as usize)
        .checked_sub(bus_start as usize)?
        .checked_mul(ECAM_BYTES_PER_BUS)?;
    let offset = bus_offset
        .checked_add((dev as usize) << 15)?
        .checked_add((fun as usize) << 12)?
        .checked_add(off)?;
    Some(ecam_base.checked_add(offset)? as *mut u8)
}

/// Read vendor/device identity from one checked ECAM function.
///
/// # Safety
/// The admitted ECAM window must be identity-mapped and remain live.
#[allow(dead_code)]
unsafe fn read_identity_in_range(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bus: u8,
    dev: u8,
    fun: u8,
) -> Option<(u16, u16)> {
    let ptr = config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, CFG_VENDOR_ID)?;
    // SAFETY: the checked function header lies in the caller-provided live ECAM window.
    let vendor = unsafe { core::ptr::read_volatile(ptr.cast::<u16>()) };
    // SAFETY: device ID immediately follows vendor ID in the same function header.
    let device = unsafe { core::ptr::read_volatile(ptr.add(CFG_DEVICE_ID).cast::<u16>()) };
    Some((vendor, device))
}

/// Compute MMIO address for the fixed bus-0 kernel fallback scanners.
#[inline(always)]
fn config_addr(ecam_base: usize, bus: u8, dev: u8, fun: u8, off: usize) -> *mut u8 {
    config_addr_in_range(ecam_base, 0, u8::MAX, bus, dev, fun, off)
        .expect("PCI ECAM address is bounded")
}

/// Read a u32 from PCI config space via ECAM MMIO.
///
/// # Safety
/// `config_addr(ecam_base, bus, dev, fun, off)` must point into the identity-
/// mapped ECAM window established by `init_kernel_paging*()`.  `off` must be
/// 4-byte-aligned.
unsafe fn read32(ecam_base: usize, bus: u8, dev: u8, fun: u8, off: usize) -> u32 {
    // SAFETY: caller guarantees the ECAM window is identity-mapped.
    let ptr = config_addr(ecam_base, bus, dev, fun, off) as *const u32;
    // SAFETY: volatile prevents the compiler from optimising away hardware reads.
    unsafe { core::ptr::read_volatile(ptr) }
}

/// Read a u16 from PCI config space via ECAM MMIO.
///
/// # Safety
/// Same contract as `read32`; `off` must be 2-byte-aligned.
unsafe fn read16(ecam_base: usize, bus: u8, dev: u8, fun: u8, off: usize) -> u16 {
    // SAFETY: caller guarantees the ECAM window is identity-mapped.
    let ptr = config_addr(ecam_base, bus, dev, fun, off) as *const u16;
    // SAFETY: volatile prevents optimisation of hardware register reads.
    unsafe { core::ptr::read_volatile(ptr) }
}

/// Read a u8 from PCI config space via ECAM MMIO.
///
/// # Safety
/// Same contract as `read32`.
unsafe fn read8(ecam_base: usize, bus: u8, dev: u8, fun: u8, off: usize) -> u8 {
    // SAFETY: caller guarantees the ECAM window is identity-mapped.
    let ptr = config_addr(ecam_base, bus, dev, fun, off);
    // SAFETY: volatile prevents optimisation of hardware register reads.
    unsafe { core::ptr::read_volatile(ptr) }
}

/// Write a u32 to PCI config space via ECAM MMIO.
///
/// # Safety
/// Same contract as `read32`; used only for BAR size-probe writes which are
/// bounded to the currently scanned function and immediately restored.
unsafe fn write32(ecam_base: usize, bus: u8, dev: u8, fun: u8, off: usize, val: u32) {
    // SAFETY: caller guarantees the ECAM window is identity-mapped.
    let ptr = config_addr(ecam_base, bus, dev, fun, off) as *mut u32;
    // SAFETY: volatile ensures the write reaches the MMIO register.
    unsafe {
        core::ptr::write_volatile(ptr, val);
    }
}

/// Write a u16 to PCI config space.
///
/// # Safety
/// Same contract as `write32`.
unsafe fn write16(ecam_base: usize, bus: u8, dev: u8, fun: u8, off: usize, val: u16) {
    // SAFETY: caller guarantees the ECAM window is identity-mapped.
    let ptr = config_addr(ecam_base, bus, dev, fun, off) as *mut u16;
    // SAFETY: volatile ensures the write reaches the MMIO register.
    unsafe {
        core::ptr::write_volatile(ptr, val);
    }
}

// ── BAR decode ────────────────────────────────────────────────────────────────

/// Decode all BARs for a type-0 function.
///
/// The write-all-ones / read-back size-probe modifies the command register to
/// disable memory decode before each probe and restores it after, preventing
/// the device from responding to MMIO accesses during the transient probe window.
///
/// # Safety
/// `ecam_base` must be the identity-mapped ECAM window base.
unsafe fn decode_bars(ecam_base: usize, bus: u8, dev: u8, fun: u8) -> [Bar; 6] {
    let mut bars = [Bar::None; 6];
    let mut i = 0usize;
    while i < 6 {
        let off = CFG_BAR0 + i * 4;
        // SAFETY: ECAM window is identity-mapped; off is within type-0 header.
        let raw = unsafe { read32(ecam_base, bus, dev, fun, off) };
        if raw & 1 == 1 {
            // I/O BAR — skip.
            bars[i] = Bar::Io;
            i += 1;
            continue;
        }
        let bar_type = (raw >> 1) & 0x3; // bits [2:1]
        let addr32 = raw & !0xF;

        if bar_type == 0x2 {
            // 64-bit MMIO: base spans this slot + next.
            if i + 1 >= 6 {
                bars[i] = Bar::None;
                i += 1;
                continue;
            }
            let off_hi = CFG_BAR0 + (i + 1) * 4;
            // SAFETY: ECAM window is identity-mapped.
            let raw_hi = unsafe { read32(ecam_base, bus, dev, fun, off_hi) };
            let addr64 = (addr32 as u64) | ((raw_hi as u64) << 32);
            let size64 = unsafe { probe_bar_size64(ecam_base, bus, dev, fun, i) };
            bars[i] = Bar::Memory64 {
                addr: addr64,
                size: size64,
            };
            bars[i + 1] = Bar::None; // second slot consumed
            i += 2;
        } else {
            // 32-bit MMIO.
            let size32 = unsafe { probe_bar_size32(ecam_base, bus, dev, fun, i) };
            bars[i] = Bar::Memory32 {
                addr: addr32,
                size: size32,
            };
            i += 1;
        }
    }
    bars
}

/// Probe size of a 32-bit MMIO BAR via the write-all-ones / read-back method.
///
/// Saves/restores both the command register (disabling memory decode during the
/// probe) and the BAR value so the device is left in its original state.
///
/// # Safety
/// `ecam_base` must be the identity-mapped ECAM window base.
unsafe fn probe_bar_size32(ecam_base: usize, bus: u8, dev: u8, fun: u8, bar_idx: usize) -> u32 {
    let off = CFG_BAR0 + bar_idx * 4;
    // Save original values.
    // SAFETY: ECAM window is identity-mapped.
    let orig_cmd = unsafe { read16(ecam_base, bus, dev, fun, CFG_COMMAND) };
    // SAFETY: ECAM window is identity-mapped.
    let orig_bar = unsafe { read32(ecam_base, bus, dev, fun, off) };

    // Disable memory decode before writing all-ones to BAR.
    // This prevents the device from claiming MMIO during the probe window.
    // SAFETY: writing command register disables decode only for this function.
    unsafe {
        write16(
            ecam_base,
            bus,
            dev,
            fun,
            CFG_COMMAND,
            orig_cmd & !CMD_MEM_SPACE,
        );
    }

    // Write all-ones to BAR.
    // SAFETY: ECAM window is identity-mapped; BAR write is bounded to this function.
    unsafe {
        write32(ecam_base, bus, dev, fun, off, 0xFFFF_FFFF);
    }
    // SAFETY: ECAM window is identity-mapped.
    let readback = unsafe { read32(ecam_base, bus, dev, fun, off) };

    // Restore BAR and command register.
    // SAFETY: ECAM window is identity-mapped.
    unsafe {
        write32(ecam_base, bus, dev, fun, off, orig_bar);
    }
    unsafe {
        write16(ecam_base, bus, dev, fun, CFG_COMMAND, orig_cmd);
    }

    // Size = ~(readback & mask) + 1 where mask = 0xFFFFFFF0 (clear lower 4 bits).
    let mask = readback & 0xFFFF_FFF0;
    if mask == 0 {
        return 0;
    }
    (!mask).wrapping_add(1)
}

/// Probe size of a 64-bit MMIO BAR (both low and high dwords).
///
/// # Safety
/// `ecam_base` must be the identity-mapped ECAM window base.
unsafe fn probe_bar_size64(ecam_base: usize, bus: u8, dev: u8, fun: u8, bar_idx: usize) -> u64 {
    let off_lo = CFG_BAR0 + bar_idx * 4;
    let off_hi = CFG_BAR0 + (bar_idx + 1) * 4;

    // SAFETY: ECAM window is identity-mapped.
    let orig_cmd = unsafe { read16(ecam_base, bus, dev, fun, CFG_COMMAND) };
    // SAFETY: ECAM window is identity-mapped.
    let orig_lo = unsafe { read32(ecam_base, bus, dev, fun, off_lo) };
    // SAFETY: ECAM window is identity-mapped.
    let orig_hi = unsafe { read32(ecam_base, bus, dev, fun, off_hi) };

    // Disable memory decode during probe.
    // SAFETY: ECAM window is identity-mapped.
    unsafe {
        write16(
            ecam_base,
            bus,
            dev,
            fun,
            CFG_COMMAND,
            orig_cmd & !CMD_MEM_SPACE,
        );
    }

    // Write all-ones to both halves.
    // SAFETY: ECAM window is identity-mapped.
    unsafe {
        write32(ecam_base, bus, dev, fun, off_lo, 0xFFFF_FFFF);
    }
    unsafe {
        write32(ecam_base, bus, dev, fun, off_hi, 0xFFFF_FFFF);
    }

    // SAFETY: ECAM window is identity-mapped.
    let rb_lo = unsafe { read32(ecam_base, bus, dev, fun, off_lo) };
    // SAFETY: ECAM window is identity-mapped.
    let rb_hi = unsafe { read32(ecam_base, bus, dev, fun, off_hi) };

    // Restore.
    // SAFETY: ECAM window is identity-mapped.
    unsafe {
        write32(ecam_base, bus, dev, fun, off_lo, orig_lo);
    }
    unsafe {
        write32(ecam_base, bus, dev, fun, off_hi, orig_hi);
    }
    unsafe {
        write16(ecam_base, bus, dev, fun, CFG_COMMAND, orig_cmd);
    }

    let mask64 = ((rb_hi as u64) << 32) | ((rb_lo & 0xFFFF_FFF0) as u64);
    if mask64 == 0 {
        return 0;
    }
    (!mask64).wrapping_add(1)
}

// ── Capability list walk ──────────────────────────────────────────────────────

/// Walk the capability list for a type-0 function; return MSI-X and PM caps.
///
/// # Safety
/// `ecam_base` must be the identity-mapped ECAM window base.
unsafe fn walk_caps(
    ecam_base: usize,
    bus: u8,
    dev: u8,
    fun: u8,
) -> (Option<MsixCap>, Option<PmCap>) {
    let mut msix = None;
    let mut pm = None;

    // Capability list is present only when status bit 4 is set.
    // SAFETY: ECAM window is identity-mapped.
    let status = unsafe { read16(ecam_base, bus, dev, fun, CFG_STATUS) };
    if status & (1 << 4) == 0 {
        return (None, None);
    }

    // SAFETY: ECAM window is identity-mapped.
    let mut cap_ptr = unsafe { read8(ecam_base, bus, dev, fun, CFG_CAP_PTR) } & 0xFC;
    let mut budget = 64u8; // guard against malformed circular lists

    while cap_ptr != 0 && budget > 0 {
        budget -= 1;
        // SAFETY: ECAM window is identity-mapped.
        let cap_id = unsafe { read8(ecam_base, bus, dev, fun, cap_ptr as usize) };
        // SAFETY: ECAM window is identity-mapped.
        let next = unsafe { read8(ecam_base, bus, dev, fun, cap_ptr as usize + 1) } & 0xFC;

        match cap_id {
            CAP_ID_MSIX => {
                // SAFETY: ECAM window is identity-mapped.
                let msg_ctrl = unsafe { read16(ecam_base, bus, dev, fun, cap_ptr as usize + 2) };
                msix = Some(MsixCap {
                    cap_offset: cap_ptr,
                    msg_ctrl,
                });
            }
            CAP_ID_PM => {
                pm = Some(PmCap {
                    cap_offset: cap_ptr,
                });
            }
            _ => {}
        }
        cap_ptr = next;
    }

    (msix, pm)
}

/// BARs (type-0 headers only), MSI-X and PM capabilities of one function: the
/// record the boot scan keeps, so a function the Platform Cell registers later
/// is recorded exactly like a scanned one.
///
/// # Safety
/// `ecam_base + (config_bus << 20)` must map the function's bus window.
unsafe fn decode_function(
    ecam_base: usize,
    config_bus: u8,
    dev: u8,
    fun: u8,
) -> ([Bar; 6], Option<MsixCap>, Option<PmCap>) {
    // SAFETY (all reads below): the caller guarantees the bus window is mapped.
    let hdr = unsafe { read8(ecam_base, config_bus, dev, fun, CFG_HEADER_TYPE) } & 0x7F;
    // Decode BARs for type-0 headers (endpoints). Skip bridges (type-1).
    let bars = if hdr == 0 {
        unsafe { decode_bars(ecam_base, config_bus, dev, fun) }
    } else {
        [Bar::None; 6]
    };
    let (msix, pm) = unsafe { walk_caps(ecam_base, config_bus, dev, fun) };
    (bars, msix, pm)
}

/// [`decode_function`] for `bus:dev.fun` inside the admitted window
/// `bus_start..=bus_end`; `None` outside it or when no function answers.
///
/// # Safety
/// `ecam_base` must map config space for buses `bus_start..=bus_end`.
#[cfg(target_arch = "x86_64")]
unsafe fn decode_function_in_window(
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

// ── Scanner ───────────────────────────────────────────────────────────────────

/// Scan one bus of the ECAM window and populate the global device list.
///
/// # Safety
/// `ecam_base + (config_bus << 20)` must point to the identity-mapped ECAM
/// window for `reported_bus`. Nonzero-start allocations use `config_bus = 0`
/// because their allocation base already denotes the first admitted bus.
unsafe fn scan(ecam_base: usize, config_bus: u8, reported_bus: u8) {
    let mut devices = PCI_DEVICES.lock();

    for dev in 0u8..32 {
        // SAFETY: the selected ECAM bus window is identity-mapped.
        let vendor = unsafe { read16(ecam_base, config_bus, dev, 0, CFG_VENDOR_ID) };
        if vendor == 0xFFFF {
            continue;
        }

        // SAFETY: the selected ECAM bus window is identity-mapped.
        let hdr_type = unsafe { read8(ecam_base, config_bus, dev, 0, CFG_HEADER_TYPE) };
        let max_fun: u8 = if hdr_type & 0x80 != 0 { 8 } else { 1 };

        for fun in 0u8..max_fun {
            // SAFETY: the selected ECAM bus window is identity-mapped.
            let vid = unsafe { read16(ecam_base, config_bus, dev, fun, CFG_VENDOR_ID) };
            if vid == 0xFFFF {
                continue;
            }

            // SAFETY: the selected ECAM bus window is identity-mapped.
            let did = unsafe { read16(ecam_base, config_bus, dev, fun, CFG_DEVICE_ID) };
            let prog_if = unsafe { read8(ecam_base, config_bus, dev, fun, CFG_CLASS_PROG) };
            let subclass = unsafe { read8(ecam_base, config_bus, dev, fun, CFG_SUBCLASS) };
            let class = unsafe { read8(ecam_base, config_bus, dev, fun, CFG_CLASS_CODE) };

            // SAFETY: the selected ECAM bus window is identity-mapped.
            let (bars, msix, pm) = unsafe { decode_function(ecam_base, config_bus, dev, fun) };

            let bar0_addr = bars[0].base_addr();
            log::info!(
                "[pcie] {:02x}:{:02x}.{} vendor={:04x} device={:04x} \
                 class={:02x}:{:02x}:{:02x} bar0={:#x}",
                reported_bus,
                dev,
                fun,
                vid,
                did,
                class,
                subclass,
                prog_if,
                bar0_addr
            );
            if class == 0x02
                && subclass == 0x00
                && prog_if == 0x00
                && !ethernet_id_has_driver(vid, did)
            {
                log::warn!(
                    "[e1000] unsupported Ethernet {:04x}:{:04x}; driver gate closed",
                    vid,
                    did
                );
            }

            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::DRIVER_SHARED,
            );
            devices.push(PciDevice {
                bdf: (reported_bus, dev, fun),
                vendor_id: vid,
                device_id: did,
                class,
                subclass,
                prog_if,
                bars,
                msix,
                pm,
            });
        }
    }
}

/// The admitted ECAM window: `(base, first bus, last bus)`; base 0 = none.
///
/// Only a window the kernel maps and knows to be backed counts. AArch64 has none:
/// QEMU `virt` moves ECAM above 4 GiB whenever highmem ECAM is on (the default for
/// 64-bit CPUs), and the kernel neither reads the DTB `pcie` node nor scans there,
/// so a config access at the board profile's low-ECAM guess (`0x3F00_0000`) hits
/// nothing and takes a synchronous external abort.
fn ecam_window() -> (usize, u8, u8) {
    #[cfg(target_arch = "x86_64")]
    let window = {
        let (start, end) = ecam_bus_range_x86();
        (ecam_base_x86(), start, end)
    };
    #[cfg(target_arch = "riscv64")]
    let window = (ECAM_BASE_RISCV, 0u8, 0u8);
    #[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64")))]
    let window = (0usize, 0u8, 0u8);
    window
}

/// Whether the kernel has a config-space window to quiesce or enable functions
/// through. Without one, [`quiesce_function`] fails closed for every BDF.
pub fn config_space_admitted() -> bool {
    ecam_window().0 != 0
}

/// The admitted ECAM window as a byte range `(base, len)`, or `None` when the
/// kernel has none. This is the only physical range the Platform Cell's
/// `RequestMmio` bypass may claim.
pub fn admitted_ecam_range() -> Option<(usize, usize)> {
    let (base, bus_start, bus_end) = ecam_window();
    if base == 0 {
        return None;
    }
    let len = ecam_window_size(bus_start, bus_end)?;
    base.checked_add(len).map(|_| (base, len))
}

fn split_bdf(bdf: u32) -> (u8, u8, u8) {
    (
        ((bdf >> 8) & 0xFF) as u8,
        ((bdf >> 3) & 0x1F) as u8,
        (bdf & 0x07) as u8,
    )
}

/// Enable MMIO decoding and bus mastering after the caller has established
/// exclusive BDF ownership and an IOMMU mapping. Also clears the INTx Disable
/// bit that [`quiesce_function`] set when a previous owner retired, so a new
/// owner receives the function in the state it had before any quiescence.
pub fn enable_bus_master(bdf: u32) {
    let (bus, dev, fun) = split_bdf(bdf);
    let (ecam_base, bus_start, bus_end) = ecam_window();
    if ecam_base == 0 {
        return;
    }
    let Some(command_ptr) =
        config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, CFG_COMMAND)
    else {
        log::warn!(
            "[pcie] refusing bus-master enable outside ECAM range: {:02x}:{:02x}.{}",
            bus,
            dev,
            fun
        );
        return;
    };

    // SAFETY: the complete admitted ECAM window is identity-mapped before
    // Driver Cells run, and command_ptr was checked against that exact range.
    unsafe {
        let command = core::ptr::read_volatile(command_ptr as *const u16);
        core::ptr::write_volatile(
            command_ptr as *mut u16,
            (command | CMD_MEM_SPACE | CMD_BUS_MASTER) & !CMD_INTX_DISABLE,
        );
    }
}

/// Command register once a Driver Cell owns the function: memory decoding on,
/// every other bit as it was. Bus mastering stays off until `GrantDma`
/// ([`enable_bus_master`]), so a device the cell has not reset yet cannot DMA.
const fn decoding_command(command: u16) -> u16 {
    command | CMD_MEM_SPACE
}

/// Turn on memory decoding (only) for a function a Driver Cell has just
/// claimed, so its first register access does not depend on firmware having
/// enabled decoding. VMware's EFI leaves it off on a function it has no driver
/// for (vmxnet3), and BAR reads then return all-ones.
pub fn enable_memory_decode(bdf: u32) {
    let (ecam_base, bus_start, bus_end) = ecam_window();
    if ecam_base == 0 {
        return;
    }
    // SAFETY: the admitted ECAM window is identity-mapped for the kernel's life.
    if !unsafe { enable_memory_decode_in_window(ecam_base, bus_start, bus_end, bdf) } {
        let (bus, dev, fun) = split_bdf(bdf);
        log::warn!(
            "[pcie] refusing memory-decode enable outside ECAM range: {bus:02x}:{dev:02x}.{fun}"
        );
    }
}

/// [`enable_memory_decode`] against an explicit ECAM window; `false` when the
/// BDF lies outside it. An absent function (all-ones command) is left alone.
///
/// # Safety
/// `ecam_base` must map config space for buses `bus_start..=bus_end`.
unsafe fn enable_memory_decode_in_window(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bdf: u32,
) -> bool {
    if bdf > 0xFFFF {
        return false;
    }
    let (bus, dev, fun) = split_bdf(bdf);
    let Some(ptr) = config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, CFG_COMMAND)
    else {
        return false;
    };
    // SAFETY: `ptr` was range-checked against the caller's window.
    unsafe {
        let command = core::ptr::read_volatile(ptr as *const u16);
        if command != 0xFFFF && command & CMD_MEM_SPACE == 0 {
            core::ptr::write_volatile(ptr as *mut u16, decoding_command(command));
        }
    }
    true
}

// ── Retirement quiescence ─────────────────────────────────────────────────────

/// Command register after quiescence: no new DMA, no INTx. Memory decoding is
/// kept so a later reset (or the next driver) can still reach the BARs.
const fn quiesced_command(command: u16) -> u16 {
    (command & !CMD_BUS_MASTER) | CMD_INTX_DISABLE
}

/// MSI Message Control after quiescence: MSI Enable (bit 0) cleared.
const fn quiesced_msi_control(control: u16) -> u16 {
    control & !MSI_ENABLE
}

/// MSI-X Message Control after quiescence: MSI-X Enable (bit 15) cleared and
/// Function Mask (bit 14) set, so no vector can fire even if re-enabled.
const fn quiesced_msix_control(control: u16) -> u16 {
    (control & !MSIX_ENABLE) | MSIX_FUNCTION_MASK
}

/// What `quiesce_function` left in config space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quiesced {
    /// Command register as read back.
    pub command: u16,
    /// The function has an MSI / MSI-X capability (now disabled).
    pub msi: bool,
    pub msix: bool,
    /// Some register held a live value (Bus Master, INTx, MSI or MSI-X enabled)
    /// before this call; `false` on an idempotent repeat.
    pub changed: bool,
}

/// Why a function could not be shown quiescent; its owner must keep it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuiesceError {
    /// No ECAM window, or the BDF lies outside it.
    OutOfRange,
    /// A config write did not read back as written.
    Readback { offset: u16, wrote: u16, read: u16 },
}

/// Stop a function a dead driver owned from starting DMA or raising
/// interrupts: clear Bus Master, set INTx Disable, and turn off MSI and MSI-X
/// (masking MSI-X), each confirmed by readback.
///
/// The capability list is walked here, bounded by the same 64-entry budget as
/// the scan, through range-checked config addresses; no cached or
/// driver-supplied offset is used.
///
/// Clearing Bus Master stops new requests but does not prove that writes the
/// device already posted have drained. The caller still needs the IOMMU fence
/// before it releases any frame.
pub fn quiesce_function(bdf: u32) -> Result<Quiesced, QuiesceError> {
    let (ecam_base, bus_start, bus_end) = ecam_window();
    if ecam_base == 0 {
        return Err(QuiesceError::OutOfRange);
    }
    // SAFETY: the admitted ECAM window is identity-mapped for the kernel's life.
    unsafe { quiesce_in_window(ecam_base, bus_start, bus_end, bdf) }
}

/// [`quiesce_function`] against an explicit ECAM window.
///
/// # Safety
/// `ecam_base` must map config space for buses `bus_start..=bus_end`.
unsafe fn quiesce_in_window(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bdf: u32,
) -> Result<Quiesced, QuiesceError> {
    // A BDF is 16 bits; anything above would alias another function.
    if bdf > 0xFFFF {
        return Err(QuiesceError::OutOfRange);
    }
    let (bus, dev, fun) = split_bdf(bdf);
    let addr = |off: usize| {
        config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, off)
            .ok_or(QuiesceError::OutOfRange)
    };
    // SAFETY (all accesses below): every pointer comes from
    // `config_addr_in_range` for the caller's window.
    let read16 =
        |off: usize| addr(off).map(|p| unsafe { core::ptr::read_volatile(p as *const u16) });
    let read8 = |off: usize| addr(off).map(|p| unsafe { core::ptr::read_volatile(p as *const u8) });
    let changed = core::cell::Cell::new(false);
    let update = |off: usize, next: fn(u16) -> u16| -> Result<u16, QuiesceError> {
        let ptr = addr(off)? as *mut u16;
        let before = unsafe { core::ptr::read_volatile(ptr) };
        let wrote = next(before);
        if wrote != before {
            changed.set(true);
            unsafe { core::ptr::write_volatile(ptr, wrote) };
        }
        let read = unsafe { core::ptr::read_volatile(ptr) };
        if read != wrote {
            return Err(QuiesceError::Readback {
                offset: off as u16,
                wrote,
                read,
            });
        }
        Ok(read)
    };

    // No function answers here (all-ones vendor): nothing can master the bus
    // or interrupt, and config writes would be dropped.
    if read16(CFG_VENDOR_ID)? == 0xFFFF {
        return Ok(Quiesced {
            command: 0xFFFF,
            msi: false,
            msix: false,
            changed: false,
        });
    }

    // Function first: once Bus Master is off the device cannot start new DMA,
    // whatever the interrupt capabilities below turn out to hold.
    let command = update(CFG_COMMAND, quiesced_command)?;
    let mut quiesced = Quiesced {
        command,
        msi: false,
        msix: false,
        changed: false,
    };

    if read16(CFG_STATUS)? & STATUS_CAP_LIST != 0 {
        let mut cap = read8(CFG_CAP_PTR)? & 0xFC;
        let mut budget = 64u8;
        while cap != 0 && budget > 0 {
            budget -= 1;
            let at = cap as usize;
            match read8(at)? {
                CAP_ID_MSI => {
                    quiesced.msi = true;
                    update(at + 2, quiesced_msi_control)?;
                }
                CAP_ID_MSIX => {
                    quiesced.msix = true;
                    update(at + 2, quiesced_msix_control)?;
                }
                _ => {}
            }
            cap = read8(at + 1)? & 0xFC;
        }
    }
    quiesced.changed = changed.get();
    Ok(quiesced)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Initialise the ECAM scanner.
///
/// Must be called after paging is active (ECAM window mapped) and before
/// Driver Cells are spawned (they call sys_find_pcie_device to discover their
/// endpoint). Idempotent; safe to call more than once (later calls are no-ops).
pub fn init() {
    // Determine ECAM base for the current architecture.
    // x86_64: use only the runtime value from validated ACPI MCFG.
    #[cfg(target_arch = "x86_64")]
    let ecam_base = ecam_base_x86();
    #[cfg(target_arch = "riscv64")]
    let ecam_base = ECAM_BASE_RISCV;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64")))]
    let ecam_base = 0usize; // see `ecam_window`: no discovered config space

    #[cfg(target_arch = "x86_64")]
    let scan_bus = ecam_bus_range_x86().0;
    #[cfg(not(target_arch = "x86_64"))]
    let scan_bus = 0u8;

    if ecam_base == 0 {
        log::info!("[pcie] ECAM: no PCIe on this architecture");
        return;
    }

    // If already populated (e.g. called twice), skip re-scan.
    if !PCI_DEVICES.lock().is_empty() {
        log::info!("[pcie] ECAM: already scanned, skipping");
        return;
    }
    log::info!(
        "[pcie] ECAM fallback scan bus {} @ {:#x} (1 MiB window)",
        scan_bus,
        ecam_base
    );

    // SAFETY: the first admitted ECAM bus window is identity-mapped before this
    // function is called. The MCFG allocation base denotes `scan_bus`, so
    // config-space access is relative to local bus offset zero.
    unsafe {
        scan(ecam_base, 0, scan_bus);
    }

    let count = PCI_DEVICES.lock().len();
    if count == 0 {
        log::warn!(
            "[pcie] ECAM scan found no devices on bus {} — check base and MMIO mapping",
            scan_bus
        );
    } else {
        log::info!("[pcie] ECAM scan complete: {} device(s) found", count);
    }

    // Register all discovered BAR windows in the Resource Registry so Driver
    // Cells with PcieDriverCap can call sys_request_mmio on them.
    for dev in PCI_DEVICES.lock().iter() {
        for bar in &dev.bars {
            let base = bar.base_addr() as usize;
            if base == 0 {
                continue;
            }
            // Probe size is stored in Bar::Memory32/Memory64; use a safe default
            // of 16 KiB if the probed size is 0 (identity entry).
            let len = match bar {
                Bar::Memory32 { size, .. } => {
                    if *size == 0 {
                        0x4000
                    } else {
                        *size as usize
                    }
                }
                Bar::Memory64 { size, .. } => {
                    if *size == 0 {
                        0x4000
                    } else {
                        *size as usize
                    }
                }
                _ => continue,
            };
            crate::resource_registry::register_pcie_bar(base, len);
        }
    }
}

/// What [`register_device`] (syscall 236) did with a Platform Cell registration.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registration {
    /// The function is in `PCI_DEVICES`: recorded now, or earlier by the boot
    /// scan or a previous registration.
    Recorded,
    /// Recorded exactly like [`Registration::Recorded`], so the machine
    /// fingerprint (433) sees it as it sees a bus-0 function, but it is an
    /// Ethernet controller (`02:00:00`) no Driver Cell drives
    /// ([`ethernet_id_has_driver`]): the handout queries ([`find_class_where`],
    /// [`find_vendor_device`]) never offer it.
    RecordedWithoutDriver,
    /// No function answers at the BDF inside the admitted window (or there is
    /// no window); nothing is recorded.
    Absent,
}

/// Register a device announced by the Platform Cell via `sys_register_pci_device`.
///
/// On x86 the kernel records the function from config space, exactly as the
/// boot scan records a bus-0 function: identity, class code, every BAR and its
/// capabilities are re-read by BDF through the admitted ECAM window, so `cls`
/// is only compared (a mismatch is logged) and never stored. A function behind
/// a bridge (bus ≥ 1) therefore keeps a BAR other than BAR0 — VMware's and
/// ICH9's AHCI decode their ABAR at BAR5. Every function is recorded, an
/// Ethernet ID without a Driver Cell included (that one reports
/// [`Registration::RecordedWithoutDriver`]; the queries are the only gate).
///
/// The duplicate check, the BAR size probe and the insert run under one
/// `PCI_DEVICES` guard, so two registrations of one BDF cannot both probe it or
/// both insert it; a BDF already present (the kernel ECAM scan may have found
/// it) is not probed again. Other architectures keep the Platform Cell's class
/// and BAR0. A new record also registers BAR0 in the Resource Registry for
/// `sys_request_mmio` validation.
#[must_use]
pub fn register_device(bdf: u32, cls: u32, bar0_base: usize, bar0_size: usize) -> Registration {
    // Canonical BDF wire layout everywhere (Platform Cell, FindPcieDevice,
    // GrantDma, iommu_x86): bus[15:8] | dev[7:3] | fun[2:0].
    let (bus, dev, fun) = split_bdf(bdf);

    #[cfg(target_arch = "x86_64")]
    let (registration, added) = {
        let (ecam_base, bus_start, bus_end) = ecam_window();
        if ecam_base == 0 {
            return Registration::Absent;
        }
        let mut devices = PCI_DEVICES.lock();
        // SAFETY: x86 paging maps the validated MCFG allocation before the
        // Platform Cell can issue registration syscalls.
        unsafe {
            record_in_window(
                ecam_base,
                bus_start,
                bus_end,
                (bus, dev, fun),
                cls,
                &mut devices,
            )
        }
    };

    #[cfg(not(target_arch = "x86_64"))]
    let (registration, added) = {
        let mut devices = PCI_DEVICES.lock();
        let added = !devices.iter().any(|d| d.bdf == (bus, dev, fun));
        if added {
            let (class, subclass, prog_if) = ((cls >> 16) as u8, (cls >> 8) as u8, cls as u8);
            let bars = platform_bar0(bar0_base, bar0_size);
            log::info!(
                "[pcie] platform-reg {:02x}:{:02x}.{} class={:02x}:{:02x}:{:02x} bar0={:#x}",
                bus,
                dev,
                fun,
                class,
                subclass,
                prog_if,
                bars[0].base_addr()
            );
            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::DRIVER_SHARED,
            );
            devices.push(PciDevice {
                bdf: (bus, dev, fun),
                vendor_id: 0,
                device_id: 0,
                class,
                subclass,
                prog_if,
                bars,
                msix: None,
                pm: None,
            });
        }
        (Registration::Recorded, added)
    };

    if added && bar0_base != 0 && bar0_size != 0 {
        crate::resource_registry::register_pcie_bar(bar0_base, bar0_size);
    }
    registration
}

/// Record `bdf` in `devices` from config space unless it is already there:
/// `(outcome, whether a record was added)`. The caller holds the
/// `PCI_DEVICES` guard for the whole call, which makes the check, the probe
/// and the insert one step per BDF.
///
/// # Safety
/// The admitted ECAM window must be identity-mapped and remain live.
#[cfg(target_arch = "x86_64")]
unsafe fn record_in_window(
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

    #[cfg(cellos_p01_trace)]
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
#[cfg(target_arch = "x86_64")]
fn ethernet_without_driver(
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
#[cfg(target_arch = "x86_64")]
unsafe fn class_code_in_range(
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

/// The Platform Cell's BAR0 as a BAR set: the record of a registration on an
/// architecture where the kernel does not re-read the function.
#[cfg(any(not(target_arch = "x86_64"), test))]
fn platform_bar0(bar0_base: usize, bar0_size: usize) -> [Bar; 6] {
    let mut bars = [Bar::None; 6];
    if bar0_base != 0 {
        bars[0] = if bar0_size <= u32::MAX as usize {
            Bar::Memory32 {
                addr: bar0_base as u32,
                size: bar0_size as u32,
            }
        } else {
            Bar::Memory64 {
                addr: bar0_base as u64,
                size: bar0_size as u64,
            }
        };
    }
    bars
}

/// `(base, size)` of the first memory BAR with a non-zero base. Empty BARs
/// decode as `Memory32 { addr: 0, .. }`, so the base must be checked or the
/// first absent slot wins.
pub fn first_memory_bar(bars: &[Bar; 6]) -> Option<(u64, u64)> {
    bars.iter().find_map(|bar| match *bar {
        Bar::Memory32 { addr, size } if addr != 0 => Some((addr as u64, size as u64)),
        Bar::Memory64 { addr, size } if addr != 0 => Some((addr, size)),
        _ => None,
    })
}

/// Whether firmware placed every memory BAR the function implements: at least
/// one memory BAR, and none with a size but base 0. Memory decoding is turned
/// on at claim time only then; an unassigned BAR would decode at address 0.
pub fn memory_bars_assigned(bars: &[Bar; 6]) -> bool {
    let unassigned = bars.iter().any(|bar| match *bar {
        Bar::Memory32 { addr, size } => addr == 0 && size != 0,
        Bar::Memory64 { addr, size } => addr == 0 && size != 0,
        _ => false,
    });
    !unassigned && first_memory_bar(bars).is_some()
}

/// Return a cloned snapshot of the discovered device list.
pub fn devices() -> Vec<PciDevice> {
    let devices = PCI_DEVICES.lock();
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    devices.clone()
}

/// `(vendor, device, class, subclass, prog_if)` of every registered function,
/// for the machine fingerprint (syscall 433). Every field is re-read from
/// config space by BDF, so none comes from a Platform Cell `cls` argument.
/// `None` without an admitted ECAM window. Read-only: `PCI_DEVICES`,
/// registration and `PLATFORM_REGISTERED` are untouched.
#[cfg(target_arch = "x86_64")]
pub fn fingerprint_functions() -> Option<Vec<api::machine_facts::PciFunctionId>> {
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
#[cfg(target_arch = "x86_64")]
unsafe fn fingerprint_in_window(
    ecam_base: usize,
    bus_start: u8,
    bus_end: u8,
    bdfs: &[(u8, u8, u8)],
) -> Vec<api::machine_facts::PciFunctionId> {
    bdfs.iter()
        .filter_map(|&(bus, dev, fun)| {
            // SAFETY: the caller guarantees the window is live and mapped.
            let (vendor, device) =
                unsafe { read_identity_in_range(ecam_base, bus_start, bus_end, bus, dev, fun)? };
            // SAFETY: as above.
            let (class, subclass, prog_if) =
                unsafe { class_code_in_range(ecam_base, bus_start, bus_end, bus, dev, fun)? };
            Some(api::machine_facts::PciFunctionId {
                vendor,
                device,
                class,
                subclass,
                prog_if,
            })
        })
        .collect()
}

/// Find the first supported device matching (class, subclass, prog_if).
///
/// Example: NVMe = `find_class(0x01, 0x08, 0x02)`.
///
/// The Ethernet class triple is one triple for every NIC family, so on x86 it is
/// offered only for the family that still owns it (see
/// [`ETHERNET_LEGACY_CLASS_IDS`]); a NIC cell whose controller shares the triple
/// uses [`find_vendor_device`] instead.
pub fn find_class(class: u8, subclass: u8, prog_if: u8) -> Option<PciDevice> {
    find_class_where(class, subclass, prog_if, |_| true)
}

/// [`find_class`], skipping every match whose canonical BDF fails `usable`.
///
/// Syscall 418 passes "unowned or owned by the caller" (ADR-0031 CP1 §6), so a
/// second instance of one driver reaches the next identical function. `usable`
/// runs after the device list lock is released.
pub fn find_class_where(
    class: u8,
    subclass: u8,
    prog_if: u8,
    usable: impl Fn(u32) -> bool,
) -> Option<PciDevice> {
    let matches: Vec<PciDevice> = PCI_DEVICES
        .lock()
        .iter()
        .filter(|d| {
            if d.class != class || d.subclass != subclass || d.prog_if != prog_if {
                return false;
            }
            // Ethernet spans several Driver Cells (/bin/e1000, /bin/e1000e,
            // /bin/igb, /bin/virtio-net), so the class triple cannot name a
            // device uniquely. It is served only for the legacy family; every
            // cell claims its own controller by exact vendor:device
            // (`find_vendor_device`). Binding an endpoint by generic Ethernet
            // class would program an incompatible register model, so an ID with
            // no cell stays refused here and is named in `scan`'s warning.
            #[cfg(target_arch = "x86_64")]
            if class == 0x02 && subclass == 0x00 && prog_if == 0x00 {
                return ethernet_class_query_serves(d.vendor_id, d.device_id);
            }
            true
        })
        .cloned()
        .collect();
    matches.into_iter().find(|d| usable(d.canonical_bdf()))
}

/// Find the first device with an exact `(vendor_id, device_id)` whose canonical
/// BDF passes `usable` (syscall 424 passes "unowned or owned by the caller",
/// ADR-0031 CP1 §6; `usable` runs after the device list lock is released).
///
/// This is how a Driver Cell whose controller family shares a class triple with
/// another cell (the two Ethernet cells on `02:00:00`) claims *its* device: the
/// match names the device, so no sibling has to decline and release it first.
/// The Ethernet family gate still applies, so an Ethernet ID with no Driver Cell
/// is never offered even when its vendor:device pair is queried verbatim.
pub fn find_vendor_device(
    vendor_id: u16,
    device_id: u16,
    usable: impl Fn(u32) -> bool,
) -> Option<PciDevice> {
    let matches: Vec<PciDevice> = PCI_DEVICES
        .lock()
        .iter()
        .filter(|d| {
            if d.vendor_id != vendor_id || d.device_id != device_id {
                return false;
            }
            #[cfg(target_arch = "x86_64")]
            if d.class == 0x02 && d.subclass == 0x00 && d.prog_if == 0x00 {
                return ethernet_id_has_driver(d.vendor_id, d.device_id);
            }
            true
        })
        .cloned()
        .collect();
    matches.into_iter().find(|d| usable(d.canonical_bdf()))
}

/// The scanned function at canonical `bdf`, if any.
pub fn device(bdf: u32) -> Option<PciDevice> {
    PCI_DEVICES
        .lock()
        .iter()
        .find(|d| d.canonical_bdf() == bdf)
        .cloned()
}

/// The scanned function with a memory BAR at `base`, and that BAR's index.
pub fn device_with_bar(base: u64) -> Option<(PciDevice, u8)> {
    PCI_DEVICES.lock().iter().find_map(|d| {
        d.bars
            .iter()
            .position(|bar| base != 0 && bar.base_addr() == base)
            .map(|index| (d.clone(), index as u8))
    })
}

/// Range-checked pointer to config byte `off` of canonical `bdf` inside the
/// admitted ECAM window; `None` outside it. The window is identity-mapped for
/// the kernel's lifetime, so the pointer stays valid; accesses must be volatile.
#[cfg(target_arch = "x86_64")]
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

/// Test seam: add scanned functions to the global list.
#[cfg(test)]
pub(crate) fn test_insert_devices(devices: &[PciDevice]) {
    PCI_DEVICES.lock().extend_from_slice(devices);
}

/// Test seam: drop every function of `vendor_id` from the global list.
#[cfg(test)]
pub(crate) fn test_remove_devices(vendor_id: u16) {
    PCI_DEVICES.lock().retain(|d| d.vendor_id != vendor_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecam_window_size_is_inclusive_and_rejects_reversed_ranges() {
        assert_eq!(ecam_window_size(0, 0), Some(ECAM_BYTES_PER_BUS));
        assert_eq!(ecam_window_size(0, 1), Some(2 * ECAM_BYTES_PER_BUS));
        assert_eq!(ecam_window_size(64, 65), Some(2 * ECAM_BYTES_PER_BUS));
        assert_eq!(ecam_window_size(2, 1), None);
        assert_eq!(ecam_window_size(0, u8::MAX), Some(0x1000_0000));
    }

    #[test]
    fn ecam_addressing_is_relative_to_bus_start_and_bounded() {
        let base = 0xB000_0000usize;
        assert_eq!(
            config_addr_in_range(base, 64, 65, 64, 0, 0, 0).map(|p| p as usize),
            Some(base)
        );
        assert_eq!(
            config_addr_in_range(base, 64, 65, 65, 31, 7, 0xFFC).map(|p| p as usize),
            Some(base + 2 * ECAM_BYTES_PER_BUS - 4)
        );
        assert!(config_addr_in_range(base, 64, 65, 63, 0, 0, 0).is_none());
        assert!(config_addr_in_range(base, 64, 65, 66, 0, 0, 0).is_none());
        assert!(config_addr_in_range(base, 64, 65, 64, 32, 0, 0).is_none());
        assert!(config_addr_in_range(base, 64, 65, 64, 0, 8, 0).is_none());
        assert!(config_addr_in_range(base, 64, 65, 64, 0, 0, 0x1000).is_none());
        assert!(config_addr_in_range(usize::MAX, 0, 0, 0, 0, 0, 1).is_none());
    }

    #[test]
    fn ethernet_family_gate_covers_driver_cells_only() {
        // Every admitted ID is offered to exactly its owning cell.
        assert_eq!(ethernet_driver_cell(0x8086, 0x100E), Some("/bin/e1000")); // 82540EM
        assert_eq!(ethernet_driver_cell(0x8086, 0x100F), Some("/bin/e1000")); // 82545EM
        assert_eq!(ethernet_driver_cell(0x8086, 0x10D3), Some("/bin/e1000e")); // 82574L
        assert_eq!(ethernet_driver_cell(0x8086, 0x10C9), Some("/bin/igb")); // QEMU igb
        assert_eq!(ethernet_driver_cell(0x8086, 0x1533), Some("/bin/igb")); // i210 copper
        assert_eq!(ethernet_driver_cell(0x8086, 0x1521), Some("/bin/igb")); // I350 copper
        assert_eq!(
            ethernet_driver_cell(0x1AF4, 0x1000),
            Some("/bin/virtio-net")
        );
        assert_eq!(
            ethernet_driver_cell(0x1AF4, 0x1041),
            Some("/bin/virtio-net")
        );
        assert_eq!(ethernet_driver_cell(0x15AD, 0x07B0), Some("/bin/vmxnet3")); // vmxnet3
                                                                                // Everything else stays refused (and `scan` names it): the igb SKUs whose
                                                                                // prerequisites the cell does not implement (flashless i210 and i211
                                                                                // need iNVM; fibre/SerDes/SGMII need media link setup), the e1000e
                                                                                // `Target` SKUs (82574LA, I219), an unknown Intel ID, a non-Intel
                                                                                // controller, and non-Ethernet virtio.
        for (vendor, device) in [
            (0x8086, 0x1539), // i211 — needs iNVM
            (0x8086, 0x157B), // i210 flashless — needs iNVM
            (0x8086, 0x1536), // i210 fibre — needs media setup
            (0x8086, 0x10F6), // 82574LA — e1000e Target
            (0x8086, 0x15B7), // I219-LM — e1000e Target (phase 13)
            (0x8086, 0x1011), // 82545EM fiber — not admitted
            (0x8086, 0xBEEF), // unknown Intel ID
            (0x8086, 0x0),
            (0x10EC, 0x8168), // Realtek RTL8111 — no cell
            (0x1AF4, 0x1042), // virtio-blk is not Ethernet
            (0x15AD, 0x07C0), // PVSCSI is storage, not Ethernet
            (0x15AD, 0x0720), // VMware vmxnet (v1/v2) — no cell
        ] {
            assert!(
                !ethernet_id_has_driver(vendor, device),
                "{vendor:04x}:{device:04x} must stay refused"
            );
            assert_eq!(ethernet_driver_cell(vendor, device), None);
        }
        // Each ID has exactly one owner.
        for (index, &(vendor, device, _)) in ETHERNET_DRIVER_IDS.iter().enumerate() {
            assert!(ETHERNET_DRIVER_IDS[index + 1..]
                .iter()
                .all(|&(v, d, _)| (v, d) != (vendor, device)));
        }
    }

    #[test]
    fn class_triple_serves_only_the_legacy_family() {
        // Only the 82540EM keeps the class-triple query (418 is unchanged):
        // every other family is reachable exclusively by `find_vendor_device`,
        // so the Ethernet Driver Cells cannot race for one class triple.
        assert_eq!(ETHERNET_LEGACY_CLASS_IDS, [(0x8086, 0x100E)]);
        assert!(ethernet_class_query_serves(0x8086, 0x100E));
        for &(vendor, device, _) in &ETHERNET_DRIVER_IDS {
            if (vendor, device) != (0x8086, 0x100E) {
                assert!(
                    !ethernet_class_query_serves(vendor, device),
                    "class query must not serve {vendor:04x}:{device:04x}"
                );
                // ... but the exact query still offers it, so no family is lost.
                assert!(ethernet_id_has_driver(vendor, device));
            }
        }
    }

    #[test]
    fn identity_read_uses_relative_bus_window() {
        let mut ecam = alloc::vec![0u8; 2 * ECAM_BYTES_PER_BUS];
        let function_offset = ECAM_BYTES_PER_BUS + (3usize << 15) + (2usize << 12);
        ecam[function_offset..function_offset + 2].copy_from_slice(&0x8086u16.to_ne_bytes());
        ecam[function_offset + 2..function_offset + 4].copy_from_slice(&0x100Eu16.to_ne_bytes());

        // SAFETY: the test owns a correctly sized, live in-memory ECAM stand-in.
        let identity = unsafe { read_identity_in_range(ecam.as_ptr() as usize, 64, 65, 65, 3, 2) };
        assert_eq!(identity, Some((0x8086, 0x100E)));
    }

    /// The registration outcome uses the scan's Ethernet family gate, so no
    /// second Ethernet ID list exists: only an Ethernet function without a
    /// Driver Cell reports `RecordedWithoutDriver`, on any bus.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn ethernet_without_driver_follows_the_family_table() {
        // (class, subclass, prog_if, vendor, device, without driver)
        let cases: [(u8, u8, u8, u16, u16, bool); 6] = [
            // Non-Ethernet functions are never gated, whatever their identity.
            (0x01, 0x06, 0x01, 0x15AD, 0x07E0, false), // VMware AHCI
            (0x01, 0x08, 0x02, 0x15AD, 0x07F0, false), // VMware NVMe
            (0x02, 0x80, 0x00, 0x8086, 0xBEEF, false), // other network controller
            // Ethernet: gated unless an ID has a Driver Cell.
            (0x02, 0x00, 0x00, 0x8086, 0x100E, false), // 82540EM, /bin/e1000
            (0x02, 0x00, 0x00, 0x1022, 0x2000, true),  // AMD PCnet, no cell
            (0x02, 0x00, 0x00, 0x8086, 0xBEEF, true),  // unknown Intel ID
        ];
        for (class, subclass, prog_if, vendor, device, without) in cases {
            assert_eq!(
                ethernet_without_driver(class, subclass, prog_if, vendor, device),
                without,
                "{class:02x}:{subclass:02x}:{prog_if:02x} {vendor:04x}:{device:04x}"
            );
        }
        // VMware's NICs (82545EM, 82574L, vmxnet3) follow the table, whatever
        // the phase that admits them.
        for (vendor, device) in [(0x8086, 0x100F), (0x8086, 0x10D3), (0x15AD, 0x07B0)] {
            assert_eq!(
                ethernet_without_driver(0x02, 0x00, 0x00, vendor, device),
                !ethernet_id_has_driver(vendor, device),
                "{vendor:04x}:{device:04x}"
            );
        }
    }

    /// A type-0 function at `bus:00.0` of an in-memory window that starts at
    /// bus 0: identity dword, class dword (0x08) and BAR5.
    #[cfg(target_arch = "x86_64")]
    fn put_function(ecam: &mut [u8], bus: usize, identity: u32, class: u32, bar5: u32) {
        let at = bus * ECAM_BYTES_PER_BUS;
        ecam[at..at + 0x40].fill(0);
        for (off, value) in [
            (CFG_VENDOR_ID, identity),
            (0x08, class),
            (CFG_BAR0 + 5 * 4, bar5),
        ] {
            ecam[at + off..at + off + 4].copy_from_slice(&value.to_ne_bytes());
        }
    }

    /// Syscall 236's record step over a three-bus in-memory window (`0..=2`).
    #[cfg(target_arch = "x86_64")]
    fn register_in(
        ecam: &mut [u8],
        bdf: (u8, u8, u8),
        cls: u32,
        devices: &mut Vec<PciDevice>,
    ) -> (Registration, bool) {
        assert_eq!(ecam.len(), 3 * ECAM_BYTES_PER_BUS);
        // SAFETY: the test owns the live three-bus in-memory ECAM stand-in.
        unsafe { record_in_window(ecam.as_mut_ptr() as usize, 0, 2, bdf, cls, devices) }
    }

    /// An Ethernet function without a Driver Cell behind a root port is
    /// recorded like a bus-0 function, so the machine fingerprint sees it,
    /// while the handout queries still never offer it.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn registration_records_an_ethernet_function_without_a_driver() {
        let mut ecam = alloc::vec![0xFFu8; 3 * ECAM_BYTES_PER_BUS];
        // 01:00.0 AMD PCnet 1022:2000, class 02:00:00 (revision 0x10).
        put_function(&mut ecam, 1, 0x2000_1022, 0x0200_0010, 0);
        let mut devices = Vec::new();
        assert_eq!(
            register_in(&mut ecam, (1, 0, 0), 0x02_00_00, &mut devices),
            (Registration::RecordedWithoutDriver, true)
        );
        assert_eq!(devices.len(), 1);
        let pcnet = devices[0].clone();
        assert_eq!((pcnet.vendor_id, pcnet.device_id), (0x1022, 0x2000));
        assert_eq!(
            (pcnet.class, pcnet.subclass, pcnet.prog_if),
            (0x02, 0x00, 0x00)
        );

        // SAFETY: as in `register_in`.
        let ids = unsafe { fingerprint_in_window(ecam.as_mut_ptr() as usize, 0, 2, &[pcnet.bdf]) };
        assert_eq!(
            ids,
            [api::machine_facts::PciFunctionId {
                vendor: 0x1022,
                device: 0x2000,
                class: 0x02,
                subclass: 0x00,
                prog_if: 0x00,
            }]
        );

        // The queries stay the only gate.
        test_insert_devices(&[pcnet]);
        assert!(find_vendor_device(0x1022, 0x2000, |_| true).is_none());
        assert!(find_class_where(0x02, 0x00, 0x00, |_| true).is_none_or(|d| d.vendor_id != 0x1022));
        test_remove_devices(0x1022);
    }

    /// The class code is read from config space, never taken from the
    /// Platform Cell's `cls`; a second registration of one BDF neither probes
    /// nor inserts again; an absent function records nothing.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn registration_rereads_the_class_and_records_each_bdf_once() {
        let mut ecam = alloc::vec![0xFFu8; 3 * ECAM_BYTES_PER_BUS];
        // 02:00.0 VMware AHCI 15ad:07e0, class 01:06:01, ABAR at BAR5.
        put_function(&mut ecam, 2, 0x07E0_15AD, 0x0106_0100, 0xFD5F_F000);
        let mut devices = Vec::new();
        // A `cls` claiming Ethernet does not change the record.
        assert_eq!(
            register_in(&mut ecam, (2, 0, 0), 0x02_00_00, &mut devices),
            (Registration::Recorded, true)
        );
        let ahci = &devices[0];
        assert_eq!(
            (ahci.class, ahci.subclass, ahci.prog_if),
            (0x01, 0x06, 0x01)
        );
        assert_eq!((ahci.vendor_id, ahci.device_id), (0x15AD, 0x07E0));
        assert_eq!(ahci.bars[5].base_addr(), 0xFD5F_F000);

        // Moving BAR5 shows whether a repeat registration decodes again.
        put_function(&mut ecam, 2, 0x07E0_15AD, 0x0106_0100, 0xFE00_0000);
        assert_eq!(
            register_in(&mut ecam, (2, 0, 0), 0x01_06_01, &mut devices),
            (Registration::Recorded, false)
        );
        assert_eq!(devices.len(), 1);
        assert_eq!(
            devices[0].bars[5].base_addr(),
            0xFD5F_F000,
            "not probed again"
        );

        // Nothing answers at 02:01.0, and bus 3 is outside the window.
        for bdf in [(2, 1, 0), (3, 0, 0)] {
            assert_eq!(
                register_in(&mut ecam, bdf, 0x01_06_01, &mut devices),
                (Registration::Absent, false)
            );
        }
        assert_eq!(devices.len(), 1);
    }

    /// A function behind a bridge is decoded through its own bus window and
    /// keeps a BAR other than BAR0: VMware's AHCI at 02:00.0 behind `pciBridge0`
    /// decodes its ABAR at BAR5, which a BAR0-only record lost (S3 finding S2).
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn registered_function_keeps_every_bar_of_its_bus() {
        let mut ecam = alloc::vec![0xFFu8; 3 * ECAM_BYTES_PER_BUS];
        let at = 2 * ECAM_BYTES_PER_BUS; // 02:00.0
        ecam[at..at + 0x40].fill(0);
        let mut put32 = |off: usize, value: u32| {
            ecam[at + off..at + off + 4].copy_from_slice(&value.to_ne_bytes());
        };
        put32(CFG_VENDOR_ID, 0x07E0_15AD);
        put32(0x08, 0x0106_0100); // class 01:06:01
        put32(CFG_BAR0 + 5 * 4, 0xFD5F_F000); // BAR5: 32-bit memory
        let base = ecam.as_mut_ptr() as usize;

        // SAFETY: the test owns a live three-bus in-memory ECAM stand-in.
        let (bars, msix, pm) = unsafe { decode_function_in_window(base, 0, 2, 2, 0, 0) }
            .expect("02:00.0 answers inside the window");
        assert_eq!(
            first_memory_bar(&bars).map(|(addr, _)| addr),
            Some(0xFD5F_F000)
        );
        assert!(matches!(
            bars[5],
            Bar::Memory32 {
                addr: 0xFD5F_F000,
                ..
            }
        ));
        assert_eq!(bars[0].base_addr(), 0);
        assert!(msix.is_none() && pm.is_none());
        // The size probe restored the BAR.
        let bar5 = at + CFG_BAR0 + 5 * 4;
        assert_eq!(
            u32::from_ne_bytes(ecam[bar5..bar5 + 4].try_into().unwrap()),
            0xFD5F_F000
        );

        // SAFETY: as above; nothing answers at 02:01.0, and bus 3 is outside.
        let base = ecam.as_mut_ptr() as usize;
        assert!(unsafe { decode_function_in_window(base, 0, 2, 2, 1, 0) }.is_none());
        assert!(unsafe { decode_function_in_window(base, 0, 2, 3, 0, 0) }.is_none());
    }

    #[test]
    fn first_memory_bar_skips_empty_and_io_slots() {
        let mut bars = [Bar::Memory32 { addr: 0, size: 0 }; 6];
        bars[1] = Bar::Io;
        bars[4] = Bar::Memory64 {
            addr: 0x8_0000_0000,
            size: 0x4000,
        };
        assert_eq!(first_memory_bar(&bars), Some((0x8_0000_0000, 0x4000)));
        assert_eq!(first_memory_bar(&[Bar::None; 6]), None);
        assert!(matches!(
            platform_bar0(0xFEBF_0000, 0x2_0000)[0],
            Bar::Memory32 {
                addr: 0xFEBF_0000,
                size: 0x2_0000
            }
        ));
        assert_eq!(first_memory_bar(&platform_bar0(0, 0)), None);
    }

    #[test]
    fn decoding_waits_for_every_memory_bar_to_be_assigned() {
        let mut bars = [Bar::None; 6];
        assert!(!memory_bars_assigned(&bars), "no memory BAR");
        bars[0] = Bar::Memory32 {
            addr: 0xFE01_3000,
            size: 0x1000,
        };
        bars[1] = Bar::Io;
        assert!(memory_bars_assigned(&bars));
        bars[2] = Bar::Memory64 {
            addr: 0,
            size: 0x2000,
        };
        assert!(!memory_bars_assigned(&bars), "a sized BAR at base 0");
        bars[2] = Bar::Memory64 {
            addr: 0x8_0000_0000,
            size: 0x2000,
        };
        assert!(memory_bars_assigned(&bars));
    }

    /// One in-memory function at bus 0, 00:02.0, inside a one-bus window.
    fn function_config(ecam: &mut [u8], off: usize, value: u16) {
        let at = (2usize << 15) + off;
        ecam[at..at + 2].copy_from_slice(&value.to_ne_bytes());
    }

    fn read_config(ecam: &[u8], off: usize) -> u16 {
        let at = (2usize << 15) + off;
        u16::from_ne_bytes([ecam[at], ecam[at + 1]])
    }

    #[test]
    fn quiesce_stops_dma_and_every_interrupt_path() {
        let mut ecam = alloc::vec![0u8; ECAM_BYTES_PER_BUS];
        function_config(&mut ecam, CFG_COMMAND, CMD_MEM_SPACE | CMD_BUS_MASTER);
        function_config(&mut ecam, CFG_STATUS, STATUS_CAP_LIST);
        // Capability list: 0x40 MSI (enabled) -> 0x50 PM -> 0x60 MSI-X (enabled).
        function_config(&mut ecam, CFG_CAP_PTR, 0x40);
        function_config(&mut ecam, 0x40, u16::from(CAP_ID_MSI) | (0x50 << 8));
        function_config(&mut ecam, 0x42, MSI_ENABLE | (1 << 7));
        function_config(&mut ecam, 0x50, u16::from(CAP_ID_PM) | (0x60 << 8));
        function_config(&mut ecam, 0x60, u16::from(CAP_ID_MSIX));
        function_config(&mut ecam, 0x62, MSIX_ENABLE | 0x7);

        // SAFETY: the test owns a one-bus in-memory ECAM stand-in.
        let quiesced = unsafe { quiesce_in_window(ecam.as_mut_ptr() as usize, 0, 0, 2 << 3) };
        assert_eq!(
            quiesced,
            Ok(Quiesced {
                command: CMD_MEM_SPACE | CMD_INTX_DISABLE,
                msi: true,
                msix: true,
                changed: true,
            })
        );
        assert_eq!(
            read_config(&ecam, CFG_COMMAND),
            CMD_MEM_SPACE | CMD_INTX_DISABLE
        );
        assert_eq!(
            read_config(&ecam, 0x42),
            1 << 7,
            "MSI enable cleared, other bits kept"
        );
        assert_eq!(
            read_config(&ecam, 0x62),
            MSIX_FUNCTION_MASK | 0x7,
            "MSI-X off and masked"
        );

        // A retry after an IOMMU timeout repeats quiescence; it must not report
        // a second transition.
        // SAFETY: as above.
        let repeat = unsafe { quiesce_in_window(ecam.as_mut_ptr() as usize, 0, 0, 2 << 3) };
        assert_eq!(repeat.map(|q| q.changed), Ok(false));
    }

    #[test]
    fn quiesce_refuses_a_function_outside_the_window() {
        let mut ecam = alloc::vec![0u8; ECAM_BYTES_PER_BUS];
        // SAFETY: as above; bus 1 lies outside the 0..=0 window, so nothing is touched.
        let result =
            unsafe { quiesce_in_window(ecam.as_mut_ptr() as usize, 0, 0, (1 << 8) | (2 << 3)) };
        assert_eq!(result, Err(QuiesceError::OutOfRange));
    }

    #[test]
    fn quiesce_survives_a_circular_capability_list() {
        let mut ecam = alloc::vec![0u8; ECAM_BYTES_PER_BUS];
        function_config(&mut ecam, CFG_COMMAND, CMD_BUS_MASTER);
        function_config(&mut ecam, CFG_STATUS, STATUS_CAP_LIST);
        function_config(&mut ecam, CFG_CAP_PTR, 0x40);
        function_config(&mut ecam, 0x40, u16::from(CAP_ID_PM) | (0x40 << 8));
        // SAFETY: as above.
        let quiesced = unsafe { quiesce_in_window(ecam.as_mut_ptr() as usize, 0, 0, 2 << 3) };
        assert_eq!(quiesced.map(|q| q.command), Ok(CMD_INTX_DISABLE));
    }

    #[test]
    fn quiesce_leaves_an_absent_function_and_refuses_an_overflowing_bdf() {
        let mut ecam = alloc::vec![0xFFu8; ECAM_BYTES_PER_BUS];
        let base = ecam.as_mut_ptr() as usize;
        // SAFETY: as above.
        let absent = unsafe { quiesce_in_window(base, 0, 0, 0x00FF) };
        assert_eq!(absent.map(|q| q.changed), Ok(false));
        assert!(
            ecam.iter().all(|&byte| byte == 0xFF),
            "nothing written to an absent function"
        );
        // 0x02_00_00 decodes to 00:00.0 if the high bits are dropped.
        // SAFETY: as above.
        let overflow = unsafe { quiesce_in_window(base, 0, 0, 0x02_00_00) };
        assert_eq!(overflow, Err(QuiesceError::OutOfRange));
    }

    #[test]
    fn claim_enables_memory_decoding_but_never_bus_mastering() {
        let mut ecam = alloc::vec![0u8; ECAM_BYTES_PER_BUS];
        let base = ecam.as_mut_ptr() as usize;
        // A function a previous owner left quiesced (INTx off, no bus master).
        function_config(&mut ecam, CFG_COMMAND, CMD_INTX_DISABLE);
        // SAFETY: the test owns a one-bus in-memory ECAM stand-in.
        assert!(unsafe { enable_memory_decode_in_window(base, 0, 0, 2 << 3) });
        assert_eq!(
            read_config(&ecam, CFG_COMMAND),
            CMD_MEM_SPACE | CMD_INTX_DISABLE
        );
        // Outside the window or above 16 bits: refused, nothing written.
        // SAFETY: as above.
        assert!(!unsafe { enable_memory_decode_in_window(base, 0, 0, (1 << 8) | (2 << 3)) });
        assert!(!unsafe { enable_memory_decode_in_window(base, 0, 0, 0x02_00_00) });
        assert_eq!(
            decoding_command(CMD_BUS_MASTER),
            CMD_BUS_MASTER | CMD_MEM_SPACE
        );
        assert_eq!(decoding_command(0) & CMD_BUS_MASTER, 0);

        // An absent function reads all-ones and is not written.
        let mut absent = alloc::vec![0xFFu8; ECAM_BYTES_PER_BUS];
        // SAFETY: as above.
        assert!(unsafe {
            enable_memory_decode_in_window(absent.as_mut_ptr() as usize, 0, 0, 2 << 3)
        });
        assert!(absent.iter().all(|&byte| byte == 0xFF));
    }

    /// The fingerprint takes identity and class code from config space by BDF,
    /// never from a registration's `cls` argument, and skips BDFs outside the
    /// admitted window.
    #[test]
    fn fingerprint_reads_identity_and_class_from_config_space() {
        let mut ecam = alloc::vec![0u8; ECAM_BYTES_PER_BUS];
        let at = (3usize << 15) | (1 << 12);
        ecam[at..at + 4].copy_from_slice(&[0x86, 0x80, 0x0e, 0x10]);
        ecam[at + 8..at + 12].copy_from_slice(&[0x03, 0x00, 0x00, 0x02]);
        // SAFETY: the window is the live 1 MiB buffer above; bus 1 is outside it.
        let ids = unsafe {
            fingerprint_in_window(ecam.as_mut_ptr() as usize, 0, 0, &[(0, 3, 1), (1, 0, 0)])
        };
        assert_eq!(
            ids,
            [api::machine_facts::PciFunctionId {
                vendor: 0x8086,
                device: 0x100e,
                class: 0x02,
                subclass: 0x00,
                prog_if: 0x00,
            }]
        );
    }
}
