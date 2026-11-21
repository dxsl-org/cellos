use super::{
    checked_mmio_end, ecam_claim_admitted, owns_exact_mmio, request_ecam_mmio, static_range_class,
    valid_pcie_bar_window, DEV_GPIO, DEV_I2C, DEV_SPI,
};
use crate::dev::ownership::{claim_pci, pci_owner, release_for, Scope};
use types::ViError;

const BCM_WINDOWS: &[(usize, usize, u8)] = &[
    (0x3F20_0000, 0x1000, DEV_GPIO),
    (0x3F20_4000, 0x1000, DEV_SPI),
    (0x3F80_4000, 0x1000, DEV_I2C),
];

#[test]
fn mmio_ranges_reject_zero_length_and_overflow() {
    assert_eq!(checked_mmio_end(0x1000, 0), Err(ViError::InvalidInput));
    assert_eq!(checked_mmio_end(usize::MAX, 2), Err(ViError::InvalidInput));
    assert_eq!(checked_mmio_end(0x1000, 0x1000), Ok(0x2000));
}

#[test]
fn controller_classes_only_authorize_their_own_window() {
    assert_eq!(
        static_range_class(BCM_WINDOWS, 0x3F80_4000, 0x3F80_5000, DEV_I2C),
        Some(DEV_I2C)
    );
    assert_eq!(
        static_range_class(BCM_WINDOWS, 0x3F20_4000, 0x3F20_5000, DEV_SPI),
        Some(DEV_SPI)
    );
    assert_eq!(
        static_range_class(BCM_WINDOWS, 0x3F20_4000, 0x3F20_5000, DEV_I2C),
        None
    );
    assert_eq!(
        static_range_class(BCM_WINDOWS, 0x3F80_4000, 0x3F80_5000, DEV_SPI),
        None
    );
}

#[test]
fn controller_grants_cannot_escape_or_reuse_gpio_authority() {
    assert_eq!(
        static_range_class(BCM_WINDOWS, 0x3F80_4000, 0x3F80_5001, DEV_I2C),
        None
    );
    assert_eq!(
        static_range_class(BCM_WINDOWS, 0x3F20_4000, 0x3F20_5000, DEV_GPIO),
        None
    );
}

#[test]
fn pcie_bar_windows_reject_unbounded_or_misaligned_authority() {
    assert!(valid_pcie_bar_window(0xF000_0000, 0x20_000));
    assert!(!valid_pcie_bar_window(0, 0x4000));
    assert!(!valid_pcie_bar_window(0xF000_0000, 0));
    assert!(!valid_pcie_bar_window(0xF000_1000, 0x4000));
    assert!(!valid_pcie_bar_window(0xF000_0000, 0x3000));
    assert!(!valid_pcie_bar_window(0x8000_0000, (1 << 30) + 1));
    assert!(!valid_pcie_bar_window(usize::MAX - 0xFFF, 0x1000));
}

/// Phase 08C non-reuse: a retirement that cannot prove the function quiescent
/// (no ECAM window on the host, so `quiesce_function` refuses) keeps the dead
/// owner's BDF, and a replacement cannot claim it.
#[test]
fn unproven_quiescence_retains_the_bdf_against_a_replacement() {
    const BDF: u32 = 0x00_E1;
    const DEAD_TID: usize = 0x5_08C1;
    const REPLACEMENT_TID: usize = 0x5_08C2;
    release_for(DEAD_TID, Scope::Retired);
    release_for(REPLACEMENT_TID, Scope::Retired);
    assert!(claim_pci(BDF, DEAD_TID));

    assert!(
        !crate::task::teardown_dma_authority(DEAD_TID),
        "teardown must report retention"
    );
    assert_eq!(pci_owner(BDF), Some(DEAD_TID));
    assert!(!claim_pci(BDF, REPLACEMENT_TID));

    release_for(DEAD_TID, Scope::Retired);
}

/// X1: a PCIe BAR goes only to the owner of its function, and a VirtIO slot's
/// first claim is undone when its window is refused.
#[test]
fn a_device_window_goes_only_to_the_owner_of_its_device() {
    use super::{
        lookup_mmio_owner, release_mmio_for, release_windows, request_mmio, request_mmio_unchecked,
        retry_quarantined_windows, DEV_PCIE, RETRY_ARMED,
    };
    use crate::dev::ownership::{self, DeviceKind};
    use crate::memory::paging::PageTableError;
    use core::sync::atomic::Ordering;
    const BDF: u32 = 0x00_D1;
    const BAR: usize = 0xFD10_0000;
    const SLOT: usize = 0x7D10_0000;
    const LINE: u8 = 0x3D;
    const OWNER: usize = 0x7D01;
    const OTHER: usize = 0x7D02;
    let _guard = crate::TEST_STATE_LOCK.lock();
    ownership::register_pci_bar(BDF, BAR, 0x4000);

    // Before: any PcieDriverCap cell could claim a BAR it was never handed.
    assert_eq!(
        request_mmio(OWNER, BAR, 0x4000, DEV_PCIE),
        Err(ViError::PermissionDenied)
    );
    assert!(claim_pci(BDF, OTHER));
    assert_eq!(
        request_mmio(OWNER, BAR, 0x4000, DEV_PCIE),
        Err(ViError::PermissionDenied)
    );
    release_for(OTHER, Scope::Retired);
    assert!(claim_pci(BDF, OWNER));
    assert_eq!(request_mmio(OWNER, BAR, 0x4000, DEV_PCIE), Ok(()));
    assert_eq!(lookup_mmio_owner(BAR), Some(OWNER));
    release_mmio_for(OWNER);
    release_for(OWNER, Scope::Retired);

    // A free slot whose window is still quarantined: the claim is refused and
    // the record is left free.
    ownership::register_mmio_device(SLOT, 0x1000, DeviceKind::VirtioSlot, Some(LINE));
    assert_eq!(request_mmio_unchecked(OTHER, SLOT, 0x1000), Ok(()));
    let _ = release_windows(OTHER, |start, _, _| {
        Err((start, PageTableError::InvalidationUnconfirmed))
    });
    assert_eq!(
        request_mmio(OWNER, SLOT, 0x1000, DEV_PCIE),
        Err(ViError::AlreadyExists)
    );
    assert!(
        !ownership::irq_owned_by(OWNER, LINE),
        "refused claim rolled back"
    );
    RETRY_ARMED.store(true, Ordering::Release);
    retry_quarantined_windows();

    assert_eq!(request_mmio(OWNER, SLOT, 0x1000, DEV_PCIE), Ok(()));
    assert!(ownership::irq_owned_by(OWNER, LINE));
    assert_eq!(
        request_mmio(OTHER, SLOT, 0x1000, DEV_PCIE),
        Err(ViError::AlreadyExists)
    );
    release_mmio_for(OWNER);
    release_for(OWNER, Scope::Retired);
    assert!(!ownership::irq_owned_by(OWNER, LINE));
}

/// A claim overlapping another owner's window reads owner liveness without
/// holding RESOURCE_REGISTRY (SCHEDULER ranks above it; a held REGISTRY here
/// panics the host rank check): `request_mmio`'s device path reaches
/// `grant_window` with a collider, refuses the overlap while its holder is live
/// and keeps the holder's window, then grants it once the holder is gone.
#[test]
fn a_claim_over_a_held_window_is_refused_without_a_rank_inversion() {
    use super::{
        lookup_mmio_owner, release_mmio_for, request_mmio, request_mmio_unchecked, DEV_PCIE,
    };
    use crate::sched::scheduler::Scheduler;
    use crate::task::{tcb::Task, SCHEDULER};
    const BDF: u32 = 0x00_D2;
    const BAR: usize = 0xFD20_0000;
    const HELD: usize = BAR + 0x1000;
    const HOLDER: usize = 0x7D11;
    const OTHER: usize = 0x7D12;
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    scheduler.tasks.insert(
        HOLDER,
        alloc::boxed::Box::new(Task::new(
            HOLDER,
            types::CellId(83),
            "mmio-holder",
            alloc::vec::Vec::new(),
        )),
    );
    *SCHEDULER.lock() = Some(scheduler);
    crate::dev::ownership::register_pci_bar(BDF, BAR, 0x2000);
    assert_eq!(request_mmio_unchecked(HOLDER, HELD, 0x1000), Ok(()));
    assert!(claim_pci(BDF, OTHER));

    assert_eq!(
        request_mmio(OTHER, BAR, 0x2000, DEV_PCIE),
        Err(ViError::AlreadyExists)
    );
    assert_eq!(lookup_mmio_owner(HELD), Some(HOLDER));
    assert_eq!(lookup_mmio_owner(BAR), None);

    // Gone from the scheduler, the holder is seen dead: the overlap is granted.
    SCHEDULER
        .lock()
        .as_mut()
        .expect("installed above")
        .tasks
        .remove(&HOLDER);
    assert_eq!(request_mmio(OTHER, BAR, 0x2000, DEV_PCIE), Ok(()));
    assert_eq!(lookup_mmio_owner(BAR), Some(OTHER));

    release_mmio_for(OTHER);
    release_mmio_for(HOLDER);
    release_for(OTHER, Scope::Retired);
    *SCHEDULER.lock() = saved;
}

/// Phase 16: a released window whose lowering is not confirmed on every hart
/// stays registered, owned by nobody, and no later claim or release frees it;
/// the confirmed window of the same release is freed. Only a retry the timer
/// poll armed lowers it again, and releases it once that lowering is in force.
#[test]
fn an_unlowered_window_is_quarantined_until_an_armed_retry_lowers_it() {
    use super::{
        lookup_mmio_owner, release_mmio_for, release_windows, request_mmio_unchecked,
        retry_quarantined_windows, QUARANTINED_OWNER, REGISTRY, RETRY_ARMED,
    };
    use crate::memory::paging::PageTableError;
    use core::sync::atomic::Ordering;
    const OWNER: usize = 0x7E2B;
    const UNLOWERED: usize = 0x7E2B_0000_0000;
    const LOWERED: usize = 0x7E2B_0000_1000;
    let _guard = crate::TEST_STATE_LOCK.lock();
    let quarantined = || {
        REGISTRY
            .lock()
            .values()
            .filter(|region| region.owner_tid == QUARANTINED_OWNER)
            .count()
    };
    assert!(request_mmio_unchecked(OWNER, UNLOWERED, 0x1000).is_ok());
    assert!(request_mmio_unchecked(OWNER, LOWERED, 0x1000).is_ok());

    let before = quarantined();
    let release = release_windows(OWNER, |start, _, _| {
        if start == UNLOWERED {
            Err((start, PageTableError::InvalidationUnconfirmed))
        } else {
            Ok(())
        }
    });
    assert_eq!(release.released, [(LOWERED, 0x1000)]);
    assert_eq!(release.quarantined, before + 1);

    assert_eq!(lookup_mmio_owner(UNLOWERED), None);
    assert!(!owns_exact_mmio(OWNER, UNLOWERED, 0x1000));
    assert_eq!(
        request_mmio_unchecked(OWNER + 1, UNLOWERED, 0x1000),
        Err(ViError::AlreadyExists)
    );
    // A later release of the same (reused) tid does not free it either.
    release_mmio_for(OWNER);
    assert_eq!(
        request_mmio_unchecked(OWNER, UNLOWERED, 0x1000),
        Err(ViError::AlreadyExists)
    );
    assert_eq!(quarantined(), before + 1);

    // The window whose lowering was confirmed is free for the next owner.
    assert!(request_mmio_unchecked(OWNER + 1, LOWERED, 0x1000).is_ok());
    release_mmio_for(OWNER + 1);

    // A housekeeping pass another producer armed does not lower it again.
    RETRY_ARMED.store(false, Ordering::Release);
    assert_eq!(retry_quarantined_windows(), before + 1);
    assert_eq!(
        request_mmio_unchecked(OWNER + 1, UNLOWERED, 0x1000),
        Err(ViError::AlreadyExists)
    );

    // The timer poll's arming does, once (host tests have no page tables: the
    // lowering is in force), and releases it to the next claim.
    RETRY_ARMED.store(true, Ordering::Release);
    assert_eq!(retry_quarantined_windows(), 0);
    assert!(!RETRY_ARMED.load(Ordering::Acquire));
    assert!(request_mmio_unchecked(OWNER + 1, UNLOWERED, 0x1000).is_ok());
    release_mmio_for(OWNER + 1);
}

/// Phase 16 (MMIO lowering off the registry lock): a release lowers its
/// windows with `REGISTRY` free (it ranks after `KERNEL_ROOT` and the console
/// that the lowering takes), and meanwhile the window has no owner and refuses
/// every claim on it, including one that only shares the page being lowered.
#[test]
fn a_release_lowers_without_the_registry_and_refuses_claims_meanwhile() {
    use super::{lookup_mmio_owner, release_windows, request_mmio_unchecked, REGISTRY};
    const OWNER: usize = 0x7E2C;
    const PAGE: usize = 0x7E2C_0000_0000;
    // A sub-page window: x86_64 (this host) lowers the page it touches.
    const BASE: usize = PAGE + 0x10;
    const NEIGHBOUR: usize = PAGE + 0x800;
    let _guard = crate::TEST_STATE_LOCK.lock();
    assert!(request_mmio_unchecked(OWNER, BASE, 0x20).is_ok());

    let mut lowered = alloc::vec::Vec::new();
    let release = release_windows(OWNER, |start, pages, _| {
        assert!(REGISTRY.try_lock().is_some(), "lowering under REGISTRY");
        assert_eq!(lookup_mmio_owner(BASE), None);
        assert!(!owns_exact_mmio(OWNER, BASE, 0x20));
        assert_eq!(
            request_mmio_unchecked(OWNER + 1, BASE, 0x20),
            Err(ViError::AlreadyExists)
        );
        assert_eq!(
            request_mmio_unchecked(OWNER + 1, NEIGHBOUR, 0x10),
            Err(ViError::AlreadyExists)
        );
        lowered.push((start, pages));
        Ok(())
    });
    assert_eq!(lowered, [(PAGE, 1)]);
    assert_eq!(release.released, [(BASE, 0x20)]);
    assert!(release.unlowered.is_empty());

    // Released: the page is free for the neighbour's claim.
    assert!(request_mmio_unchecked(OWNER + 1, NEIGHBOUR, 0x10).is_ok());
    super::release_mmio_for(OWNER + 1);
}

/// One release waits for at most one unconfirmed flush: the flag the first
/// failing run clears reaches every later run of every window of that release,
/// and the next release starts waiting again.
#[test]
fn one_unconfirmed_flush_stops_every_later_lowering_of_the_release_waiting() {
    use super::{release_windows, request_mmio_unchecked, QUARANTINED_OWNER};
    use crate::memory::paging::PageTableError;
    const OWNER: usize = 0x7E2E;
    const BASE: usize = 0x7E2E_0000_0000;
    const PAGE: usize = 0x1000;
    let _guard = crate::TEST_STATE_LOCK.lock();
    // Three windows, the first two pages apart so each window is its own run.
    for window in 0..3 {
        assert!(request_mmio_unchecked(OWNER, BASE + 2 * window * PAGE, PAGE).is_ok());
    }

    let mut waited = alloc::vec::Vec::new();
    let release = release_windows(OWNER, |start, _, await_flush| {
        waited.push(*await_flush);
        if start == BASE {
            // `restore_mmio_run` clears the flag when its flush is unconfirmed.
            *await_flush = false;
            Err((start, PageTableError::InvalidationUnconfirmed))
        } else {
            Ok(())
        }
    });
    assert_eq!(waited, [true, false, false]);
    assert_eq!(release.unlowered.len(), 1);

    // The quarantined window's retry is a new release: it waits again.
    let mut retried = alloc::vec::Vec::new();
    let retry = release_windows(QUARANTINED_OWNER, |_, _, await_flush| {
        retried.push(*await_flush);
        Ok(())
    });
    assert_eq!(retried, [true]);
    assert_eq!(retry.released, [(BASE, PAGE)]);
}

/// A release lowers its window's pages as contiguous runs, skips a page
/// another owner's live window still covers, and keeps nothing for a
/// quarantined or releasing window.
#[test]
fn a_release_lowers_contiguous_runs_and_keeps_a_page_a_live_window_covers() {
    use super::{lowering_runs, MmioRegion, CLASS_ECAM, QUARANTINED_OWNER, RELEASING_OWNER};
    const BASE: usize = 0x7E2D_0000_0000;
    const PAGE: usize = 0x1000;
    let region = |owner_tid, len| MmioRegion {
        len,
        owner_tid,
        class: CLASS_ECAM,
    };
    let mut registry = alloc::collections::BTreeMap::new();
    registry.insert(BASE, region(1, 6 * PAGE));
    // A live window handed over on page 2.
    registry.insert(BASE + 2 * PAGE + 0x10, region(2, 0x10));
    // Quarantined on page 4, releasing on page 5: neither keeps its page.
    registry.insert(BASE + 4 * PAGE, region(QUARANTINED_OWNER, PAGE));
    registry.insert(BASE + 5 * PAGE, region(RELEASING_OWNER, PAGE));
    assert_eq!(
        lowering_runs(&registry, 1, BASE, 6 * PAGE),
        [(BASE, 2), (BASE + 3 * PAGE, 3)]
    );
}

/// H-3: the PlatformCap bypass may claim only inside the admitted ECAM window
/// and never over managed RAM, even when firmware places the window on RAM.
#[test]
fn ecam_claim_is_confined_to_the_window_and_refuses_ram() {
    const WINDOW: Option<(usize, usize)> = Some((0xB000_0000, 0x1000_0000));
    let no_ram = || core::iter::empty::<(usize, usize)>();
    let ram = || [(0x0010_0000, 0x8000_0000), (0x1_0000_0000, 0x2_0000_0000)].into_iter();

    assert!(ecam_claim_admitted(0xB000_0000, 0xC000_0000, WINDOW, ram()));
    assert!(ecam_claim_admitted(0xB010_0000, 0xB020_0000, WINDOW, ram()));
    // Kernel RAM / image, a straddle of the window end, and no window at all.
    assert!(!ecam_claim_admitted(
        0x0020_0000,
        0x0030_0000,
        WINDOW,
        ram()
    ));
    assert!(!ecam_claim_admitted(
        0xBFFF_F000,
        0xC000_1000,
        WINDOW,
        no_ram()
    ));
    assert!(!ecam_claim_admitted(
        0xB000_0000,
        0xB010_0000,
        None,
        no_ram()
    ));
    // A firmware window placed over RAM still refuses the overlapping bytes.
    let ram_window = Some((0x7000_0000, 0x2000_0000));
    assert!(!ecam_claim_admitted(
        0x7FF0_0000,
        0x8010_0000,
        ram_window,
        ram()
    ));
    assert!(ecam_claim_admitted(
        0x8000_0000,
        0x8010_0000,
        ram_window,
        ram()
    ));
}

/// Without an admitted window (host build: no MCFG) every claim is refused and
/// nothing is recorded.
#[test]
fn ecam_request_without_a_window_records_nothing() {
    const TID: usize = 0x5_0EC0;
    assert_eq!(
        request_ecam_mmio(TID, 0x20_0000, 0x1000),
        Err(ViError::PermissionDenied)
    );
    assert!(!owns_exact_mmio(TID, 0x20_0000, 0x1000));
}
