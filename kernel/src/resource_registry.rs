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
//!    a task. Owners are task ids, the same key as the device records: the claiming
//!    task holds the window, so retirement releases it per member task. The
//!    released pages first return to their boot posture (kernel-only) on every
//!    hart, lowered without the registry lock while the window refuses every
//!    claim; a window whose lowering is unconfirmed stays owned by nobody until
//!    a retry the timer poll arms confirms it.
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
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

#[cfg(target_arch = "aarch64")]
#[path = "arch/aarch64/resource_registry.rs"]
mod aarch64;
#[cfg(target_arch = "aarch64")]
use aarch64::*;

/// SiFive GPIO0 for QEMU `sifive_u` machine (FU540/FU740).
/// The kernel serial driver owns NS16550 at 0x1000_0000 — excluded from allowlist.
#[cfg(target_arch = "riscv64")]
const ALLOWED: &[(usize, usize, u8)] = &[
    (0x1001_2000, 0x1000, DEV_GPIO), // SiFive GPIO0 — QEMU sifive_u machine
];

#[cfg(target_arch = "x86_64")]
const ALLOWED: &[(usize, usize, u8)] = &[];

#[cfg(not(cellos_wide))]
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

/// Current Driver Cells support bounded conventional BARs only.
///
/// ReBAR and larger accelerator apertures need a separate policy because they
/// materially widen a Cell's MMIO authority.
pub(crate) fn valid_pcie_bar_window(base: usize, len: usize) -> bool {
    const MAX_SUPPORTED_BAR_LEN: usize = 1 << 30;

    base != 0
        && len != 0
        && len <= MAX_SUPPORTED_BAR_LEN
        && len.is_power_of_two()
        && base & (len - 1) == 0
        && base.checked_add(len).is_some()
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
    if reg
        .iter()
        .any(|(&window_base, region)| claim_collides(window_base, region, base, end))
    {
        return Err(ViError::AlreadyExists);
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
        // No allocator means managed RAM is unknown: refuse rather than guess.
        crate::memory::frame::with(|allocator| {
            ecam_claim_admitted(
                base,
                end,
                window,
                allocator
                    .managed_ranges()
                    .map(|range| (range.start, range.start + range.frames * 4096)),
            )
        })
        .unwrap_or(false)
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

/// The platform's DWC2 host controller aperture `(base, size)`, if it has one.
pub(crate) fn dwc2_aperture() -> Option<(usize, usize)> {
    #[cfg(target_arch = "aarch64")]
    if let Some(dwc2) =
        <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::DWC2_WINDOW
    {
        return Some((dwc2.base, dwc2.size));
    }
    None
}

/// Check if `[base, base + len)` falls inside the platform's DWC2 host controller MMIO window.
pub fn is_dwc2_mmio(base: usize, len: usize) -> bool {
    dwc2_aperture().is_some_and(|(dwc2_base, dwc2_size)| {
        len > 0
            && base >= dwc2_base
            && base
                .checked_add(len)
                .is_some_and(|end| end <= dwc2_base + dwc2_size)
    })
}

/// Base of the DWC2 aperture when task `tid` owns a window inside it: the
/// one device the aperture is, whatever sub-window its driver claimed.
pub fn dwc2_aperture_owned_by(tid: usize) -> Option<usize> {
    let (base, _) = dwc2_aperture()?;
    REGISTRY
        .lock()
        .values()
        .any(|region| region.owner_tid == tid && region.class == CLASS_DWC2)
        .then_some(base)
}

/// Request exclusive ownership of a DWC2 MMIO region for a driver cell holding
/// `UsbDriverCap`: the aperture's device record is claimed first (its owner,
/// or its first claimant while free), and a first claim is undone when the
/// window is refused.
pub fn request_dwc2_mmio(tid: usize, base: usize, len: usize) -> ViResult<()> {
    if !is_dwc2_mmio(base, len) {
        return Err(ViError::PermissionDenied);
    }
    let end = checked_mmio_end(base, len)?;
    claim_device_then(tid, base, len, true, || {
        let mut reg = REGISTRY.lock();
        if reg
            .iter()
            .any(|(&window_base, region)| claim_collides(window_base, region, base, end))
        {
            return Err(ViError::AlreadyExists);
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
    })
}

/// Claim the device record holding window `[base, base + len)` for `tid`, then
/// run `grant`; a first claim of an MMIO device is undone when `grant` fails.
///
/// A PCI function's BAR is granted only to the function's owner (the 418/424
/// claimant), so no driver can map, reset or reprogram a device it was not
/// handed. A VirtIO slot or the DWC2 aperture is claimed by its first
/// requester and refused to everyone else until its owner is reaped. Lock
/// order: `DEVICES` is released before `grant` takes `REGISTRY`.
fn claim_device_then(
    tid: usize,
    base: usize,
    len: usize,
    dwc2: bool,
    grant: impl FnOnce() -> ViResult<()>,
) -> ViResult<()> {
    use crate::dev::ownership::{claim_window, unclaim, WindowClaim};
    let claim = claim_window(tid, base, len, dwc2);
    match claim {
        WindowClaim::NoDevice | WindowClaim::NotOwner => return Err(ViError::PermissionDenied),
        WindowClaim::Held => return Err(ViError::AlreadyExists),
        WindowClaim::Owned | WindowClaim::Claimed(_) => {}
    }
    let granted = grant();
    if let (Err(_), WindowClaim::Claimed(key)) = (&granted, claim) {
        unclaim(tid, key);
    }
    granted
}

/// Request exclusive ownership of `[base, base+len)` for task `tid`.
///
/// Returns:
/// - `Ok(())` — range is now owned by the caller; construct `MmioRegion` and
///   hand it to the Cell.
/// - `Err(PermissionDenied)` — range not in allowlist, or its device class is
///   not among `allowed_devices` (the cell's declared `mmio_devices`), or it is
///   the BAR of a PCI function the caller does not own.
/// - `Err(AlreadyExists)` — range overlaps an already-granted region, or is a
///   VirtIO slot another task owns.
/// - `Err(InvalidInput)` — zero length or arithmetic overflow in `base + len`.
pub fn request_mmio(tid: usize, base: usize, len: usize, allowed_devices: u8) -> ViResult<()> {
    // 1. Allowlist check — the range must fall inside a known device window
    //    AND that window's device class must be one the cell declared.
    let end = checked_mmio_end(base, len)?;

    // PCIe path: the window must be one a device record holds (a scanned or
    // registered BAR, or a VirtIO slot), and the caller must own that device.
    if allowed_devices & DEV_PCIE != 0 {
        return claim_device_then(tid, base, len, false, || {
            grant_window(tid, base, len, end, DEV_PCIE as MmioClass)
        });
    }
    // SoC peripheral path: static per-arch allowlist.
    match static_range_class(ALLOWED, base, end, allowed_devices) {
        Some(class) => grant_window(tid, base, len, end, class as MmioClass),
        None => Err(ViError::PermissionDenied),
    }
}

/// Record `[base, end)` as `tid`'s window of `class` unless it overlaps a
/// window another live task holds, or one being released or quarantined.
fn grant_window(tid: usize, base: usize, len: usize, end: usize, class: MmioClass) -> ViResult<()> {
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
        if claim_collides(eb, region, base, end) {
            // A quarantined or releasing window is never handed over
            // (`release_mmio_for`).
            if region.owner_tid == QUARANTINED_OWNER || region.owner_tid == RELEASING_OWNER {
                return Err(ViError::AlreadyExists);
            }
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

/// Release all MMIO regions owned by task `tid`.
///
/// Retirement calls this for every member task of the retiring incarnation.
/// The windows' USER pages first go back to the boot posture (kernel-only) on
/// every architecture: `RequestMmio` installed them, so without this a dead
/// Cell's window would stay U-mode/EL0/ring-3-reachable for every other Cell.
/// The windows pass to [`RELEASING_OWNER`] first, under `REGISTRY`, so every
/// claim on them is refused; the lowering then runs without `REGISTRY`,
/// which ranks after `KERNEL_ROOT` and the console that the lowering takes.
/// A window is released only once its lowering is in force on every hart. A
/// window whose lowering is not (a remote hart did not confirm the
/// invalidation, or the leaf could not be rewritten) stays registered to
/// [`QUARANTINED_OWNER`] — counted, logged, and handed to no Cell — because a
/// stale USER translation may survive on that hart, until
/// [`retry_quarantined_windows`] lowers it again and that lowering is
/// confirmed.
pub fn release_mmio_for(tid: usize) {
    let release = release_windows(tid, lower_run);
    for &(base, len, page, error) in &release.unlowered {
        log::error!(
            "[mmio] release tid={tid}: window {base:#x}+{len:#x} quarantined, page {page:#x} \
             may stay USER ({error:?}); handed out again only once a retry lowers it \
             ({} quarantined)",
            release.quarantined
        );
    }
    // The MMIO devices (VirtIO slot, DWC2 aperture) it claimed are free again;
    // a quarantined window still refuses every claim.
    crate::dev::ownership::release_mmio_devices_for(tid);
    // A VideoCore allocation and its USER mapping remain valid until reboot.
    // Do not clear Active metadata or permit a restarted cell to replace it:
    // restart is fail-stop for display after registration.
}

/// Owner of a released window whose USER pages are not provably gone on every
/// hart. No task id is ever `usize::MAX`, so no owner query matches it, and
/// every claim overlapping it is refused until [`retry_quarantined_windows`]
/// confirms its lowering.
const QUARANTINED_OWNER: usize = usize::MAX;

/// Owner of a window whose release lowering is in progress without
/// `REGISTRY` ([`release_windows`]). No task id reaches it either: no owner
/// query matches it, every claim overlapping the window or sharing one of its
/// released pages is refused, and only the release that set it changes it.
const RELEASING_OWNER: usize = usize::MAX - 1;

/// Windows registered to [`QUARANTINED_OWNER`], so the timer poll and the
/// housekeeping retry cost one load while there are none. Written only under
/// `REGISTRY`.
static QUARANTINED_WINDOWS: AtomicUsize = AtomicUsize::new(0);

/// Set by the hart-0 timer poll ([`poll_quarantined_windows`]) and consumed
/// only by [`retry_quarantined_windows`]: a housekeeping pass armed by any
/// other producer (a zombie reap, a grant reap, a tag wait ending) does not
/// lower the quarantined windows again.
static RETRY_ARMED: AtomicBool = AtomicBool::new(false);

/// Hart-0 timer ticks between two retries of the quarantined windows' lowering
/// (about 1.3 s at 10 ms): the backoff that keeps a never-confirmed lowering
/// from re-arming the housekeeping pass on every tick.
const RETRY_EVERY_TICKS: usize = 128;

/// A released window `(base, len)` whose lowering failed at `page`.
type Unlowered = (usize, usize, usize, crate::memory::paging::PageTableError);

/// A window taken out of service for its lowering: registered to
/// [`RELEASING_OWNER`] from [`begin_release`] to [`finish_release`].
struct Releasing {
    base: usize,
    len: usize,
    /// Page runs `(start, pages)` its release lowers ([`lowering_runs`]).
    runs: alloc::vec::Vec<(usize, usize)>,
}

/// What one [`release_windows`] did.
struct Release {
    /// `(base, len)` of every window released.
    released: alloc::vec::Vec<(usize, usize)>,
    unlowered: alloc::vec::Vec<Unlowered>,
    /// Windows quarantined after it, all owners.
    quarantined: usize,
}

/// Release `owner`'s windows: mark them releasing under `REGISTRY`, lower
/// their runs through `lower` without it, then re-take it to release each
/// window whose lowering is in force and quarantine the others.
fn release_windows(
    owner: usize,
    lower: impl FnMut(usize, usize) -> Result<(), (usize, crate::memory::paging::PageTableError)>,
) -> Release {
    let windows = begin_release(&mut REGISTRY.lock(), owner);
    let unlowered = lower_windows(&windows, lower);
    let quarantined = finish_release(&mut REGISTRY.lock(), &windows, &unlowered);
    let released = windows
        .iter()
        .filter(|window| !unlowered.iter().any(|failed| failed.0 == window.base))
        .map(|window| (window.base, window.len))
        .collect();
    Release {
        released,
        unlowered,
        quarantined,
    }
}

/// Pass `owner`'s windows to [`RELEASING_OWNER`] and return them with the
/// page runs to lower, computed while `REGISTRY` still shows which pages
/// another live window covers.
fn begin_release(
    registry: &mut BTreeMap<usize, MmioRegion>,
    owner: usize,
) -> alloc::vec::Vec<Releasing> {
    let windows: alloc::vec::Vec<Releasing> = registry
        .iter()
        .filter(|(_, region)| region.owner_tid == owner)
        .map(|(&base, region)| Releasing {
            base,
            len: region.len,
            runs: lowering_runs(registry, owner, base, region.len),
        })
        .collect();
    for window in &windows {
        if let Some(region) = registry.get_mut(&window.base) {
            region.owner_tid = RELEASING_OWNER;
        }
    }
    windows
}

/// The pages `RequestMmio` made USER for `[base, base + len)`: the whole pages
/// on RISC-V and AArch64 (`user_pages`), every page the window touches on
/// x86_64 (`map_mmio_user_x86` maps the page-rounded window).
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
fn released_pages(base: usize, len: usize) -> core::ops::Range<usize> {
    user_pages(base, len)
}

/// [`released_pages`] where `RequestMmio` maps the page-rounded window.
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
fn released_pages(base: usize, len: usize) -> core::ops::Range<usize> {
    const PAGE: usize = 4096;
    match base
        .checked_add(len)
        .and_then(|end| end.checked_add(PAGE - 1))
    {
        Some(end) if len != 0 => (base & !(PAGE - 1))..(end & !(PAGE - 1)),
        _ => 0..0,
    }
}

/// The released pages of window `[base, base + len)` held by `owner` that its
/// release lowers, as contiguous runs `(start, pages)`. A page another
/// owner's live window still covers (one handed over from a Terminated owner
/// before this reap) keeps USER; a quarantined or releasing window has no
/// owner to keep it for.
fn lowering_runs(
    registry: &BTreeMap<usize, MmioRegion>,
    owner: usize,
    base: usize,
    len: usize,
) -> alloc::vec::Vec<(usize, usize)> {
    const PAGE: usize = 4096;
    let mut runs: alloc::vec::Vec<(usize, usize)> = alloc::vec::Vec::new();
    for page in released_pages(base, len).step_by(PAGE) {
        let still_owned = registry.iter().any(|(&live, other)| {
            other.owner_tid != owner
                && other.owner_tid != QUARANTINED_OWNER
                && other.owner_tid != RELEASING_OWNER
                && live < page + PAGE
                && page < live + other.len
        });
        if still_owned {
            continue;
        }
        match runs.last_mut() {
            Some((start, pages)) if *start + *pages * PAGE == page => *pages += 1,
            _ => runs.push((page, 1)),
        }
    }
    runs
}

/// Lower every run of `windows` through `lower`, with no `REGISTRY` held.
/// Every run is attempted; a window is reported with its first page that
/// failed.
fn lower_windows(
    windows: &[Releasing],
    mut lower: impl FnMut(usize, usize) -> Result<(), (usize, crate::memory::paging::PageTableError)>,
) -> alloc::vec::Vec<Unlowered> {
    let mut unlowered = alloc::vec::Vec::new();
    for window in windows {
        let mut failed = None;
        for &(start, pages) in &window.runs {
            if let Err(failure) = lower(start, pages) {
                failed.get_or_insert(failure);
            }
        }
        if let Some((page, error)) = failed {
            unlowered.push((window.base, window.len, page, error));
        }
    }
    unlowered
}

/// Return a run to the boot posture on every hart: one shared flush per
/// 2 MiB step, absent leaves included (`paging::restore_mmio_run`).
#[cfg(all(cellos_wide, not(test)))]
fn lower_run(
    start: usize,
    pages: usize,
) -> Result<(), (usize, crate::memory::paging::PageTableError)> {
    crate::memory::paging::restore_mmio_run(start, pages).map(drop)
}

/// Nothing to lower: host tests have no page tables, and the 32-bit targets
/// never map a window USER.
#[cfg(not(all(cellos_wide, not(test))))]
fn lower_run(
    _start: usize,
    _pages: usize,
) -> Result<(), (usize, crate::memory::paging::PageTableError)> {
    Ok(())
}

/// Release each of `windows` whose lowering is in force on every hart and
/// pass the others (`unlowered`) to [`QUARANTINED_OWNER`]. A releasing window
/// is changed by no other path, so each is still registered as it was left.
/// Returns the windows quarantined after it.
fn finish_release(
    registry: &mut BTreeMap<usize, MmioRegion>,
    windows: &[Releasing],
    unlowered: &[Unlowered],
) -> usize {
    for window in windows {
        let Some(region) = registry
            .get_mut(&window.base)
            .filter(|region| region.owner_tid == RELEASING_OWNER && region.len == window.len)
        else {
            continue;
        };
        if unlowered.iter().any(|failed| failed.0 == window.base) {
            region.owner_tid = QUARANTINED_OWNER;
        } else {
            registry.remove(&window.base);
        }
    }
    count_quarantined(registry)
}

/// Lower every quarantined window again and release those whose lowering is
/// now in force on every hart. The housekeeping pass calls it; it acts only
/// when the timer poll armed it ([`RETRY_ARMED`]) and no remote hart owes an
/// invalidation, and it lowers without `REGISTRY`, as [`release_mmio_for`]
/// does. Returns the windows still quarantined.
pub fn retry_quarantined_windows() -> usize {
    if !RETRY_ARMED.swap(false, Ordering::AcqRel)
        || QUARANTINED_WINDOWS.load(Ordering::Acquire) == 0
    {
        return QUARANTINED_WINDOWS.load(Ordering::Acquire);
    }
    // A hart that still owes an invalidation would leave this lowering
    // unconfirmed too: the next poll arms the retry again.
    #[cfg(cellos_wide)]
    if crate::memory::tlb_shootdown::tag_invalidation_outstanding().is_some() {
        return QUARANTINED_WINDOWS.load(Ordering::Acquire);
    }
    let release = release_windows(QUARANTINED_OWNER, lower_run);
    for (base, len) in release.released {
        log::warn!(
            "[mmio] quarantined window {base:#x}+{len:#x} lowered on every hart: released \
             ({} still quarantined)",
            release.quarantined
        );
    }
    release.quarantined
}

/// Hart-0 timer hook: while a window is quarantined, arm the retry and the
/// housekeeping pass every [`RETRY_EVERY_TICKS`] once no remote hart owes an
/// invalidation, so [`retry_quarantined_windows`] runs when its flush can
/// confirm. One load while nothing is quarantined; never waits.
#[cfg(cellos_wide)]
pub fn poll_quarantined_windows() {
    if QUARANTINED_WINDOWS.load(Ordering::Acquire) == 0
        || !crate::task::system_ticks().is_multiple_of(RETRY_EVERY_TICKS)
    {
        return;
    }
    if crate::memory::tlb_shootdown::tag_invalidation_outstanding().is_none() {
        RETRY_ARMED.store(true, Ordering::Release);
        crate::task::note_housekeeping();
    }
}

/// Windows registered to [`QUARANTINED_OWNER`]; refreshes
/// [`QUARANTINED_WINDOWS`]. The caller holds `REGISTRY`.
fn count_quarantined(registry: &BTreeMap<usize, MmioRegion>) -> usize {
    let quarantined = registry
        .values()
        .filter(|region| region.owner_tid == QUARANTINED_OWNER)
        .count();
    QUARANTINED_WINDOWS.store(quarantined, Ordering::Release);
    quarantined
}

/// Does a claim of `[base, end)` collide with the registered window at
/// `window_base`? A window being released also refuses a claim that only
/// shares one of its released pages: its lowering runs without `REGISTRY` and
/// would take USER from the new owner's page.
fn claim_collides(window_base: usize, region: &MmioRegion, base: usize, end: usize) -> bool {
    let window_end = window_base + region.len;
    let (start, stop) = if region.owner_tid == RELEASING_OWNER {
        let pages = released_pages(window_base, region.len);
        (window_base.min(pages.start), window_end.max(pages.end))
    } else {
        (window_base, window_end)
    };
    base < stop && start < end
}

/// The windows [`revoke_mmio_for`] would release, without releasing them: a
/// revoke lowers their PTEs first and releases exactly these only once the
/// lowering is in force on every hart ([`release_mmio_windows`]).
pub fn mmio_windows_for(tid: usize, device_mask: MmioClass) -> alloc::vec::Vec<(usize, usize)> {
    if device_mask == 0 {
        return alloc::vec::Vec::new();
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::DRIVER_SHARED,
    );
    REGISTRY
        .lock()
        .iter()
        .filter(|(_, region)| region.owner_tid == tid && region.class & device_mask != 0)
        .map(|(&base, region)| (base, region.len))
        .collect()
}

/// Release exactly `windows` of `tid` (bases from [`mmio_windows_for`]); a
/// window claimed since is untouched.
pub fn release_mmio_windows(tid: usize, windows: &[(usize, usize)]) {
    REGISTRY.lock().retain(|base, region| {
        !(region.owner_tid == tid && windows.contains(&(*base, region.len)))
    });
    crate::dev::ownership::release_mmio_devices_in(tid, windows);
}

/// Release the regions owned by task `tid` whose class is in `device_mask`, and
/// return their `(base, len)` so the caller can remove user accessibility.
///
/// Selective by construction: a class the caller did not name stays owned and
/// mapped, so revoking one device class never disarms another. Used by the
/// MMIO revoke self-test; `sys_cap_revoke` lowers first ([`mmio_windows_for`]),
/// and task retirement uses [`release_mmio_for`], which drops every region
/// regardless of class.
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
/// Returns `None` if no cell has requested that exact base address, or the
/// window is being released or quarantined (no cell owns it any more).
/// Used by the GPIO IRQ handler to route interrupts to the current MMIO owner.
pub fn lookup_mmio_owner(base: usize) -> Option<usize> {
    REGISTRY
        .lock()
        .get(&base)
        .map(|region| region.owner_tid)
        .filter(|&tid| tid != QUARANTINED_OWNER && tid != RELEASING_OWNER)
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
// PCIe BDF ownership: views over the device records (`dev::ownership`)
// ---------------------------------------------------------------------------

/// Atomically claim an unowned PCIe BDF for task `tid`.
///
/// Repeated claims by the same task are idempotent. A live owner cannot be
/// displaced; task reaping releases its claims through [`release_bdfs_for`].
pub fn claim_bdf_owner(bdf: u32, tid: usize) -> bool {
    crate::dev::ownership::claim_pci(bdf, tid)
}

/// Return the task ID that currently owns `bdf`, or `None` if unowned.
pub fn owner_of_bdf(bdf: u32) -> Option<usize> {
    crate::dev::ownership::pci_owner(bdf)
}

/// Every BDF that task `tid` currently owns.
pub fn bdfs_owned_by(tid: usize) -> alloc::vec::Vec<u32> {
    crate::dev::ownership::pci_functions_of(tid)
}

/// Release all BDF ownerships held by task `tid` (called on Cell exit).
pub fn release_bdfs_for(tid: usize) {
    crate::dev::ownership::release_pci_for(tid);
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
