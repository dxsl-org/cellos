//! Boot self-test for selective MMIO revocation (`.agents/260712-1901` P03).
//!
//! Runtime revocation of an `mmio_devices` bit must (a) release only the windows
//! of the revoked device class and (b) take the window's *user* accessibility
//! away. A mapped MMIO window is reachable without any syscall on the access
//! path, so clearing a TCB bit alone would be a label change, not a revocation.
//!
//! One marker per property:
//!   * `MMIO-REVOKE-CLASS` — two windows of different classes: revoking one
//!     leaves the other owned.
//!   * `MMIO-REVOKE-ALLOWLIST` — the same through the real device-class path
//!     (`request_mmio` deriving the class from the board allowlist); SKIP on a
//!     board with no allowlisted windows (x86 q35 has none).
//!   * `MMIO-REVOKE-USERBIT` — after `revoke_mmio_user`, the page is still
//!     identity-mapped for the kernel but no longer user-accessible.
//!
//! Runs in the same single-hart boot window as the other trust self-tests.

use super::syscall::{handle_syscall, Syscall};
use super::thread_cap_selftest::{insert, mk_task, remove};
use crate::memory::paging;
use crate::resource_registry as registry;
use types::CellId;

// Synthetic tid/cell above the boot-assigned range; removed before return.
const TARGET_TID: usize = 9301;
const TARGET_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 12) as u64;
/// A second cell for the allowlist path, so a stale region owned by the first
/// cannot make the revoke look selective.
const ALLOW_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 13) as u64;

const PAGE: usize = 4096;
/// Two windows owned under classes no `DEV_*` bit names — the class the platform
/// registers its ECAM window under is one of them.
const WINDOW_A: usize = 0x5000_0000;
const WINDOW_B: usize = 0x5001_0000;

/// Revoke one class and leave the other window owned, then revoke the rest.
///
/// The classes are registered explicitly: the board allowlist (the other probe)
/// carries at most one window per board, so a second class has to come from the
/// test hook to make selectivity observable.
fn probe_classes() -> bool {
    let cell = CellId(TARGET_CELL);
    if !registry::test_register_region(cell, WINDOW_A, PAGE, registry::CLASS_ECAM)
        || !registry::test_register_region(cell, WINDOW_B, PAGE, registry::CLASS_DWC2)
    {
        return false;
    }

    let revoked = registry::revoke_mmio_for(cell, registry::CLASS_ECAM);
    let selective = revoked == alloc::vec![(WINDOW_A, PAGE)]
        && registry::lookup_mmio_owner(WINDOW_A).is_none()
        && registry::owns_exact_mmio(cell, WINDOW_B, PAGE);
    let rest = registry::revoke_mmio_for(cell, registry::CLASS_DWC2);
    selective && rest == alloc::vec![(WINDOW_B, PAGE)]
}

/// The real device-class path: `request_mmio` derives the class from the board
/// allowlist, and the revoke matches on exactly that class.
///
/// `None` when this board has no allowlisted window to claim.
fn probe_allowlist() -> Option<bool> {
    let &(base, len, class) = registry::allowed_windows().first()?;
    let cell = CellId(ALLOW_CELL);
    if registry::request_mmio(cell, base, len, class).is_err() {
        return Some(false);
    }
    let revoked = registry::revoke_mmio_for(cell, class as registry::MmioClass);
    Some(revoked == alloc::vec![(base, len)] && registry::lookup_mmio_owner(base).is_none())
}

/// A user-mapped page loses user access and keeps its identity mapping.
///
/// The page comes from a grant rather than the board's MMIO window: the mapping
/// change is the same one a revoked MMIO window gets (`protect_page` with the
/// boot flags minus `USER`), and a grant page is user-mapped on every board,
/// including the QEMU targets whose allowlisted windows are not mapped at all.
fn probe_userbit() -> bool {
    let Ok(base) = handle_syscall(TARGET_TID, Syscall::GrantAlloc { size: PAGE }) else {
        return false;
    };
    if base == 0 {
        return false;
    }
    let before = paging::mapping_state(base);
    let cleared = paging::revoke_mmio_user(base, PAGE);
    let after = paging::mapping_state(base);
    let _ = handle_syscall(TARGET_TID, Syscall::GrantFree { grant_id: base });
    before == (true, true) && cleared == Ok(1) && after == (true, false)
}

/// Run the MMIO revocation self-test and leave the registry as it was found.
pub(crate) fn self_test() -> bool {
    insert(mk_task(TARGET_TID, TARGET_CELL));

    let classes = probe_classes();
    let userbit = probe_userbit();
    let allowlist = probe_allowlist();

    // Teardown: release anything a probe left owned, then drop the task.
    let _ = registry::revoke_mmio_for(CellId(TARGET_CELL), u16::MAX);
    let _ = registry::revoke_mmio_for(CellId(ALLOW_CELL), u16::MAX);
    remove(TARGET_TID);

    report("MMIO-REVOKE-CLASS", classes);
    report("MMIO-REVOKE-USERBIT", userbit);
    match allowlist {
        Some(ok) => report("MMIO-REVOKE-ALLOWLIST", ok),
        None => log::info!("MMIO-REVOKE-ALLOWLIST: SKIP (no allowlisted windows on this board)"),
    }

    classes && userbit && allowlist.unwrap_or(true)
}

fn report(marker: &str, ok: bool) {
    if ok {
        log::info!("{marker}: PASS");
    } else {
        log::error!("{marker}: FAIL");
    }
}
