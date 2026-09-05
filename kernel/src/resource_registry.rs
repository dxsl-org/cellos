//! Resource Registry — exclusive MMIO region grants for Driver Cells.
//!
//! A Driver Cell calls `sys_request_mmio(base, len)` (added in Phase 03).
//! The kernel checks here before handing an `MmioRegion` to the Cell:
//!
//! 1. **Allowlist**: the requested range must fall within a known-safe
//!    device window for the active target. Unknown ranges are rejected
//!    so a misbehaving Cell cannot map arbitrary kernel memory as MMIO.
//!
//! 2. **Exclusive ownership**: at most one Cell may hold a given MMIO range.
//!    A second `request_mmio` for an overlapping range returns `AlreadyExists`.
//!
//! 3. **Release-on-exit**: `release_for(cell_id)` frees all ranges owned by
//!    a Cell.  Call this from every Cell-exit path alongside
//!    `cell_quota::deregister`.
//!
//! # Current scope
//! Allowlist is hardcoded per target (DTB discovery remains deferred).
//!
//! | Target | Device | Base | Size |
//! |--------|--------|------|------|
//! | QEMU ARM virt (aarch64) | PL011 UART0 | 0x0900_0000 | 0x1000 |
//! | QEMU ARM virt (aarch64) | PL061 GPIO  | 0x0903_0000 | 0x1000 |
//! | Raspberry Pi 3 | BCM2837 GPIO/BSC1/SPI0/AUX | exact 4-KiB windows | 0x1000 |
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
/// PCIe device BAR window. Set on tasks with `PcieDriverCap`; PCIe authority is
/// path-gated and intentionally remains separate from hardware manifest flags.
pub const DEV_PCIE: u8 = 1 << 2;
/// CAN bus controller window (v2 manifest — freed by the u16 flags widening).
/// Set when the manifest declares `can = true`.
pub const DEV_CAN: u8 = 1 << 3;
/// ADC controller window (v2 manifest). Set when the manifest declares `adc = true`.
pub const DEV_ADC: u8 = 1 << 4;
/// I2C controller window (v2 manifest). Set when the manifest declares `i2c = true`.
pub const DEV_I2C: u8 = 1 << 5;
pub const DEV_SPI: u8 = 1 << 6;
/// Firmware / display controller window.
pub const DEV_DISPLAY: u8 = 1 << 7;
// DEV_USB is intentionally absent: the u8 manifest byte is full (bits 0–7 used).
// USB host controller authority requires policy v3 with an explicit signed byte.
// Gate with a test matrix in policy::self_test before implementing.
// ---------------------------------------------------------------------------
// Allowlist (per active target, currently hardcoded)
// ---------------------------------------------------------------------------

/// `(base, len, device_class)` triples a Driver Cell may request. The device
/// class scopes the capability: a cell may claim a range only if it declared
/// the matching device class, so one peripheral class cannot claim another
/// class's controller window.
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
const ALLOWED: &[(usize, usize, u8)] = &[
    (
        hal_soc_bcm27xx::BCM2837.mmio.gpio_base,
        hal_soc_bcm27xx::BCM2837.mmio.gpio_grant_size,
        DEV_GPIO,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.aux_base,
        hal_soc_bcm27xx::BCM2837.mmio.aux_grant_size,
        DEV_UART,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.bsc1_base,
        hal_soc_bcm27xx::BCM2837.mmio.bsc1_grant_size,
        DEV_I2C,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.spi0_base,
        hal_soc_bcm27xx::BCM2837.mmio.spi0_grant_size,
        DEV_SPI,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.mailbox_base,
        hal_soc_bcm27xx::BCM2837.mmio.mailbox_grant_size,
        DEV_DISPLAY,
    ),
];

#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
const ALLOWED: &[(usize, usize, u8)] = &[
    (
        hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base,
        hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.size,
        DEV_UART,
    ),
    (
        hal_soc_arm_virt::QEMU_ARM_VIRT.gpio.mmio.base,
        hal_soc_arm_virt::QEMU_ARM_VIRT.gpio.mmio.size,
        DEV_GPIO,
    ),
];

#[cfg(all(
    target_arch = "aarch64",
    feature = "board-rpi4",
    not(feature = "board-rpi3")
))]
const ALLOWED: &[(usize, usize, u8)] = &[
    (
        hal_soc_bcm27xx::BCM2711.mmio.uart_base,
        hal_soc_bcm27xx::BCM2711.mmio.uart_grant_size,
        DEV_UART,
    ),
    (
        hal_soc_bcm27xx::BCM2711.mmio.gpio_base,
        hal_soc_bcm27xx::BCM2711.mmio.gpio_grant_size,
        DEV_GPIO,
    ),
];

/// SiFive GPIO0 for QEMU `sifive_u` machine (FU540/FU740).
/// The kernel serial driver owns NS16550 at 0x1000_0000 — excluded from allowlist.
#[cfg(target_arch = "riscv64")]
const ALLOWED: &[(usize, usize, u8)] = &[
    (0x1001_2000, 0x1000, DEV_GPIO), // SiFive GPIO0 — QEMU sifive_u machine
];

#[cfg(target_arch = "x86_64")]
const ALLOWED: &[(usize, usize, u8)] = &[];

#[cfg(not(any(
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "x86_64"
)))]
const ALLOWED: &[(usize, usize, u8)] = &[];

// ---------------------------------------------------------------------------
// Registry state
// ---------------------------------------------------------------------------

/// Maps MMIO base address → (len, owner CellId).
static REGISTRY: Spinlock<BTreeMap<usize, (usize, CellId)>> = Spinlock::new(BTreeMap::new());

#[derive(Clone, Copy, PartialEq, Eq)]
struct DisplayFramebuffer {
    owner: CellId,
    base: usize,
    size: usize,
    width: u16,
    height: u16,
    pitch: usize,
}

/// Firmware scanout metadata for the current boot.
///
/// The mapping remains shared among trusted Tier-1 cells by architecture; this
/// record is an authority and geometry guard, not per-cell MMU isolation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayFramebufferState {
    Empty,
    Pending(DisplayFramebuffer),
    Active(DisplayFramebuffer),
}

static DISPLAY_FRAMEBUFFER: Spinlock<DisplayFramebufferState> =
    Spinlock::new(DisplayFramebufferState::Empty);

/// Dynamically discovered PCIe BAR windows (base → len).
/// Populated by `pcie_ecam::init()` after the ECAM scan; consumed by
/// `request_mmio` when the caller holds `PcieDriverCap` (DEV_PCIE).
static PCIE_BARS: Spinlock<BTreeMap<usize, usize>> = Spinlock::new(BTreeMap::new());

fn static_range_allowed(
    allowed: &[(usize, usize, u8)],
    base: usize,
    end: usize,
    allowed_devices: u8,
) -> bool {
    allowed.iter().any(|&(window_base, window_len, class)| {
        window_base
            .checked_add(window_len)
            .is_some_and(|window_end| {
                base >= window_base && end <= window_end && class & allowed_devices != 0
            })
    })
}

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
    DISPLAY_FRAMEBUFFER.force_unlock();
    // SAFETY: same contract as REGISTRY above.
    unsafe {
        PCIE_BARS.force_unlock();
    }
}

/// Register a PCIe BAR window discovered during ECAM scan.
///
/// Called by `pcie_ecam::init()` for every non-zero BAR on every device.
/// Driver Cells may subsequently call `sys_request_mmio` on these ranges
/// if they hold `PcieDriverCap`.
pub fn register_pcie_bar(base: usize, len: usize) {
    if !valid_pcie_bar_window(base, len) {
        log::warn!(
            "[pcie] rejected invalid BAR window base={:#x} len={:#x}",
            base,
            len
        );
        return;
    }
    PCIE_BARS.lock().insert(base, len);
}

/// Current Driver Cells support bounded conventional BARs only.
///
/// ReBAR and larger accelerator apertures need a separate policy because they
/// materially widen a Cell's MMIO authority.
fn valid_pcie_bar_window(base: usize, len: usize) -> bool {
    const MAX_SUPPORTED_BAR_LEN: usize = 1 << 30;

    base != 0
        && len != 0
        && len <= MAX_SUPPORTED_BAR_LEN
        && len.is_power_of_two()
        && base & (len - 1) == 0
        && base.checked_add(len).is_some()
}

/// Return `true` if `[base, base+len)` is a known PCIe BAR window.
///
/// Used by the `RequestMmio` handler to decide whether to take the PCIe path.
pub fn is_pcie_bar(base: usize, len: usize) -> bool {
    if len == 0 {
        return false;
    }
    let guard = PCIE_BARS.lock();
    guard.get(&base).is_some_and(|&bar_len| len <= bar_len)
}

fn checked_mmio_end(base: usize, len: usize) -> ViResult<usize> {
    if len == 0 {
        return Err(ViError::InvalidInput);
    }
    base.checked_add(len).ok_or(ViError::InvalidInput)
}

/// Request exclusive MMIO ownership without allowlist validation (Platform Cell only).
///
/// Bypasses the per-arch ALLOWED list and the PCIE_BARS table. The overlap check
/// still runs — two cells cannot share a byte. Used by the PlatformCap bypass path
/// in `sys_request_mmio` so the Platform Cell can claim the ECAM config-space window
/// (which is not a device BAR and therefore not in either allowlist).
pub fn request_mmio_unchecked(cell_id: CellId, base: usize, len: usize) -> ViResult<()> {
    let end = checked_mmio_end(base, len)?;
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
/// - `Err(InvalidInput)` — zero length or arithmetic overflow in `base + len`.
pub fn request_mmio(cell_id: CellId, base: usize, len: usize, allowed_devices: u8) -> ViResult<()> {
    // 1. Allowlist check — the range must fall inside a known device window
    //    AND that window's device class must be one the cell declared.
    let end = checked_mmio_end(base, len)?;

    // PCIe path: validate against the dynamic BAR table populated by pcie_ecam.
    let in_allowlist = if allowed_devices & DEV_PCIE != 0 {
        let bars = PCIE_BARS.lock();
        bars.get(&base).is_some_and(|&bar_len| len <= bar_len)
    } else {
        // SoC peripheral path: static per-arch allowlist.
        static_range_allowed(ALLOWED, base, end, allowed_devices)
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

// ---------------------------------------------------------------------------
// Boot-time allowlist self-test (runs on every boot, QEMU-visible)
// ---------------------------------------------------------------------------

/// Power-on self-test of the MMIO allowlist. Non-RPi3 targets have nothing
/// pinned yet; the test exists so the boot chain can call it unconditionally.
#[cfg(not(all(target_arch = "aarch64", feature = "board-rpi3")))]
pub fn self_test() -> bool {
    true
}

/// Power-on self-test of the RPi3 production MMIO allowlist.
///
/// Pins two properties on the REAL table (`ALLOWED`), not a synthetic copy:
///
/// 1. Positive control — a known window (mailbox/DEV_DISPLAY) still authorizes.
///    Without this, an accidentally emptied table would make every denial
///    below vacuous.
/// 2. DWC2 denial — the DWC2 USB window must be rejected for every device
///    class, full masks included, over the whole aperture AND at its edges.
///    The entry was previously present mis-tagged `DEV_PCIE`; it may return
///    only via policy v3 with a signed USB-host authority byte.
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub fn self_test() -> bool {
    let mmio = hal_soc_bcm27xx::BCM2837.mmio;

    // 1. Positive control: the table is live.
    let mbox_base = mmio.mailbox_base;
    let mbox_end = mbox_base + mmio.mailbox_grant_size;
    if !static_range_allowed(ALLOWED, mbox_base, mbox_end, DEV_DISPLAY) {
        log::error!("[selftest] mmio-allowlist: FAIL — mailbox window no longer authorized");
        return false;
    }

    // 2. DWC2 denial across the full aperture, per class and combined masks.
    let dwc2_base = mmio.dwc2_base;
    let dwc2_end = dwc2_base + mmio.dwc2_grant_size;
    let classes = [
        DEV_UART,
        DEV_GPIO,
        DEV_PCIE,
        DEV_CAN,
        DEV_ADC,
        DEV_I2C,
        DEV_SPI,
        DEV_DISPLAY,
        0xFF,
    ];
    for class in classes {
        if static_range_allowed(ALLOWED, dwc2_base, dwc2_end, class) {
            log::error!(
                "[selftest] mmio-allowlist: FAIL — DWC2 window authorized for class {class:#04x}"
            );
            return false;
        }
    }

    // Edge words: same full class sweep as the whole aperture — a future
    // sub-range entry tagged with ANY class must fail the boot self-test.
    let last_word = dwc2_end - 4;
    for addr in [dwc2_base, last_word] {
        for class in classes {
            if static_range_allowed(ALLOWED, addr, addr + 4, class) {
                log::error!(
                    "[selftest] mmio-allowlist: FAIL — DWC2 edge word {addr:#x} authorized \
                     for class {class:#04x}"
                );
                return false;
            }
        }
    }
    true
}

/// Release all MMIO regions owned by `cell_id`.
///
/// Call this from every Cell-exit path (Exit syscall, ForceExit, fault, watchdog).
pub fn release_for(cell_id: CellId) {
    REGISTRY
        .lock()
        .retain(|_base, &mut (_len, owner)| owner != cell_id);
    // A VideoCore allocation and its USER mapping remain valid until reboot.
    // Do not clear Active metadata or permit a restarted cell to replace it:
    // restart is fail-stop for display after registration.
}

/// Return the task ID (TID) of the cell that currently owns the MMIO region
/// whose base address exactly matches `base`.
///
/// Returns `None` if no cell has requested that exact base address.
/// Used by the GPIO IRQ handler to route interrupts to the current MMIO owner.
pub fn lookup_mmio_owner(base: usize) -> Option<usize> {
    REGISTRY
        .lock()
        .get(&base)
        .map(|&(_len, cell_id)| cell_id.0 as usize)
}

/// Return whether `cell_id` owns exactly `[base, base + len)`.
///
/// Display registration uses this rather than an overlap query: the VideoCore
/// mailbox authority is one fixed capability, so a subrange cannot be confused
/// with an independently granted peripheral window.
pub fn owns_exact_mmio(cell_id: CellId, base: usize, len: usize) -> bool {
    REGISTRY
        .lock()
        .get(&base)
        .is_some_and(|&(owned_len, owner)| owned_len == len && owner == cell_id)
}

/// Outcome of atomically reserving one framebuffer mapping transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayFramebufferReservation {
    Reserved,
    ActiveReplay,
}

/// Why a framebuffer reservation could not be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayFramebufferReservationError {
    /// Another pending or active framebuffer transaction owns the singleton.
    Conflict,
}

fn display_framebuffer_candidate(
    owner: CellId,
    base: usize,
    size: usize,
    width: u16,
    height: u16,
    pitch: usize,
) -> DisplayFramebuffer {
    DisplayFramebuffer {
        owner,
        base,
        size,
        width,
        height,
        pitch,
    }
}

/// Reserve one framebuffer mapping before page-table mutation.
///
/// A reservation serializes preflight/map/activation. Only an identical Active
/// replay is accepted; a pending or different active request fails closed.
pub fn reserve_display_framebuffer(
    owner: CellId,
    base: usize,
    size: usize,
    width: u16,
    height: u16,
    pitch: usize,
) -> Result<DisplayFramebufferReservation, DisplayFramebufferReservationError> {
    let candidate = display_framebuffer_candidate(owner, base, size, width, height, pitch);
    let mut framebuffer = DISPLAY_FRAMEBUFFER.lock();
    match *framebuffer {
        DisplayFramebufferState::Empty => {
            *framebuffer = DisplayFramebufferState::Pending(candidate);
            Ok(DisplayFramebufferReservation::Reserved)
        }
        DisplayFramebufferState::Active(existing) if existing == candidate => {
            Ok(DisplayFramebufferReservation::ActiveReplay)
        }
        DisplayFramebufferState::Pending(_) | DisplayFramebufferState::Active(_) => {
            Err(DisplayFramebufferReservationError::Conflict)
        }
    }
}

/// Commit a previously reserved mapping after every page was installed.
pub fn activate_display_framebuffer(
    owner: CellId,
    base: usize,
    size: usize,
    width: u16,
    height: u16,
    pitch: usize,
) -> bool {
    let candidate = display_framebuffer_candidate(owner, base, size, width, height, pitch);
    let mut framebuffer = DISPLAY_FRAMEBUFFER.lock();
    if *framebuffer == DisplayFramebufferState::Pending(candidate) {
        *framebuffer = DisplayFramebufferState::Active(candidate);
        true
    } else {
        false
    }
}

/// Cancel a reservation when preflight or mapping fails before activation.
pub fn cancel_display_framebuffer(
    owner: CellId,
    base: usize,
    size: usize,
    width: u16,
    height: u16,
    pitch: usize,
) {
    let candidate = display_framebuffer_candidate(owner, base, size, width, height, pitch);
    let mut framebuffer = DISPLAY_FRAMEBUFFER.lock();
    if *framebuffer == DisplayFramebufferState::Pending(candidate) {
        *framebuffer = DisplayFramebufferState::Empty;
    }
}

/// Return the active firmware scanout geometry, if registered.
pub fn display_framebuffer_resolution() -> Option<(u16, u16)> {
    match *DISPLAY_FRAMEBUFFER.lock() {
        DisplayFramebufferState::Active(framebuffer) => {
            Some((framebuffer.width, framebuffer.height))
        }
        DisplayFramebufferState::Empty | DisplayFramebufferState::Pending(_) => None,
    }
}

/// Current number of registered regions (diagnostics).
pub fn region_count() -> usize {
    REGISTRY.lock().len()
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn display_registration_is_fail_stop_after_activation() {
        let owner = CellId(0x7d01);
        assert_eq!(
            reserve_display_framebuffer(owner, 0x3e00_0000, 0x1000, 800, 480, 3200,),
            Ok(DisplayFramebufferReservation::Reserved)
        );
        cancel_display_framebuffer(owner, 0x3e00_0000, 0x1000, 800, 480, 3200);
        assert_eq!(display_framebuffer_resolution(), None);
        assert_eq!(
            reserve_display_framebuffer(owner, 0x3e00_0000, 0x1000, 800, 480, 3200,),
            Ok(DisplayFramebufferReservation::Reserved)
        );
        assert!(activate_display_framebuffer(
            owner,
            0x3e00_0000,
            0x1000,
            800,
            480,
            3200,
        ));
        assert_eq!(
            reserve_display_framebuffer(owner, 0x3e00_0000, 0x1000, 800, 480, 3200,),
            Ok(DisplayFramebufferReservation::ActiveReplay)
        );
        assert!(
            reserve_display_framebuffer(CellId(0x7d02), 0x3e00_0000, 0x1000, 800, 480, 3200,)
                .is_err()
        );
        assert_eq!(display_framebuffer_resolution(), Some((800, 480)));
        release_for(owner);
        assert_eq!(display_framebuffer_resolution(), Some((800, 480)));
    }
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

#[cfg(feature = "test-hooks")]
#[path = "resource_registry_selftest.rs"]
mod selftest;

#[cfg(feature = "test-hooks")]
pub(crate) fn pcie_bar_window_self_test() -> bool {
    selftest::run()
}

#[cfg(test)]
#[path = "resource_registry_tests.rs"]
mod tests;
