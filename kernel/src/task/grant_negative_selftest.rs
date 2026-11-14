//! Domain grant negatives with fault-injection controls, and the deferred
//! revoke witness (Phase 15).
//!
//! The body is architecture-neutral — it drives the production `handle_syscall`
//! entry points and the domain copy commit with real `TaskAddressSpace::Domain`
//! tasks — and is wired into the x86_64 boot fixtures, the lane that ports the
//! RV64/AArch64 grant suite.
//!
//! **Deferred revoke** (`{ARCH_TAG}-GRANT-DEFERRED`): while the tag
//! acknowledgement is withheld, `GrantFree` must fail closed — the record stays
//! `Revoking`, its frames are retained, no slice or share resolves — after the
//! local tag invalidation (INVPCID type 1, or a CR3 reload without PCID, on x86)
//! has run. Once the acknowledgement resumes, the owner's exit hands the row to
//! the deferred sweep, which completes the revoke and frees the frames.
//!
//! **Unbound owner** (`{ARCH_TAG}-GRANT-UNBOUND-OWNER`): a `GrantAlloc` whose
//! caller loses its live binding after its own root mapped the backing leaves
//! no owner mapping behind and frees the frames only after the owner's tag is
//! confirmed.
//!
//! **Negatives** (`{ARCH_TAG}-GRANT-NEG-<CASE>`): revoke while mapped, receiver
//! death, stale grant id, cross-domain forgery, and fault during copy with its
//! permission twin (a leaf downgraded during the copy). Each runs as built and
//! must report `PASS`; then its CONTROL disables exactly the one check the case
//! guards ([`GuardedCheck`]) and the same assertions must fail for exactly the
//! reason that check exists for: `{ARCH_TAG}-GRANT-CONTROL-<CASE>: CAUGHT
//! (<reason>)`. A control that fails for any other reason (setup, teardown, a
//! different property) did not exercise its check and reports `FAIL`; one that
//! still passes reports `MISSED`. Either fails the terminal `{ARCH_TAG}-GRANT-NEG`.
//!
//! The withheld window is bracketed by `[grant-neg] withhold-ack window OPEN` /
//! `CLOSED`: the tag invalidations it reports unconfirmed are its purpose, and
//! the lane excludes only the lines between those two markers from its
//! unconfirmed-invalidation rule.
//!
//! It also reports the longest lifecycle-gate holds the boot's grant fixtures
//! caused (`[grant] gate-held max publish=… drain=…`, risk R4), since the gate
//! holder runs with interrupts masked while it waits for remote
//! acknowledgements.

use super::domain_grant::{gate_hold, DomainGrantState, ARCH_TAG};
use super::fixture_controls::{GuardedCheck, DISABLED};
use super::syscall::{domain_row_for_test, handle_syscall, Syscall, SyscallError};
use super::tcb::TaskAddressSpace;
use super::thread_cap_selftest::{insert, mk_task, remove};
use super::user_copy::{stage_domain_for_test, CopyError};
use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder, MappingKind};
use crate::memory::deferred_release;
use crate::memory::frame::phys_to_virt;
use crate::memory::paging::{Flags, PAGE_SIZE};
use crate::memory::tlb_shootdown;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use types::GrantPerm;

// Synthetic tids and cells clear of the sibling boot fixtures, removed before
// return.
const OWNER_TID: usize = 9801;
const RECEIVER_TID: usize = 9802;
const THIRD_TID: usize = 9803;
const SUCCESSOR_TID: usize = 9804;
const OWNER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 30) as u64;
const RECEIVER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 31) as u64;
const THIRD_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 32) as u64;
const SUCCESSOR_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 33) as u64;

/// Private-root VAs for each task's own page, above every lane machine's RAM
/// (grant pages are published at VA = PA) and below `hal::paging::USER_LIMIT`.
const OWNER_VA: usize = 0x30_1000_0000;
const RECEIVER_VA: usize = 0x30_1400_0000;
const THIRD_VA: usize = 0x30_1800_0000;
const SUCCESSOR_VA: usize = 0x30_1c00_0000;
/// The copy fixture's own private page.
const COPY_VA: usize = 0x30_2000_0000;

const GRANT_SIZE: usize = PAGE_SIZE;
const SENTINEL: u8 = 0xA5;
/// How long the release half waits for the reaper once the acknowledgement
/// resumes. Generous: it bounds a fixture, not a syscall.
const RELEASE_BUDGET_NS: u64 = 2_000_000_000;

/// The domain tasks every case runs against.
struct Fixture {
    owner: Arc<AddressSpace>,
    receiver: Arc<AddressSpace>,
    third: Arc<AddressSpace>,
    successor: Arc<AddressSpace>,
}

/// A case's verdict: `Err` names the first property that did not hold.
type Verdict = Result<(), &'static str>;

/// One negative: its marker, the check its CONTROL disables, the exact verdict
/// that control must produce, and its body.
struct Case {
    name: &'static str,
    check: GuardedCheck,
    /// The `Err` the body returns when `check` is the only thing missing.
    reason: &'static str,
    run: fn(&Fixture) -> Verdict,
}

const CASES: [Case; 6] = [
    Case {
        name: "REVOKE-MAPPED",
        check: GuardedCheck::RevokeDrainsReceiver,
        reason: "receiver-mapping-survived-revoke",
        run: revoke_while_mapped,
    },
    Case {
        name: "RECEIVER-DEATH",
        check: GuardedCheck::DeathDrainsReceiver,
        reason: "dead-receiver-mapping-survived",
        run: receiver_death,
    },
    Case {
        name: "STALE-ID",
        check: GuardedCheck::SliceNeedsLive,
        reason: "owner-resolved-revoking-record",
        run: stale_grant_id,
    },
    Case {
        name: "FORGERY",
        check: GuardedCheck::SliceNeedsGrantee,
        reason: "third-domain-resolved-grant",
        run: cross_domain_forgery,
    },
    Case {
        name: "COPY-FAULT",
        check: GuardedCheck::CommitNeedsLeaf,
        reason: "commit-accepted-revoked-page",
        run: fault_during_copy,
    },
    Case {
        name: "COPY-DOWNGRADE",
        check: GuardedCheck::CommitNeedsRight,
        reason: "commit-accepted-downgraded-page",
        run: downgrade_during_copy,
    },
];

/// Disables one check for the lifetime of the value (one CONTROL run).
struct Control(GuardedCheck);

impl Control {
    fn disable(check: GuardedCheck) -> Self {
        DISABLED.fetch_or(check.bit(), Ordering::AcqRel);
        Self(check)
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        DISABLED.fetch_and(!self.0.bit(), Ordering::AcqRel);
    }
}

/// Withholds every tag acknowledgement for the lifetime of the value, with the
/// lane's bracket markers around it.
struct WithheldAck;

impl WithheldAck {
    fn open() -> Self {
        log::info!("[grant-neg] withhold-ack window OPEN");
        tlb_shootdown::set_test_withhold_tag_ack(true);
        Self
    }
}

impl Drop for WithheldAck {
    fn drop(&mut self) {
        tlb_shootdown::set_test_withhold_tag_ack(false);
        log::info!("[grant-neg] withhold-ack window CLOSED");
    }
}

fn space(va: usize) -> Option<Arc<AddressSpace>> {
    let mut builder = AddressSpaceBuilder::new();
    builder
        .map_user_page(
            va,
            MappingKind::Private,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )
        .ok()?;
    builder.build().ok()
}

fn grant_alloc(tid: usize) -> Option<usize> {
    match handle_syscall(tid, Syscall::GrantAlloc { size: GRANT_SIZE }) {
        Ok(base) if base != 0 => Some(base),
        _ => None,
    }
}

fn grant_share(owner: usize, grant_id: usize, target: usize, perm: GrantPerm) -> bool {
    handle_syscall(
        owner,
        Syscall::GrantShare {
            grant_id,
            target_cell: target,
            perm: perm as usize,
        },
    )
    .is_ok()
}

/// `Some(base)` when the slice resolved, `None` for the refusal sentinel or an
/// error.
fn grant_slice(tid: usize, grant_id: usize) -> Option<usize> {
    match handle_syscall(
        tid,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: 0,
            expected_owner_tid: 0,
            flags: 0,
        },
    ) {
        Ok(base) if base != usize::MAX => Some(base),
        _ => None,
    }
}

fn grant_free(tid: usize, grant_id: usize) -> Result<usize, SyscallError> {
    handle_syscall(tid, Syscall::GrantFree { grant_id })
}

/// Does `space` hold any mapping at `va`?
fn maps(space: &AddressSpace, va: usize) -> bool {
    space
        .ledger()
        .into_iter()
        .any(|entry| entry.virtual_address == va)
}

/// Allocate one grant and publish it to `receiver_tid` with `perm`, resolved.
fn shared_grant(receiver_tid: usize, receiver: &AddressSpace, perm: GrantPerm) -> Option<usize> {
    let base = grant_alloc(OWNER_TID)?;
    if grant_share(OWNER_TID, base, receiver_tid, perm)
        && grant_slice(receiver_tid, base) == Some(base)
        && maps(receiver, base)
    {
        Some(base)
    } else {
        let _ = grant_free(OWNER_TID, base);
        None
    }
}

/// Spin until `done` holds or [`RELEASE_BUDGET_NS`] passes, reaping the
/// deferred-release queue on every pass.
fn within_release_budget(mut done: impl FnMut() -> bool) -> bool {
    let Some(start) = crate::arch::clock::monotonic_ns() else {
        return false;
    };
    loop {
        deferred_release::reap_deferred_releases();
        if done() {
            return true;
        }
        match crate::arch::clock::monotonic_ns() {
            Some(now) if now.saturating_sub(start) < RELEASE_BUDGET_NS => core::hint::spin_loop(),
            _ => return false,
        }
    }
}

/// What the deferred revoke observed, in the order the marker reports it.
#[derive(Default)]
struct Deferred {
    refused: bool,
    tag_flushed: bool,
    retained: bool,
    revoking: bool,
    receiver_unmapped: bool,
    swept: bool,
    released: bool,
}

impl Deferred {
    fn all(&self) -> bool {
        self.refused
            && self.tag_flushed
            && self.retained
            && self.revoking
            && self.receiver_unmapped
            && self.swept
            && self.released
    }
}

fn deferred_revoke(fx: &Fixture) -> Deferred {
    let mut seen = Deferred::default();
    let Some(base) = shared_grant(RECEIVER_TID, &fx.receiver, GrantPerm::ReadWrite) else {
        return seen;
    };
    let receiver_tag = fx.receiver.asid();
    let (_, flushes_before) = crate::hal::domain::switch_counters();
    {
        let _window = WithheldAck::open();
        tlb_shootdown::begin_test_flush_observation();
        seen.refused = grant_free(OWNER_TID, base) == Err(SyscallError::PermissionDenied);
        tlb_shootdown::finish_test_flush_observation();
        // The local half ran — the receiver tag was invalidated through the HAL
        // — and only the acknowledgement is missing.
        seen.tag_flushed = tlb_shootdown::test_tag_flush_observed(receiver_tag)
            && crate::hal::domain::switch_counters().1 > flushes_before;
        // Retained: the row stays, owned, with its record `Revoking` (the
        // frames leave only with the row), and the receiver root's detached
        // tables wait in the deferred-release queue under its tag.
        seen.retained = domain_row_for_test(base) == Some((OWNER_TID, DomainGrantState::Revoking))
            && deferred_release::deferred_release_frames_for(receiver_tag) > 0;
        seen.revoking = grant_slice(OWNER_TID, base).is_none()
            && grant_slice(RECEIVER_TID, base).is_none()
            && !grant_share(OWNER_TID, base, RECEIVER_TID, GrantPerm::ReadWrite);
        seen.receiver_unmapped = !maps(&fx.receiver, base);
        // The owner exits while the acknowledgement is still missing: its row
        // loses its owner and waits for the housekeeping sweep, frames retained.
        super::syscall::reclaim_owned_grants(OWNER_TID);
        seen.retained &= domain_row_for_test(base) == Some((0, DomainGrantState::Revoking));
    }
    // Acknowledgement resumes: the housekeeping sweep completes the revoke, and
    // the row (with the frames its record held) is gone. The fixture drives the
    // sweep itself; a scheduler pass on another CPU may win the row first.
    seen.swept = within_release_budget(|| {
        super::syscall::sweep_deferred_domain_grants();
        domain_row_for_test(base).is_none()
    });
    // The receiver root's detached tables leave the queue at the reaper's next
    // pass over their now-confirmed tag, not with the row.
    seen.released =
        within_release_budget(|| deferred_release::deferred_release_frames_for(receiver_tag) == 0)
            && !deferred_release::tag_invalidation_unconfirmed(receiver_tag)
            && crate::memory::address_space::quarantined_frame_count() == 0;
    seen
}

/// `GrantAlloc` whose caller loses its live binding after its own root mapped
/// the new backing (Phase 16 9b). The pages must leave the owner's root before
/// a frame is freed; before the fix the frames were freed with the owner's
/// mapping still in place. The frames come back once the owner's tag is
/// confirmed, so the allocator's deficit is only the tables the root still owns.
fn unbound_owner(fx: &Fixture) -> bool {
    use crate::task::tcb::TaskState;
    let grant_leaves = |space: &AddressSpace| {
        space
            .ledger()
            .into_iter()
            .filter(|entry| entry.kind == MappingKind::Grant)
            .count()
    };
    let free_frames = || crate::memory::frame::stats().map_or(0, |frames| frames.free_frames);
    let set_state = |state: TaskState| {
        super::SCHEDULER.lock().as_mut().and_then(|sched| {
            sched
                .tasks
                .get_mut(&OWNER_TID)
                .map(|task| core::mem::replace(&mut task.state, state))
        })
    };
    let leaves_before = grant_leaves(&fx.owner);
    let owned_before = fx.owner.owned_frame_count();
    let leaked_before = crate::memory::frame_run::PENDING.counters().leaked();
    let free_before = free_frames();
    let Some(state) = set_state(TaskState::Retiring) else {
        log::error!("{ARCH_TAG}-GRANT-UNBOUND-OWNER: FAIL setup");
        return false;
    };
    let refused = handle_syscall(OWNER_TID, Syscall::GrantAlloc { size: GRANT_SIZE })
        == Err(SyscallError::PermissionDenied);
    set_state(state);
    let unmapped = grant_leaves(&fx.owner) == leaves_before;
    let released = within_release_budget(|| {
        crate::memory::frame_run::reap();
        free_before.saturating_sub(free_frames())
            == fx.owner.owned_frame_count().saturating_sub(owned_before)
    });
    let not_leaked = crate::memory::frame_run::PENDING.counters().leaked() == leaked_before;
    let ok = refused && unmapped && released && not_leaked;
    if ok {
        log::info!("{ARCH_TAG}-GRANT-UNBOUND-OWNER: PASS");
    } else {
        log::error!(
            "{ARCH_TAG}-GRANT-UNBOUND-OWNER: FAIL refused={refused} unmapped={unmapped} \
             released={released} not_leaked={not_leaked}"
        );
    }
    ok
}

/// Drop a receiver mapping a CONTROL run left behind in a synthetic root, so
/// the next case can publish at the same address. The root never runs on any
/// CPU, so the stale PTE was never reachable; this restores bookkeeping only.
/// `unmap_grant_page` confirms the tag and releases the address itself.
fn scrub(space: &AddressSpace, va: usize) {
    let _ = space.unmap_grant_page(va);
}

/// Complete a revoke the withheld window left `Revoking`, retrying `GrantFree`
/// while the reaper confirms the tag.
fn finish_deferred_free(grant_id: usize) -> bool {
    within_release_budget(|| grant_free(OWNER_TID, grant_id).is_ok())
}

// ── Negatives ────────────────────────────────────────────────────────────────

/// Revoke while mapped: `GrantFree` of a grant a live receiver has mapped must
/// remove the receiver PTE before the frames leave the record.
fn revoke_while_mapped(fx: &Fixture) -> Verdict {
    let base = shared_grant(RECEIVER_TID, &fx.receiver, GrantPerm::ReadWrite).ok_or("setup")?;
    let freed = grant_free(OWNER_TID, base).is_ok();
    let receiver_gone = !maps(&fx.receiver, base);
    if !receiver_gone {
        scrub(&fx.receiver, base);
    }
    if !freed {
        return Err("free-refused");
    }
    if !receiver_gone {
        return Err("receiver-mapping-survived-revoke");
    }
    if grant_slice(RECEIVER_TID, base).is_some() {
        return Err("freed-id-resolved");
    }
    Ok(())
}

/// Receiver death: the dying receiver's PTE is drained, and the owner keeps a
/// live, re-shareable grant.
fn receiver_death(fx: &Fixture) -> Verdict {
    let base = shared_grant(RECEIVER_TID, &fx.receiver, GrantPerm::ReadWrite).ok_or("setup")?;
    super::syscall::reap_grants_for_task(RECEIVER_TID);
    let receiver_gone = !maps(&fx.receiver, base);
    let owner_kept = grant_slice(OWNER_TID, base) == Some(base) && maps(&fx.owner, base);
    // A re-share to a successor drains whatever the death left behind.
    let reshared = grant_share(OWNER_TID, base, SUCCESSOR_TID, GrantPerm::ReadOnly)
        && grant_slice(SUCCESSOR_TID, base) == Some(base);
    let freed = grant_free(OWNER_TID, base).is_ok();
    if !receiver_gone {
        return Err("dead-receiver-mapping-survived");
    }
    if !owner_kept {
        return Err("owner-lost-grant");
    }
    if !reshared {
        return Err("reshare-refused");
    }
    if !freed || maps(&fx.successor, base) {
        return Err("teardown");
    }
    Ok(())
}

/// Stale grant id: a record whose revoke is still pending (`Revoking`) is
/// stale for every caller — the owner's own slice, the receiver's slice and a
/// new share all refuse it.
fn stale_grant_id(fx: &Fixture) -> Verdict {
    let base = shared_grant(RECEIVER_TID, &fx.receiver, GrantPerm::ReadOnly).ok_or("setup")?;
    let (deferred, owner_refused, receiver_refused, share_refused) = {
        let _window = WithheldAck::open();
        let deferred = grant_free(OWNER_TID, base).is_err();
        (
            deferred,
            grant_slice(OWNER_TID, base).is_none(),
            grant_slice(RECEIVER_TID, base).is_none(),
            !grant_share(OWNER_TID, base, RECEIVER_TID, GrantPerm::ReadWrite),
        )
    };
    let freed = finish_deferred_free(base);
    if !deferred {
        return Err("free-not-deferred");
    }
    if !owner_refused {
        return Err("owner-resolved-revoking-record");
    }
    if !receiver_refused {
        return Err("receiver-resolved-revoking-record");
    }
    if !share_refused {
        return Err("revoking-record-reshared");
    }
    if !freed || maps(&fx.receiver, base) {
        return Err("deferred-free-incomplete");
    }
    Ok(())
}

/// Cross-domain forgery: a third domain that the grant was never shared to
/// cannot resolve it, and gains no mapping.
fn cross_domain_forgery(fx: &Fixture) -> Verdict {
    let base = shared_grant(RECEIVER_TID, &fx.receiver, GrantPerm::ReadOnly).ok_or("setup")?;
    let refused = grant_slice(THIRD_TID, base).is_none();
    let unmapped = !maps(&fx.third, base);
    if !unmapped {
        // The record never named the third domain, so its revoke cannot drain
        // this PTE: drop it before the free returns the frames it maps.
        scrub(&fx.third, base);
    }
    let freed = grant_free(OWNER_TID, base).is_ok();
    if !refused || !unmapped {
        return Err("third-domain-resolved-grant");
    }
    if !freed || maps(&fx.third, base) || maps(&fx.receiver, base) {
        return Err("teardown");
    }
    Ok(())
}

/// Fault during copy: a page revoked between the probe and the commit of a
/// domain copy fails the commit with the recoverable error, and no byte moves.
fn fault_during_copy(_fx: &Fixture) -> Verdict {
    let space = space(COPY_VA).ok_or("setup")?;
    let staged = stage_domain_for_test(&space, COPY_VA, 64, false).map_err(|_| "stage")?;
    space
        .force_unmap_without_drain_for_test(COPY_VA)
        .map_err(|_| "injection")?;
    let mut dst = [SENTINEL; 64];
    let result = staged.commit(dst.as_mut_ptr());
    if result != Err(CopyError::InvalidAddress) {
        return Err("commit-accepted-revoked-page");
    }
    if dst.iter().any(|&byte| byte != SENTINEL) {
        return Err("bytes-moved");
    }
    Ok(())
}

/// The first 64 bytes of the frame `space` maps at `va`, read through the
/// kernel alias.
fn frame_head(space: &AddressSpace, va: usize) -> Option<[u8; 64]> {
    let entry = space
        .ledger()
        .into_iter()
        .find(|entry| entry.virtual_address == va)?;
    let mut head = [0u8; 64];
    // SAFETY: the ledger names a frame this live address space owns, and
    // `phys_to_virt` is the kernel's alias of it.
    unsafe {
        core::ptr::copy_nonoverlapping(
            phys_to_virt(entry.physical_address) as *const u8,
            head.as_mut_ptr(),
            head.len(),
        );
    }
    Some(head)
}

/// Protocol-violation injection for [`downgrade_during_copy`]: rewrite the
/// leaf `space` maps at `va` to user read-only, keeping its frame and leaving
/// the ledger as it was, without the copy-reader drain every real permission
/// change waits for. The synthetic root never runs on any CPU; the local flush
/// keeps the TLB honest anyway.
fn force_read_only(space: &AddressSpace, va: usize) -> Result<(), ()> {
    use hal::PageTableTrait;
    let root = space.root_ppn() << 12;
    // SAFETY: `root` is the live root of a synthetic address space only this
    // fixture uses, and `phys_to_virt` is the kernel's alias of it.
    let table = unsafe { &mut *(phys_to_virt(root) as *mut hal::PageTable) };
    table
        .protect(
            va,
            Flags::from_bits(
                Flags::VALID | Flags::READ | Flags::USER | Flags::ACCESSED | Flags::NON_GLOBAL,
            ),
        )
        .map_err(drop)?;
    crate::hal::paging::flush_tlb_page(va);
    Ok(())
}

/// Fault during copy, permission half: a page whose write right is removed
/// between the probe and the commit of a domain copy to user memory fails the
/// commit with the recoverable error, and no byte reaches the page.
fn downgrade_during_copy(_fx: &Fixture) -> Verdict {
    let space = space(COPY_VA).ok_or("setup")?;
    let before = frame_head(&space, COPY_VA).ok_or("setup")?;
    let staged = stage_domain_for_test(&space, COPY_VA, 64, true).map_err(|_| "stage")?;
    force_read_only(&space, COPY_VA).map_err(|_| "injection")?;
    let src = [SENTINEL; 64];
    let result = staged.commit(src.as_ptr().cast_mut());
    if result != Err(CopyError::InvalidAddress) {
        return Err("commit-accepted-downgraded-page");
    }
    if frame_head(&space, COPY_VA) != Some(before) {
        return Err("bytes-moved");
    }
    Ok(())
}

/// Run one case as built and as its CONTROL; `true` when both behaved.
fn run_case(fx: &Fixture, case: &Case) -> bool {
    let built = (case.run)(fx);
    match built {
        Ok(()) => log::info!("{ARCH_TAG}-GRANT-NEG-{}: PASS", case.name),
        Err(why) => log::error!("{ARCH_TAG}-GRANT-NEG-{}: FAIL {why}", case.name),
    }
    let control = {
        let _disabled = Control::disable(case.check);
        (case.run)(fx)
    };
    // Only the case's own reason proves the disabled check was exercised: any
    // other failure (setup, teardown, another property) happened elsewhere.
    let caught = control == Err(case.reason);
    match control {
        Err(why) if caught => {
            log::info!(
                "{ARCH_TAG}-GRANT-CONTROL-{}: CAUGHT ({why}) with {:?} disabled",
                case.name,
                case.check
            );
        }
        Err(why) => {
            log::error!(
                "{ARCH_TAG}-GRANT-CONTROL-{}: FAIL ({why}, want {}) with {:?} disabled",
                case.name,
                case.reason,
                case.check
            );
        }
        Ok(()) => {
            log::error!(
                "{ARCH_TAG}-GRANT-CONTROL-{}: MISSED ({:?} disabled, the case still passed)",
                case.name,
                case.check
            );
        }
    }
    built.is_ok() && caught
}

/// Run the deferred revoke and every negative with its CONTROL, leave the
/// scheduler and grant tables as they were found, and report the longest gate
/// hold of the boot's grant fixtures.
pub(crate) fn run_primary() {
    let (Some(owner), Some(receiver), Some(third), Some(successor)) = (
        space(OWNER_VA),
        space(RECEIVER_VA),
        space(THIRD_VA),
        space(SUCCESSOR_VA),
    ) else {
        log::error!("{ARCH_TAG}-GRANT-NEG: FAIL fixture-setup");
        return;
    };
    for (tid, cell, root) in [
        (OWNER_TID, OWNER_CELL, &owner),
        (RECEIVER_TID, RECEIVER_CELL, &receiver),
        (THIRD_TID, THIRD_CELL, &third),
        (SUCCESSOR_TID, SUCCESSOR_CELL, &successor),
    ] {
        let mut task = mk_task(tid, cell);
        task.address_space = TaskAddressSpace::Domain(Arc::clone(root));
        insert(task);
    }
    let fx = Fixture {
        owner,
        receiver,
        third,
        successor,
    };

    let deferred = deferred_revoke(&fx);
    if deferred.all() {
        log::info!("{ARCH_TAG}-GRANT-DEFERRED: PASS");
    } else {
        log::error!(
            "{ARCH_TAG}-GRANT-DEFERRED: FAIL refused={} tag_flushed={} retained={} revoking={} \
             receiver_unmapped={} swept={} released={}",
            deferred.refused,
            deferred.tag_flushed,
            deferred.retained,
            deferred.revoking,
            deferred.receiver_unmapped,
            deferred.swept,
            deferred.released
        );
    }
    let mut ok = deferred.all();
    for case in &CASES {
        ok &= run_case(&fx, case);
    }
    ok &= unbound_owner(&fx);

    super::syscall::reclaim_owned_grants(OWNER_TID);
    for tid in [OWNER_TID, RECEIVER_TID, THIRD_TID, SUCCESSOR_TID] {
        remove(tid);
    }
    if ok {
        log::info!("{ARCH_TAG}-GRANT-NEG: PASS");
    } else {
        log::error!("{ARCH_TAG}-GRANT-NEG: FAIL");
    }
    log::info!(
        "[grant] gate-held max publish={}us drain={}us cpus={}",
        gate_hold::PUBLISH_MAX_NS.load(Ordering::Acquire) / 1_000,
        gate_hold::DRAIN_MAX_NS.load(Ordering::Acquire) / 1_000,
        crate::sched::smp::online_hart_count()
    );
}
