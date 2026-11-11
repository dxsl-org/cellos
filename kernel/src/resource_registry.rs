//! Resource Registry — exclusive MMIO region grants for Driver Cells.
//!
//! A Driver Cell calls `sys_request_mmio(base, len)` (added in Phase 03).
//! The kernel checks here before handing an `MmioRegion` to the Cell:
//!
//! 1. **Allowlist**: the requested range must fall within a known-safe
//!    device window for the active target. Unknown ranges are rejected
//!    so a misbehaving Cell cannot map arbitrary kernel memory as MMIO.
//!
//! 2. **Exclusive ownership**: at most one task may hold a given MMIO range.
//!    A second `request_mmio` for an overlapping range returns `AlreadyExists`.
//!
//! 3. **Release-on-exit**: `release_mmio_for(tid)` frees all ranges owned by
//!    a task. Owners are task ids, the same key as `BDF_OWNERS`: the claiming
//!    task holds the window, so retirement releases it per member task. On
//!    AArch64 the released pages also return to their boot posture, which on
//!    the Pi boards is kernel-only.
//!
//! 4. **Grant is the mapping**: `RequestMmio` sets USER on the whole pages of
//!    a granted window. A sub-page window (the BCM2837 mailbox, which shares
//!    its page with the IRQ controller) is never USER-mapped; the kernel
//!    performs the owner's accesses to its bytes (`mediation`).
//!
//! # Current scope
//! Allowlist is hardcoded per target (DTB discovery remains deferred).
//!
//! | Target | Device | Base | Size |
//! |--------|--------|------|------|
//! | QEMU ARM virt (aarch64) | PL011 UART0 | 0x0900_0000 | 0x1000 |
//! | QEMU ARM virt (aarch64) | PL061 GPIO  | 0x0903_0000 | 0x1000 |
//! | Raspberry Pi 3 | BCM2837 GPIO/BSC1/SPI0/AUX | exact 4-KiB windows | 0x1000 |
//! | Raspberry Pi 3 | VideoCore mailbox (mediated) | 0x3F00_B880 | 0x40 |
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

/// Class of an owned MMIO window: the `DEV_*` bits, plus window kinds that have
/// no `DEV_*` bit because they are not manifest-declared devices.
///
/// The class is stored with the region when it is granted. Re-deriving it at
/// revoke time would have to guess: a PCIe BAR is in neither allowlist, and a
/// granted sub-window is contained in — not equal to — the window it came from.
pub type MmioClass = u16;

/// The Platform Cell's ECAM config-space window (`request_ecam_mmio`).
pub const CLASS_ECAM: MmioClass = 1 << 8;
/// The DWC2 USB host aperture (`request_dwc2_mmio`).
pub const CLASS_DWC2: MmioClass = 1 << 9;
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
///
/// AArch64: one table per SoC, kept here so the `DEV_*` bits stay kernel-local;
/// the board backend's [`ArmSoc`](hal::aarch64::backend::ArmSoc) selects it.
#[cfg(target_arch = "aarch64")]
const ALLOWED: &[(usize, usize, u8)] =
    match <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::SOC {
        hal::aarch64::backend::ArmSoc::QemuArmVirt => QEMU_ARM_VIRT_ALLOWED,
        hal::aarch64::backend::ArmSoc::Bcm2837 => BCM2837_ALLOWED,
        hal::aarch64::backend::ArmSoc::Bcm2711 => BCM2711_ALLOWED,
        hal::aarch64::backend::ArmSoc::AppleT8103 => APPLE_T8103_ALLOWED,
    };

#[cfg(target_arch = "aarch64")]
const APPLE_T8103_ALLOWED: &[(usize, usize, u8)] = &[];

#[cfg(target_arch = "aarch64")]
const BCM2837_ALLOWED: &[(usize, usize, u8)] = &[
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

#[cfg(target_arch = "aarch64")]
const QEMU_ARM_VIRT_ALLOWED: &[(usize, usize, u8)] = &[
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

#[cfg(target_arch = "aarch64")]
const BCM2711_ALLOWED: &[(usize, usize, u8)] = &[
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

// Authority-table equality (Q5): every AArch64 build checks all three tables at
// compile time against the pre-plan snapshot (baseline `6d5cdeb0c`, values
// copied once from the SoC profiles). A drifted profile or a mis-moved entry
// fails the build instead of widening a Cell's MMIO reach.
#[cfg(target_arch = "aarch64")]
const BCM2837_ALLOWED_SNAPSHOT: &[(usize, usize, u8)] = &[
    (0x3F20_0000, 0x1000, DEV_GPIO),
    (0x3F21_5000, 0x1000, DEV_UART),
    (0x3F80_4000, 0x1000, DEV_I2C),
    (0x3F20_4000, 0x1000, DEV_SPI),
    // Narrowed from 0x1000 (H-18): the mailbox registers only, never the
    // IRQ-controller page they sit on; accesses are kernel-mediated.
    (0x3F00_B880, 0x40, DEV_DISPLAY),
];
#[cfg(target_arch = "aarch64")]
const QEMU_ARM_VIRT_ALLOWED_SNAPSHOT: &[(usize, usize, u8)] = &[
    (0x0900_0000, 0x1000, DEV_UART),
    (0x0903_0000, 0x1000, DEV_GPIO),
];
#[cfg(target_arch = "aarch64")]
const BCM2711_ALLOWED_SNAPSHOT: &[(usize, usize, u8)] = &[
    (0xFE20_1000, 0x1000, DEV_UART),
    (0xFE20_0000, 0x1000, DEV_GPIO),
];

#[cfg(target_arch = "aarch64")]
const _: () = assert!(allowlists_equal(BCM2837_ALLOWED, BCM2837_ALLOWED_SNAPSHOT));
#[cfg(target_arch = "aarch64")]
const _: () = assert!(allowlists_equal(
    QEMU_ARM_VIRT_ALLOWED,
    QEMU_ARM_VIRT_ALLOWED_SNAPSHOT
));
#[cfg(target_arch = "aarch64")]
const _: () = assert!(allowlists_equal(BCM2711_ALLOWED, BCM2711_ALLOWED_SNAPSHOT));

/// Entry-by-entry, order-sensitive equality of two allowlists.
#[cfg(target_arch = "aarch64")]
const fn allowlists_equal(a: &[(usize, usize, u8)], b: &[(usize, usize, u8)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i].0 != b[i].0 || a[i].1 != b[i].1 || a[i].2 != b[i].2 {
            return false;
        }
        i += 1;
    }
    true
}

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

/// Maps MMIO base address → the window's ownership record.
static REGISTRY: Spinlock<BTreeMap<usize, MmioRegion>> =
    Spinlock::ranked(BTreeMap::new(), crate::sync::rank::RESOURCE_REGISTRY);

/// One owned MMIO window. `owner_tid` is the claiming task: every query
/// (`lookup_mmio_owner`, WaitIrq, block-route ownership) compares task ids.
#[derive(Clone, Copy, PartialEq, Eq)]
struct MmioRegion {
    len: usize,
    owner_tid: usize,
    class: MmioClass,
}

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

static DISPLAY_FRAMEBUFFER: Spinlock<DisplayFramebufferState> = Spinlock::ranked(
    DisplayFramebufferState::Empty,
    crate::sync::rank::DISPLAY_FRAMEBUFFER,
);

/// Dynamically discovered PCIe BAR windows (base → len).
/// Populated by `pcie_ecam::init()` after the ECAM scan; consumed by
/// `request_mmio` when the caller holds `PcieDriverCap` (DEV_PCIE).
static PCIE_BARS: Spinlock<BTreeMap<usize, usize>> =
    Spinlock::ranked(BTreeMap::new(), crate::sync::rank::PCIE_BARS);

/// Test-hooks only: register a window with an explicit class, so a boot
/// self-test can prove class-selective revocation on boards whose allowlist
/// has no window of the class it needs.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_register_region(tid: usize, base: usize, len: usize, class: MmioClass) -> bool {
    if checked_mmio_end(base, len).is_err() {
        return false;
    }
    let mut registry = REGISTRY.lock();
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    registry
        .insert(
            base,
            MmioRegion {
                len,
                owner_tid: tid,
                class,
            },
        )
        .is_none()
}

#[cfg(not(cellos_posture = "release"))]
/// The per-arch static allowlist, for callers that must reason about which
/// windows this board can grant at all (the boot self-tests, which have to stay
/// board-agnostic).
pub(crate) fn allowed_windows() -> &'static [(usize, usize, u8)] {
    ALLOWED
}

/// Device class of the allowlist window containing `[base, end)`, when the
/// caller declared that class.
///
/// Returns the class rather than a bool because the grant records it: a revoke
/// must be able to tell which device class a window belongs to without
/// re-deriving it from a table that may have changed since.
fn static_range_class(
    allowed: &[(usize, usize, u8)],
    base: usize,
    end: usize,
    allowed_devices: u8,
) -> Option<u8> {
    allowed
        .iter()
        .find(|&&(window_base, window_len, class)| {
            window_base
                .checked_add(window_len)
                .is_some_and(|window_end| {
                    base >= window_base && end <= window_end && class & allowed_devices != 0
                })
        })
        .map(|&(_, _, class)| class)
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
    let mut bars = PCIE_BARS.lock();
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    bars.insert(base, len);
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

/// Record an ECAM claim with only the overlap check (no window policy).
///
/// The `RequestMmio` syscall must go through [`request_ecam_mmio`]; this raw
/// form exists for kernel-internal fixtures that register a window directly.
pub fn request_mmio_unchecked(tid: usize, base: usize, len: usize) -> ViResult<()> {
    let end = checked_mmio_end(base, len)?;
    let mut reg = REGISTRY.lock();
    for (&eb, region) in reg.iter() {
        let ee = eb + region.len;
        if !(end <= eb || base >= ee) {
            return Err(ViError::AlreadyExists);
        }
    }
    reg.insert(
        base,
        MmioRegion {
            len,
            owner_tid: tid,
            class: CLASS_ECAM,
        },
    );
    Ok(())
}

/// Platform Cell (`PlatformCap`) claim: only inside the kernel-admitted ECAM
/// window, and never over a managed RAM frame.
///
/// The claim is user-mapped by the caller, so a range reaching kernel RAM, page
/// tables or the kernel image would hand the cell the kernel. The RAM test is
/// kept even inside the window: the window comes from firmware (ACPI MCFG).
pub fn request_ecam_mmio(tid: usize, base: usize, len: usize) -> ViResult<()> {
    let end = checked_mmio_end(base, len)?;
    let window = crate::dev::pcie_ecam::admitted_ecam_range();
    let admitted = {
        let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        // No allocator means managed RAM is unknown: refuse rather than guess.
        guard.as_ref().is_some_and(|allocator| {
            ecam_claim_admitted(
                base,
                end,
                window,
                allocator
                    .managed_ranges()
                    .map(|range| (range.start, range.start + range.frames * 4096)),
            )
        })
    };
    if !admitted {
        log::warn!(
            "[mmio] platform claim {:#x}+{:#x} outside the ECAM window or over RAM",
            base,
            len
        );
        return Err(ViError::PermissionDenied);
    }
    request_mmio_unchecked(tid, base, len)
}

/// `[base, end)` lies inside `window` (`(base, len)`) and overlaps none of
/// `ram` (`(start, end)` ranges).
fn ecam_claim_admitted(
    base: usize,
    end: usize,
    window: Option<(usize, usize)>,
    mut ram: impl Iterator<Item = (usize, usize)>,
) -> bool {
    let Some((window_base, window_len)) = window else {
        return false;
    };
    let inside = base >= window_base
        && window_base
            .checked_add(window_len)
            .is_some_and(|window_end| end <= window_end);
    inside && !ram.any(|(start, ram_end)| base < ram_end && start < end)
}

/// Check if `[base, base + len)` falls inside the platform's DWC2 host controller MMIO window.
pub fn is_dwc2_mmio(base: usize, len: usize) -> bool {
    #[cfg(target_arch = "aarch64")]
    if let Some(dwc2) =
        <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::DWC2_WINDOW
    {
        return len > 0
            && base >= dwc2.base
            && base
                .checked_add(len)
                .is_some_and(|end| end <= dwc2.base + dwc2.size);
    }
    let _ = (base, len);
    false
}

/// Request exclusive ownership of a DWC2 MMIO region for a driver cell holding `UsbDriverCap`.
pub fn request_dwc2_mmio(tid: usize, base: usize, len: usize) -> ViResult<()> {
    if !is_dwc2_mmio(base, len) {
        return Err(ViError::PermissionDenied);
    }
    let end = checked_mmio_end(base, len)?;
    let mut reg = REGISTRY.lock();
    for (&eb, region) in reg.iter() {
        let ee = eb + region.len;
        if !(end <= eb || base >= ee) {
            return Err(ViError::AlreadyExists);
        }
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    reg.insert(
        base,
        MmioRegion {
            len,
            owner_tid: tid,
            class: CLASS_DWC2,
        },
    );
    Ok(())
}

/// Request exclusive ownership of `[base, base+len)` for task `tid`.
///
/// Returns:
/// - `Ok(())` — range is now owned by the caller; construct `MmioRegion` and
///   hand it to the Cell.
/// - `Err(PermissionDenied)` — range not in allowlist, or its device class is
///   not among `allowed_devices` (the cell's declared `mmio_devices`).
/// - `Err(AlreadyExists)` — range overlaps an already-granted region.
/// - `Err(InvalidInput)` — zero length or arithmetic overflow in `base + len`.
pub fn request_mmio(tid: usize, base: usize, len: usize, allowed_devices: u8) -> ViResult<()> {
    // 1. Allowlist check — the range must fall inside a known device window
    //    AND that window's device class must be one the cell declared.
    let end = checked_mmio_end(base, len)?;

    // PCIe path: validate against the dynamic BAR table populated by pcie_ecam.
    let class: MmioClass = if allowed_devices & DEV_PCIE != 0 {
        let bars = PCIE_BARS.lock();
        if !bars.get(&base).is_some_and(|&bar_len| len <= bar_len) {
            return Err(ViError::PermissionDenied);
        }
        DEV_PCIE as MmioClass
    } else {
        // SoC peripheral path: static per-arch allowlist.
        match static_range_class(ALLOWED, base, end, allowed_devices) {
            Some(class) => class as MmioClass,
            None => return Err(ViError::PermissionDenied),
        }
    };

    // 2. Overlap check — no two live cells may share a byte
    let mut reg = REGISTRY.lock();
    reg.retain(|&eb, region| {
        let ee = eb + region.len;
        if !(end <= eb || base >= ee) && region.owner_tid == tid {
            return false;
        }
        true
    });
    for (&eb, region) in reg.iter() {
        let ee = eb + region.len;
        if !(end <= eb || base >= ee) {
            if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
                if let Some(task) = sched.tasks.get(&region.owner_tid) {
                    if matches!(task.state, crate::task::tcb::TaskState::Terminated) {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            return Err(ViError::AlreadyExists);
        }
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    reg.insert(
        base,
        MmioRegion {
            len,
            owner_tid: tid,
            class,
        },
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Boot-time allowlist self-test (runs on every boot, QEMU-visible)
// ---------------------------------------------------------------------------

/// Boot self-test of the MMIO allowlist. The boot chain calls it on every
/// non-release target; tables with nothing pinned pass trivially.
#[cfg(not(cellos_posture = "release"))]
pub fn self_test() -> bool {
    #[cfg(target_arch = "aarch64")]
    match <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::SOC {
        hal::aarch64::backend::ArmSoc::Bcm2837 => return bcm2837_self_test(),
        hal::aarch64::backend::ArmSoc::QemuArmVirt => return qemu_arm_virt_self_test(),
        hal::aarch64::backend::ArmSoc::Bcm2711 | hal::aarch64::backend::ArmSoc::AppleT8103 => {}
    }
    true
}

/// Power-on self-test of the QEMU ARM virt production MMIO allowlist (Q5).
///
/// Pins, on the REAL table (`ALLOWED`): it is the virt table, entry for entry;
/// every window authorizes its own device class; and no window authorizes any
/// other class, so one peripheral class cannot claim another's controller.
#[cfg(target_arch = "aarch64")]
fn qemu_arm_virt_self_test() -> bool {
    if !allowlists_equal(ALLOWED, QEMU_ARM_VIRT_ALLOWED_SNAPSHOT) {
        log::error!("[selftest] mmio-allowlist: FAIL — selected table is not the QEMU virt table");
        return false;
    }
    for &(base, len, class) in ALLOWED {
        let end = base + len;
        if static_range_class(ALLOWED, base, end, class) != Some(class) {
            log::error!(
                "[selftest] mmio-allowlist: FAIL — window {base:#x} no longer authorizes \
                 class {class:#04x}"
            );
            return false;
        }
        if static_range_class(ALLOWED, base, end, !class).is_some() {
            log::error!(
                "[selftest] mmio-allowlist: FAIL — window {base:#x} authorized for a class \
                 other than {class:#04x}"
            );
            return false;
        }
    }
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
#[cfg(target_arch = "aarch64")]
fn bcm2837_self_test() -> bool {
    let mmio = hal_soc_bcm27xx::BCM2837.mmio;

    // 1. Positive control: the table is live.
    let mbox_base = mmio.mailbox_base;
    let mbox_end = mbox_base + mmio.mailbox_grant_size;
    if static_range_class(ALLOWED, mbox_base, mbox_end, DEV_DISPLAY).is_none() {
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
        if static_range_class(ALLOWED, dwc2_base, dwc2_end, class).is_some() {
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
            if static_range_class(ALLOWED, addr, addr + 4, class).is_some() {
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

/// Release all MMIO regions owned by task `tid`.
///
/// Retirement calls this for every member task of the retiring incarnation.
/// On RISC-V and AArch64 the released windows' USER pages go back to the boot
/// posture (kernel-only): `RequestMmio` installed them, so without this a dead
/// Cell's window would stay U-mode/EL0-reachable for every other Cell.
pub fn release_mmio_for(tid: usize) {
    let mut registry = REGISTRY.lock();
    #[cfg(all(any(target_arch = "riscv64", target_arch = "aarch64"), not(test)))]
    let released: alloc::vec::Vec<(usize, usize)> = registry
        .iter()
        .filter(|(_, region)| region.owner_tid == tid)
        .map(|(&base, region)| (base, region.len))
        .collect();
    registry.retain(|_base, region| region.owner_tid != tid);
    // REGISTRY stays held so no new owner can claim a page between the check
    // below and its PTE update; a page a live region still covers (a window
    // handed over from a Terminated owner before this reap) keeps USER.
    #[cfg(all(any(target_arch = "riscv64", target_arch = "aarch64"), not(test)))]
    for (base, len) in released {
        const PAGE: usize = 4096;
        for page in user_pages(base, len).step_by(PAGE) {
            let still_owned = registry
                .iter()
                .any(|(&live, region)| live < page + PAGE && page < live + region.len);
            if still_owned {
                continue;
            }
            if let Err(e) = crate::memory::paging::restore_mmio_boot_posture(page, PAGE) {
                log::error!("[mmio] release tid={tid}: page {page:#x} kept USER ({e:?})");
            }
        }
    }
    // A VideoCore allocation and its USER mapping remain valid until reboot.
    // Do not clear Active metadata or permit a restarted cell to replace it:
    // restart is fail-stop for display after registration.
}

/// Release the regions owned by task `tid` whose class is in `device_mask`, and
/// return their `(base, len)` so the caller can remove user accessibility.
///
/// Selective by construction: a class the caller did not name stays owned and
/// mapped, so revoking one device class never disarms another. Used by runtime
/// revocation (`sys_cap_revoke`); task retirement uses [`release_mmio_for`],
/// which drops every region regardless of class.
pub fn revoke_mmio_for(tid: usize, device_mask: MmioClass) -> alloc::vec::Vec<(usize, usize)> {
    let mut revoked = alloc::vec::Vec::new();
    if device_mask == 0 {
        return revoked;
    }
    REGISTRY.lock().retain(|&base, region| {
        let hit = region.owner_tid == tid && region.class & device_mask != 0;
        if hit {
            #[cfg(feature = "p01-trace")]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::DRIVER_SHARED,
            );
            revoked.push((base, region.len));
        }
        !hit
    });
    revoked
}

/// Return the task ID (TID) of the cell that currently owns the MMIO region
/// whose base address exactly matches `base`.
///
/// Returns `None` if no cell has requested that exact base address.
/// Used by the GPIO IRQ handler to route interrupts to the current MMIO owner.
pub fn lookup_mmio_owner(base: usize) -> Option<usize> {
    REGISTRY.lock().get(&base).map(|region| region.owner_tid)
}

/// Return whether task `tid` owns exactly `[base, base + len)`.
///
/// Display registration uses this rather than an overlap query: the VideoCore
/// mailbox authority is one fixed capability, so a subrange cannot be confused
/// with an independently granted peripheral window.
pub fn owns_exact_mmio(tid: usize, base: usize, len: usize) -> bool {
    REGISTRY
        .lock()
        .get(&base)
        .is_some_and(|region| region.len == len && region.owner_tid == tid)
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

/// Return the active firmware scanout geometry and base, if registered.
pub fn display_framebuffer_info() -> Option<(usize, usize, u16, u16, usize)> {
    match *DISPLAY_FRAMEBUFFER.lock() {
        DisplayFramebufferState::Active(framebuffer) => Some((
            framebuffer.base,
            framebuffer.size,
            framebuffer.width,
            framebuffer.height,
            framebuffer.pitch,
        )),
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
        release_mmio_for(owner.0 as usize);
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
static BDF_OWNERS: Spinlock<alloc::collections::BTreeMap<u32, usize>> = Spinlock::ranked(
    alloc::collections::BTreeMap::new(),
    crate::sync::rank::BDF_OWNERS,
);

/// Atomically claim an unowned PCIe BDF for task `tid`.
///
/// Repeated claims by the same task are idempotent. A live owner cannot be
/// displaced; task reaping releases its claims through [`release_bdfs_for`].
pub fn claim_bdf_owner(bdf: u32, tid: usize) -> bool {
    let mut owners = BDF_OWNERS.lock();
    match owners.get(&bdf).copied() {
        Some(owner) => owner == tid,
        None => {
            #[cfg(feature = "p01-trace")]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::DRIVER_SHARED,
            );
            owners.insert(bdf, tid);
            true
        }
    }
}

/// Return the task ID that currently owns `bdf`, or `None` if unowned.
pub fn owner_of_bdf(bdf: u32) -> Option<usize> {
    BDF_OWNERS.lock().get(&bdf).copied()
}

/// Every BDF that task `tid` currently owns.
pub fn bdfs_owned_by(tid: usize) -> alloc::vec::Vec<u32> {
    BDF_OWNERS
        .lock()
        .iter()
        .filter(|(_, &owner)| owner == tid)
        .map(|(&bdf, _)| bdf)
        .collect()
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

#[cfg(any(target_arch = "aarch64", target_arch = "riscv64", test))]
#[path = "resource_registry_mediation.rs"]
mod mediation;
#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
pub(crate) use mediation::user_pages;

#[cfg(test)]
#[path = "resource_registry_tests.rs"]
mod tests;
