// SPDX-License-Identifier: MPL-2.0
//! Boot fixture `TAG-POOL-OWNER-LIMIT` (census §8.5): one cell incarnation
//! holds at most its share of the VM tag pool, while the kernel and another
//! cell still get tags.
//!
//! Cell A creates small VMs until it holds `arch::vm::vm_tags_per_owner()`
//! VM tags (AArch64 VMIDs, taken by `create_vm`; x86 SVM ASIDs, taken by each
//! `create_vcpu`). Its next tagged VM must be refused for a reason other than
//! its guest budget, and a probe of the pool must name
//! [`TagRefusal::OwnerLimit`]. The kernel and cell C must then each still get
//! a tagged VM. Both cells get a guest budget far above what these VMs
//! charge, so the budget never refuses first. Every VM is reaped before the
//! verdict.
//!
//! Shared like `VM-GUEST-BUDGET`: each arch includes it from its `vm` module
//! (`#[path]`) under its own gate, AArch64 non-release builds (EL2 hosts; the
//! `virt-vhe` lane requires it) and x86 `test-hooks` builds with SVM root
//! operation. It logs `SKIP` on a host that runs no guests, and on VMX, whose
//! VMs carry no tag.

use crate::hypervisor::registry::{self, VmCreateError};
use crate::memory::cell_quota::{Charge, Limits, QuotaReservation};
use crate::memory::tag_pool::TagRefusal;

/// Guest RAM of each fixture VM, in pages.
const VM_PAGES: usize = 1;
/// Each fixture cell's guest limit (1 MiB). A one-page VM with its vCPU
/// charges a few dozen pages, so no budget refusal comes before the tag's.
const GUEST_LIMIT: usize = 1024 * 1024;
/// Where a fixture vCPU would enter; none runs.
const ENTRY: u64 = 0;

/// One tagged VM for `owner`: the VM and, where the tag comes with the vCPU,
/// one vCPU.
fn tagged_vm(owner: Charge) -> Result<(), VmCreateError> {
    let vm_id = registry::create_vm(owner, VM_PAGES)?;
    if crate::arch::vm::VM_TAG_PER_VCPU {
        registry::create_vcpu(owner, vm_id, ENTRY)?;
    }
    Ok(())
}

/// A fresh incarnation with [`GUEST_LIMIT`]; dropping the reservation
/// deregisters it.
fn fixture_cell() -> Option<(QuotaReservation, Charge)> {
    let generation = crate::task::tcb::next_cell_generation();
    let limits = Limits {
        memory: 0,
        guest: GUEST_LIMIT,
    };
    let quota = QuotaReservation::reserve_next(generation, limits).ok()?;
    let owner = Charge::new(quota.cell_id(), generation);
    Some((quota, owner))
}

/// Run the fixture once, at boot before the first cell spawn, and log its
/// verdict.
pub(crate) fn run() {
    if !crate::arch::vm::guest_fixture_host() {
        log::info!("[selftest] TAG-POOL-OWNER-LIMIT: SKIP (this host runs no guests)");
        return;
    }
    let Some(per_owner) = crate::arch::vm::vm_tags_per_owner() else {
        log::info!("[selftest] TAG-POOL-OWNER-LIMIT: SKIP (VMs on this host carry no tag)");
        return;
    };
    let (Some((quota_a, a)), Some((quota_c, c))) = (fixture_cell(), fixture_cell()) else {
        log::error!("[selftest] TAG-POOL-OWNER-LIMIT: FAIL no free CellId for the fixture cells");
        return;
    };

    let held = (0..per_owner).take_while(|_| tagged_vm(a).is_ok()).count();
    let over = tagged_vm(a);
    let probe = crate::arch::vm::vm_tag_probe(a);
    let kernel = tagged_vm(Charge::KERNEL);
    let other = tagged_vm(c);
    for owner in [a, c, Charge::KERNEL] {
        registry::reap_vms_for(owner);
    }
    // Unpublished: dropping the reservations deregisters A and C.
    drop((quota_a, quota_c));

    let failure = if held != per_owner as usize {
        Some("cell A could not hold its share of tags")
    } else if !matches!(over, Err(VmCreateError::Refused(_))) {
        Some("cell A's VM past its share was not refused for its tag")
    } else if probe != Err(TagRefusal::OwnerLimit) {
        Some("the pool did not refuse cell A for its owner limit")
    } else if kernel.is_err() {
        Some("the kernel was refused a tag")
    } else if other.is_err() {
        Some("cell C was refused a tag")
    } else {
        None
    };
    match failure {
        None => log::info!(
            "[selftest] TAG-POOL-OWNER-LIMIT: PASS cell held {} VM tags and was refused the next (owner limit); kernel and another cell still got tags",
            held
        ),
        Some(reason) => log::error!(
            "[selftest] TAG-POOL-OWNER-LIMIT: FAIL {} (held={} per_owner={} over={:?} probe={:?} kernel={:?} other={:?})",
            reason,
            held,
            per_owner,
            over,
            probe,
            kernel,
            other
        ),
    }
}
