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

/// Ethernet controller IDs that have a Driver Cell in this image.
///
/// This is the **family gate**: an Ethernet device is offered to a Driver Cell
/// only when a cell drives its family, and every other Ethernet ID is still
/// rejected **by name** at scan time (`[e1000] unsupported Ethernet
/// vvvv:dddd; driver gate closed`). Widened in phase 04a of the x86 PC lane from
/// "82540EM only" to "families that have a driver cell": the 82540EM
/// (`/bin/e1000`) and the igb family (`/bin/igb`) — QEMU's 82576 model plus the
/// datasheet i210/i211 SKUs the cell's `identity.rs` lists.
///
/// The table bounds what the *kernel* offers; the cell repeats the identity
/// check before programming a register model, so neither can bind a foreign
/// family by accident.
pub const ETHERNET_DRIVER_IDS: [(u16, u16); 5] = [
    (0x8086, 0x100E), // 82540EM — /bin/e1000
    (0x8086, 0x10C9), // 82576 — QEMU's igb model, /bin/igb
    (0x8086, 0x1533), // i210 copper with external flash — /bin/igb
    (0x1AF4, 0x1041), // virtio-net (modern) — /bin/virtio-net (ADR-0031 phase 05)
    (0x1AF4, 0x1000), // virtio-net (transitional, modern caps required) — /bin/virtio-net
];

// Deliberately refused until the cell implements their prerequisites: flashless
// i210 (0x157B/0x157C) and i211 (0x1539) need an **iNVM** read path, and
// fibre/SerDes/SGMII (0x1536/0x1537/0x1538) need media-specific link setup.
// Widening this table without that work would hand a device to a cell that cannot
// drive it, which is worse than refusing it by name.

/// Ethernet IDs served by the **legacy class-triple** query (`FindPcieDevice`,
/// opcode 418).
///
/// `02:00:00` is one triple for every Ethernet family, so once a second family
/// has a Driver Cell the class query can no longer name a device uniquely: it
/// would hand the igb controller to `/bin/e1000`, which must then decline and
/// release it before `/bin/igb` can claim it — a scheduling-dependent race. The
/// fix is [`find_vendor_device`] (opcode 424), which matches exactly. `find_class`
/// therefore keeps serving only the family that predates the identity query, the
/// 82540EM; a cell whose controller shares its class with another cell MUST use
/// the vendor:device query. The family gate above is unchanged and still governs
/// the scan warning and the vendor query.
pub const ETHERNET_LEGACY_CLASS_IDS: [(u16, u16); 1] = [(0x8086, 0x100E)];

/// Whether `(vendor_id, device_id)` is an Ethernet family some Driver Cell in
/// this image implements. Everything else fails closed.
pub fn ethernet_id_has_driver(vendor_id: u16, device_id: u16) -> bool {
    ETHERNET_DRIVER_IDS
        .iter()
        .any(|&(vendor, device)| vendor == vendor_id && device == device_id)
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

            // Decode BARs for type-0 headers (endpoints). Skip bridges (type-1).
            let hdr = unsafe { read8(ecam_base, config_bus, dev, fun, CFG_HEADER_TYPE) } & 0x7F;
            let bars = if hdr == 0 {
                // SAFETY: the selected ECAM bus window is identity-mapped.
                unsafe { decode_bars(ecam_base, config_bus, dev, fun) }
            } else {
                [Bar::None; 6]
            };

            // SAFETY: the selected ECAM bus window is identity-mapped.
            let (msix, pm) = unsafe { walk_caps(ecam_base, config_bus, dev, fun) };

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

/// Register a device announced by the Platform Cell via `sys_register_pci_device`.
///
/// Decodes `bdf` and `cls` into a `PciDevice` stub with BAR0 populated.
/// Skips duplicate BDFs (idempotent: kernel ECAM scan may already have found the device).
/// Also registers BAR0 in the Resource Registry for `sys_request_mmio` validation.
pub fn register_device(bdf: u32, cls: u32, bar0_base: usize, bar0_size: usize) {
    // Canonical BDF wire layout everywhere (Platform Cell, FindPcieDevice,
    // GrantDma, iommu_x86): bus[15:8] | dev[7:3] | fun[2:0]. An earlier decode
    // here assumed bus<<16|dev<<8|fun, which garbled the stored (bus,dev,fun)
    // tuple for every device (it only round-tripped through FindPcieDevice's
    // re-encode by luck on bus 0).
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let fun = (bdf & 0x07) as u8;
    let class = ((cls >> 16) & 0xFF) as u8;
    let subclass = ((cls >> 8) & 0xFF) as u8;
    let prog_if = (cls & 0xFF) as u8;
    #[cfg(target_arch = "x86_64")]
    let (vendor_id, device_id) = {
        let ecam_base = ecam_base_x86();
        let (bus_start, bus_end) = ecam_bus_range_x86();
        if ecam_base == 0 {
            (0, 0)
        } else {
            // SAFETY: x86 paging maps the validated MCFG allocation before the
            // Platform Cell can issue registration syscalls.
            unsafe {
                read_identity_in_range(ecam_base, bus_start, bus_end, bus, dev, fun)
                    .unwrap_or((0, 0))
            }
        }
    };
    #[cfg(not(target_arch = "x86_64"))]
    let (vendor_id, device_id) = (0, 0);

    {
        let devices = PCI_DEVICES.lock();
        if devices.iter().any(|d| d.bdf == (bus, dev, fun)) {
            return; // already present (kernel ECAM scan ran first)
        }
    }

    let bars = if bar0_base != 0 {
        let bar0 = if bar0_size <= u32::MAX as usize {
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
        let mut arr = [Bar::None; 6];
        arr[0] = bar0;
        arr
    } else {
        [Bar::None; 6]
    };

    log::info!(
        "[pcie] platform-reg {:02x}:{:02x}.{} class={:02x}:{:02x}:{:02x} bar0={:#x}",
        bus,
        dev,
        fun,
        class,
        subclass,
        prog_if,
        bar0_base
    );

    let mut devices = PCI_DEVICES.lock();
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    devices.push(PciDevice {
        bdf: (bus, dev, fun),
        vendor_id,
        device_id,
        class,
        subclass,
        prog_if,
        bars,
        msix: None,
        pm: None,
    });
    #[cfg(cellos_p01_trace)]
    drop(_semantic);
    drop(devices);

    if bar0_base != 0 && bar0_size != 0 {
        crate::resource_registry::register_pcie_bar(bar0_base, bar0_size);
    }
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
            // Dword at 0x08: revision ID, prog IF, subclass, class code.
            let class_reg =
                config_addr_in_range(ecam_base, bus_start, bus_end, bus, dev, fun, 0x08)?;
            // SAFETY: the checked, 4-byte-aligned class register lies in the
            // same live function header.
            let class_code = unsafe { core::ptr::read_volatile(class_reg.cast::<u32>()) };
            Some(api::machine_facts::PciFunctionId {
                vendor,
                device,
                class: (class_code >> 24) as u8,
                subclass: (class_code >> 16) as u8,
                prog_if: (class_code >> 8) as u8,
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
            // Ethernet spans more than one Driver Cell (/bin/e1000 and /bin/igb),
            // so the class triple can no longer name a device uniquely. It is
            // served only for the legacy family; the igb family claims its own
            // controller by exact vendor:device (`find_vendor_device`). Binding
            // an I219/e1000e-class endpoint by generic Ethernet class would
            // program an incompatible register model, so an ID with no cell
            // stays refused here and is named in `scan`'s warning.
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
        // Families with a Driver Cell are offered.
        assert!(ethernet_id_has_driver(0x8086, 0x100E)); // 82540EM, /bin/e1000
        assert!(ethernet_id_has_driver(0x8086, 0x10C9)); // QEMU igb model
        assert!(ethernet_id_has_driver(0x8086, 0x1533)); // i210 copper, external flash
                                                         // Everything else stays refused by name — notably the recorded
                                                         // e1000e/I219 follow-up, a non-Intel Ethernet controller, and the igb
                                                         // SKUs whose prerequisites the cell does not implement yet (flashless
                                                         // i210 and i211 need iNVM; fibre/SerDes/SGMII need media link setup).
        assert!(!ethernet_id_has_driver(0x8086, 0x10D3)); // e1000e/I219
        assert!(!ethernet_id_has_driver(0x8086, 0x1539)); // i211 — needs iNVM
        assert!(!ethernet_id_has_driver(0x8086, 0x157B)); // i210 flashless — needs iNVM
        assert!(!ethernet_id_has_driver(0x8086, 0x1536)); // i210 fibre — needs media setup
        assert!(ethernet_id_has_driver(0x1AF4, 0x1000)); // virtio-net, transitional
        assert!(ethernet_id_has_driver(0x1AF4, 0x1041)); // virtio-net, modern
        assert!(!ethernet_id_has_driver(0x1AF4, 0x1042)); // virtio-blk is not Ethernet
        assert!(!ethernet_id_has_driver(0x8086, 0x0));
    }

    #[test]
    fn class_triple_serves_only_the_legacy_family() {
        // Only the 82540EM keeps the class-triple query: every other family is
        // reachable exclusively by `find_vendor_device`, so the two Ethernet
        // Driver Cells cannot race for one class triple.
        assert!(ethernet_class_query_serves(0x8086, 0x100E));
        for &(vendor, device) in &ETHERNET_DRIVER_IDS {
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
