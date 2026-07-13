//! Resource Registry — exclusive MMIO region grants for Driver Cells.
//!
//! A Driver Cell calls `sys_request_mmio(base, len)` (added in Phase 03).
//! The kernel checks here before handing an `MmioRegion` to the Cell:
//!
//! 1. **Allowlist**: the requested range must fall within a known-safe
//!    device window for the current QEMU target.  Unknown ranges are rejected
//!    so a misbehaving Cell cannot map arbitrary kernel memory as MMIO.
//!
//! 2. **Exclusive ownership**: at most one Cell may hold a given MMIO range.
//!    A second `request_mmio` for an overlapping range returns `AlreadyExists`.
//!
//! 3. **Release-on-exit**: `release_for(cell_id)` frees all ranges owned by
//!    a Cell.  Call this from every Cell-exit path alongside
//!    `cell_quota::deregister`.
//!
//! # v1 scope
//! Allowlist is hardcoded per QEMU target (DTB discovery deferred to v2).
//!
//! | Target | Device | Base | Size |
//! |--------|--------|------|------|
//! | QEMU ARM virt (aarch64) | PL011 UART0 | 0x0900_0000 | 0x1000 |
//! | QEMU ARM virt (aarch64) | PL061 GPIO  | 0x0903_0000 | 0x1000 |
//! | QEMU RISC-V virt (riscv64) | (none yet — kernel serial owns UART) | — | — |

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use types::{CellId, ViError, ViResult};

// ---------------------------------------------------------------------------
// Device-class tags (parameterized MMIO capability)
// ---------------------------------------------------------------------------

/// UART serial device window. Set in a cell's `mmio_devices` when its manifest
/// declares `uart = true`.
pub const DEV_UART: u8 = 1 << 0;
/// GPIO controller window. Set when the manifest declares `gpio = true`.
pub const DEV_GPIO: u8 = 1 << 1;
/// PCIe device BAR window. Set on tasks with `PcieDriverCap` — not a manifest flag
/// (all 8 manifest bits are occupied); gated by the ZST cap instead.
pub const DEV_PCIE: u8 = 1 << 2;
/// CAN bus controller window (v2 manifest — freed by the u16 flags widening).
/// Set when the manifest declares `can = true`.
pub const DEV_CAN: u8 = 1 << 3;
/// ADC controller window (v2 manifest). Set when the manifest declares `adc = true`.
pub const DEV_ADC: u8 = 1 << 4;

// ---------------------------------------------------------------------------
// Allowlist (per QEMU machine, v1 hardcoded)
// ---------------------------------------------------------------------------

/// `(base, len, device_class)` triples a Driver Cell may request. The device
/// class scopes the capability: a cell may claim a range only if it declared
/// the matching device (manifest gpio/uart flag), so a GPIO-only cell cannot
/// grab the UART window and vice-versa.
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
const ALLOWED: &[(usize, usize, u8)] = &[
    (0x3F20_0000, 0x1_0000, DEV_GPIO), // BCM2837 GPIO — Raspberry Pi 3 (54 pins)
    (0x3F21_5000, 0x0_1000, DEV_UART), // BCM mini UART (AUX block) — RPi 3
    // BCM I2C (0x3F804000), SPI (0x3F204000) added when respective drivers land.
];

#[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
const ALLOWED: &[(usize, usize, u8)] = &[
    (0x0900_0000, 0x1000, DEV_UART), // PL011 UART0  — QEMU ARM virt
    (0x0903_0000, 0x1000, DEV_GPIO), // PL061 GPIO   — QEMU ARM virt
];

/// SiFive GPIO0 for QEMU `sifive_u` machine (FU540/FU740).
/// The kernel serial driver owns NS16550 at 0x1000_0000 — excluded from allowlist.
#[cfg(target_arch = "riscv64")]
const ALLOWED: &[(usize, usize, u8)] = &[
    (0x1001_2000, 0x1000, DEV_GPIO), // SiFive GPIO0 — QEMU sifive_u machine
];

#[cfg(target_arch = "x86_64")]
const ALLOWED: &[(usize, usize, u8)] = &[];

#[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64", target_arch = "x86_64")))]
const ALLOWED: &[(usize, usize, u8)] = &[];

// ---------------------------------------------------------------------------
// Registry state
// ---------------------------------------------------------------------------

/// Maps MMIO base address → (len, owner CellId).
static REGISTRY: Spinlock<BTreeMap<usize, (usize, CellId)>> =
    Spinlock::new(BTreeMap::new());

/// Dynamically discovered PCIe BAR windows (base → len).
/// Populated by `pcie_ecam::init()` after the ECAM scan; consumed by
/// `request_mmio` when the caller holds `PcieDriverCap` (DEV_PCIE).
static PCIE_BARS: Spinlock<BTreeMap<usize, usize>> =
    Spinlock::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart kernel; called only from the fault/panic path with interrupts
/// disabled.  Force-unlocking an already-free Spinlock is a no-op.
pub unsafe fn force_unlock_locks() {
    REGISTRY.force_unlock();
    // SAFETY: same contract as REGISTRY above.
    unsafe { PCIE_BARS.force_unlock(); }
}

/// Register a PCIe BAR window discovered during ECAM scan.
///
/// Called by `pcie_ecam::init()` for every non-zero BAR on every device.
/// Driver Cells may subsequently call `sys_request_mmio` on these ranges
/// if they hold `PcieDriverCap`.
pub fn register_pcie_bar(base: usize, len: usize) {
    if base != 0 && len != 0 {
        PCIE_BARS.lock().insert(base, len);
    }
}

/// Return `true` if `[base, base+len)` is a known PCIe BAR window.
///
/// Used by the `RequestMmio` handler to decide whether to take the PCIe path.
pub fn is_pcie_bar(base: usize, len: usize) -> bool {
    let guard = PCIE_BARS.lock();
    guard.get(&base).map_or(false, |&bar_len| len <= bar_len)
}

/// Request exclusive MMIO ownership without allowlist validation (Platform Cell only).
///
/// Bypasses the per-arch ALLOWED list and the PCIE_BARS table. The overlap check
/// still runs — two cells cannot share a byte. Used by the PlatformCap bypass path
/// in `sys_request_mmio` so the Platform Cell can claim the ECAM config-space window
/// (which is not a device BAR and therefore not in either allowlist).
pub fn request_mmio_unchecked(cell_id: CellId, base: usize, len: usize) -> ViResult<()> {
    let end = base.checked_add(len).ok_or(ViError::InvalidInput)?;
    let mut reg = REGISTRY.lock();
    for (&eb, &(el, _)) in reg.iter() {
        let ee = eb + el;
        if !(end <= eb || base >= ee) {
            return Err(ViError::AlreadyExists);
        }
    }
    reg.insert(base, (len, cell_id));
    Ok(())
}

/// Request exclusive ownership of `[base, base+len)` for `cell_id`.
///
/// Returns:
/// - `Ok(())` — range is now owned by the caller; construct `MmioRegion` and
///   hand it to the Cell.
/// - `Err(PermissionDenied)` — range not in allowlist, or its device class is
///   not among `allowed_devices` (the cell's declared `mmio_devices`).
/// - `Err(AlreadyExists)` — range overlaps an already-granted region.
/// - `Err(InvalidInput)` — arithmetic overflow in `base + len`.
pub fn request_mmio(cell_id: CellId, base: usize, len: usize, allowed_devices: u8) -> ViResult<()> {
    // 1. Allowlist check — the range must fall inside a known device window
    //    AND that window's device class must be one the cell declared.
    let end = base.checked_add(len).ok_or(ViError::InvalidInput)?;

    // PCIe path: validate against the dynamic BAR table populated by pcie_ecam.
    let in_allowlist = if allowed_devices & DEV_PCIE != 0 {
        let bars = PCIE_BARS.lock();
        bars.get(&base).map_or(false, |&bar_len| len <= bar_len)
    } else {
        // GPIO/UART path: static per-arch allowlist.
        ALLOWED.iter().any(|&(ab, al, class)| {
            let ae = ab + al;
            base >= ab && end <= ae && (class & allowed_devices != 0)
        })
    };
    if !in_allowlist {
        return Err(ViError::PermissionDenied);
    }

    // 2. Overlap check — no two cells may share a byte
    let mut reg = REGISTRY.lock();
    for (&eb, &(el, _owner)) in reg.iter() {
        let ee = eb + el;
        // Ranges overlap when NOT (end ≤ eb OR base ≥ ee)
        if !(end <= eb || base >= ee) {
            return Err(ViError::AlreadyExists);
        }
    }

    reg.insert(base, (len, cell_id));
    Ok(())
}

/// Release all MMIO regions owned by `cell_id`.
///
/// Call this from every Cell-exit path (Exit syscall, ForceExit, fault, watchdog).
pub fn release_for(cell_id: CellId) {
    REGISTRY.lock().retain(|_base, &mut (_len, owner)| owner != cell_id);
}

/// Return the task ID (TID) of the cell that currently owns the MMIO region
/// whose base address exactly matches `base`.
///
/// Returns `None` if no cell has requested that exact base address.
/// Used by the GPIO IRQ handler to route interrupts to the current MMIO owner.
pub fn lookup_mmio_owner(base: usize) -> Option<usize> {
    REGISTRY.lock().get(&base).map(|&(_len, cell_id)| cell_id.0 as usize)
}

/// Current number of registered regions (diagnostics).
pub fn region_count() -> usize {
    REGISTRY.lock().len()
}

// ---------------------------------------------------------------------------
// PCIe BDF ownership (for sys_grant_dma authorization)
// ---------------------------------------------------------------------------

/// Maps PCIe BDF → owning task ID.
///
/// Kernel drivers (NIC, NVMe) are not registered here — they bypass `sys_grant_dma`
/// and call `iommu::map_dma_for_cell(0, bdf, ...)` directly during init.
/// Only userspace Driver Cells that receive PCIe device ownership via capability
/// delegation need to register here.
static BDF_OWNERS: Spinlock<alloc::collections::BTreeMap<u32, usize>> =
    Spinlock::new(alloc::collections::BTreeMap::new());

/// Register a PCIe BDF as owned by task `tid`.
///
/// Called when a Driver Cell is granted ownership of a PCIe device.
pub fn register_bdf_owner(bdf: u32, tid: usize) {
    BDF_OWNERS.lock().insert(bdf, tid);
}

/// Return the task ID that currently owns `bdf`, or `None` if unowned.
pub fn owner_of_bdf(bdf: u32) -> Option<usize> {
    BDF_OWNERS.lock().get(&bdf).copied()
}

/// Release all BDF ownerships held by task `tid` (called on Cell exit).
pub fn release_bdfs_for(tid: usize) {
    BDF_OWNERS.lock().retain(|_bdf, &mut owner| owner != tid);
}

/// Force-unlock BDF_OWNERS during fault teardown.
///
/// # Safety
/// See `force_unlock_locks` above — same contract.
pub unsafe fn force_unlock_bdf_locks() {
    BDF_OWNERS.force_unlock();
}
