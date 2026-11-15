// SPDX-License-Identifier: MPL-2.0
//! Boot fixture `VM-GUEST-BUDGET` (census §8.6(b)): a VM whose guest RAM
//! exceeds its owner's guest budget is refused before any effect, and a VM
//! within it holds exactly its creation charge until it is reaped.
//!
//! Cell B is registered with a guest limit of [`LIMIT_PAGES`]. Creating a VM
//! of [`OVER_PAGES`] must return [`VmCreateError::Budget`] and leave the frame
//! allocator's counts, its largest free run, the free VM tags (AArch64 VMIDs,
//! x86 SVM ASIDs) and B's guest charge as they were: no root, no carve, no
//! map, no tag. A VM of [`FIT_PAGES`] must then be created with B charged
//! exactly `arch::vm::guest_bytes(FIT_PAGES)` (guest RAM, tables, fixed
//! pages), and reaping B's VMs must return the charge to 0.
//!
//! Shared by the arches whose hosts run guests; each includes it from its
//! `vm` module (`#[path]`) under its own gate: AArch64 non-release builds (EL2
//! hosts; the `virt-vhe` lane requires it) and x86 `test-hooks` builds (SVM or
//! VMX root operation). It logs `SKIP` on a host that runs no guests.

use crate::hypervisor::registry::{self, VmCreateError};
use crate::memory::cell_quota::{self, Charge, Limits, QuotaReservation};
use crate::memory::frame::FrameStats;
use crate::memory::paging::PAGE_SIZE;
use types::CellId;

/// Cell B's guest limit, in pages.
const LIMIT_PAGES: usize = 64;
/// A VM whose guest RAM alone is past the limit.
const OVER_PAGES: usize = LIMIT_PAGES + 1;
/// A VM whose whole creation charge fits the limit on every arch.
const FIT_PAGES: usize = 32;

/// Everything a refused `create_vm` must leave as it was.
#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    frames: Option<FrameStats>,
    largest_free_run: usize,
    free_tags: usize,
    charged: usize,
}

fn snapshot(cell: CellId) -> Snapshot {
    Snapshot {
        frames: crate::memory::frame::stats(),
        largest_free_run: crate::memory::frame::largest_free_run(),
        free_tags: crate::arch::vm::free_vm_tags(),
        charged: cell_quota::guest_in_use(cell),
    }
}

/// Run the fixture once, at boot before the first cell spawn, and log its
/// verdict.
pub(crate) fn run() {
    if !crate::arch::vm::guest_fixture_host() {
        log::info!("[selftest] VM-GUEST-BUDGET: SKIP (this host runs no guests)");
        return;
    }
    let generation = crate::task::tcb::next_cell_generation();
    let limits = Limits {
        memory: 0,
        guest: LIMIT_PAGES * PAGE_SIZE,
    };
    let Ok(quota) = QuotaReservation::reserve_next(generation, limits) else {
        log::error!("[selftest] VM-GUEST-BUDGET: FAIL no free CellId for cell B");
        return;
    };
    let cell = quota.cell_id();
    let owner = Charge::new(cell, generation);

    let before = snapshot(cell);
    let refused = registry::create_vm(owner, OVER_PAGES);
    let after = snapshot(cell);

    let expected = crate::arch::vm::guest_bytes(FIT_PAGES);
    let created = registry::create_vm(owner, FIT_PAGES);
    let charged = cell_quota::guest_in_use(cell);
    registry::reap_vms_for(owner);
    let reaped = cell_quota::guest_in_use(cell);
    // Unpublished: dropping the reservation deregisters B.
    drop(quota);

    let failure = if refused != Err(VmCreateError::Budget) {
        Some("the over-budget VM was not refused for its budget")
    } else if after != before {
        Some("the refusal changed the allocator, the free tags or the charge")
    } else if created.is_err() {
        Some("the VM within the budget was refused")
    } else if Some(charged) != expected {
        Some("the charge is not guest RAM + tables + fixed pages")
    } else if reaped != 0 {
        Some("the reap left a guest charge")
    } else {
        None
    };
    match failure {
        None => log::info!(
            "[selftest] VM-GUEST-BUDGET: PASS refused {} pages over a {}-page budget with nothing taken; {} pages charged {} bytes; reaped to 0",
            OVER_PAGES,
            LIMIT_PAGES,
            FIT_PAGES,
            charged
        ),
        Some(reason) => log::error!(
            "[selftest] VM-GUEST-BUDGET: FAIL {} (refused={:?} before={:?} after={:?} created={:?} charged={} expected={:?} reaped={})",
            reason,
            refused,
            before,
            after,
            created,
            charged,
            expected,
            reaped
        ),
    }
}
