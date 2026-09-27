//! Phase-01 containment witness for domain-backed zero-copy grants.
//!
//! The private-root grant lifecycle is not qualified yet. `GrantSlice` decides
//! only *whether* the caller may slice; it never copies `shared_to`'s rights into
//! the receiver PTE (`authorize_grant_slice_locked` maps RW unconditionally), and
//! a receiver mapping created for a domain caller is only reachable through that
//! caller's own root when it must be revoked. `GrantAlloc`/`GrantRegister` also
//! publish their frames USER-mapped in the SAS root when the owner is a domain,
//! so a domain owner's backing is writable from the shared address space.
//!
//! Until the public lifecycle is proven, every entry point that could publish
//! such a record must refuse *before* a table row, PTE, pin or frame exists — and
//! the refusal must be the ABI-safe sentinel, because the `ostd` wrapper reads
//! any nonzero `GrantAlloc`/`GrantRegister` return as a grant id
//! (`libs/ostd/src/syscall.rs` `sys_grant_alloc`).
//!
//! This fixture drives the *production* `handle_syscall` path with a real
//! `TaskAddressSpace::Domain` task, so the refusal is asserted where the ABI sees
//! it, not inside a private helper. It also proves the Tier-1 SAS→SAS path is
//! untouched by the gate.
//!
//! Emits one marker per property, plus a single terminal:
//!   `S22-RV64-GRANT-GATE: PASS` — every property held.

use super::syscall::{handle_syscall, Syscall, SyscallError};
use super::tcb::TaskAddressSpace;
use super::thread_cap_selftest::{insert, mk_task, remove};
use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder, MappingKind};
use crate::memory::paging::Flags;
use alloc::sync::Arc;
use types::GrantPerm;

// Synthetic tids above the range the boot sequence assigns, removed before
// return. Cells stay clear of the ids the sibling boot self-tests use.
const DOMAIN_TID: usize = 9501;
const SAS_OWNER_TID: usize = 9502;
const SAS_GRANTEE_TID: usize = 9503;
const DOMAIN_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 14) as u64;
const SAS_OWNER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 15) as u64;
const SAS_GRANTEE_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 16) as u64;

const PAGE_SIZE: usize = 4096;
const GRANT_SIZE: usize = PAGE_SIZE;
/// A private-root virtual address for the fixture's own page. Kept out of the
/// SAS identity window so nothing else in the boot can alias it.
const DOMAIN_VA: usize = 0x0400_0000;

/// Frames currently available to the allocator.
fn free_frames() -> usize {
    crate::memory::frame::FRAME_ALLOCATOR
        .lock()
        .as_ref()
        .map(|allocator| allocator.free_frames())
        .unwrap_or(0)
}

/// A live private root for the fixture's domain task. The page is never faulted
/// in by hardware; it exists so the task is a real domain identity, not a task
/// with a hand-set flag.
fn build_domain_space() -> Option<Arc<AddressSpace>> {
    let mut builder = AddressSpaceBuilder::new();
    let bits = Flags::READ | Flags::WRITE;
    builder
        .map_user_page(DOMAIN_VA, MappingKind::Private, Flags::from_bits(bits))
        .ok()?;
    builder.build().ok()
}

/// Properties the gate must hold, in the order they are reported.
struct GateProbe {
    alloc: bool,
    register: bool,
    share: bool,
    slice: bool,
    sas: bool,
    frames: bool,
}

impl GateProbe {
    fn all(&self) -> bool {
        self.alloc && self.register && self.share && self.slice && self.sas && self.frames
    }
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

fn grant_slice(tid: usize, grant_id: usize) -> Result<usize, SyscallError> {
    handle_syscall(tid, Syscall::GrantSlice { grant_id, size_out_ptr: 0 })
}

/// Drive every grant entry point and return what the ABI observed.
///
/// The probe cleans up after itself in both worlds: on a gated kernel the alloc
/// and register calls return `Ok(0)` and nothing is published, while against an
/// ungated kernel the `Some(..)` captures below are freed here so a red run does
/// not leak frames into the following fixtures.
fn probe() -> GateProbe {
    let baseline = free_frames();

    // 1. A domain owner must be refused before a frame is allocated. `Ok(0)` is
    //    the only alloc-safe denial: any nonzero return would be decoded as a
    //    grant id by the cell-side wrapper.
    let domain_grant = match handle_syscall(DOMAIN_TID, Syscall::GrantAlloc { size: GRANT_SIZE }) {
        Ok(0) => None,
        Ok(base) => Some(base),
        Err(_) => None,
    };
    let alloc = domain_grant.is_none();

    // 2. Same for the persistent registered buffer, whose ungated path also maps
    //    the frames into the owner's domain root.
    let domain_reg = match handle_syscall(DOMAIN_TID, Syscall::GrantRegister { size: GRANT_SIZE }) {
        Ok(0) => None,
        Ok(id) => Some(id),
        Err(_) => None,
    };
    let register = domain_reg.is_none();

    // 3. A SAS owner must not be able to publish a domain receiver.
    let sas_grant = grant_alloc(SAS_OWNER_TID);
    let share = sas_grant.is_some_and(|grant_id| {
        !grant_share(SAS_OWNER_TID, grant_id, DOMAIN_TID, GrantPerm::ReadWrite)
    });

    // 4. …and without that share the domain task must not be able to resolve a
    //    raw mapping for it, at any rights.
    let slice = sas_grant.is_some_and(|grant_id| {
        matches!(grant_slice(DOMAIN_TID, grant_id), Ok(usize::MAX))
    });

    // 5. Tier-1 regression: a SAS→SAS grant still shares, slices to the
    //    identity-mapped base and frees (the copied-IPC/VFS path).
    let sas = match grant_alloc(SAS_OWNER_TID) {
        Some(grant_id) => {
            let shared = grant_share(SAS_OWNER_TID, grant_id, SAS_GRANTEE_TID, GrantPerm::ReadOnly);
            let sliced = matches!(grant_slice(SAS_GRANTEE_TID, grant_id), Ok(base) if base == grant_id);
            let freed = handle_syscall(SAS_OWNER_TID, Syscall::GrantFree { grant_id }).is_ok();
            shared && sliced && freed
        }
        None => false,
    };

    // Teardown: release whatever an ungated kernel published, then prove the
    // denies themselves did not consume or leak a frame.
    if let Some(grant_id) = sas_grant {
        let _ = handle_syscall(SAS_OWNER_TID, Syscall::GrantFree { grant_id });
    }
    if let Some(grant_id) = domain_grant {
        let _ = handle_syscall(DOMAIN_TID, Syscall::GrantFree { grant_id });
    }
    if let Some(reg_id) = domain_reg {
        let _ = handle_syscall(DOMAIN_TID, Syscall::GrantUnregister { reg_id });
    }
    let frames = free_frames() == baseline;

    GateProbe {
        alloc,
        register,
        share,
        slice,
        sas,
        frames,
    }
}

fn report(marker: &str, ok: bool) {
    if ok {
        log::info!("{marker}: PASS");
    } else {
        log::error!("{marker}: FAIL");
    }
}

/// Run the containment self-test and leave the scheduler and grant tables as
/// they were found.
pub(crate) fn run_primary() {
    let Some(space) = build_domain_space() else {
        log::error!("S22-RV64-GRANT-GATE: FAIL fixture-setup");
        return;
    };

    let mut domain_task = mk_task(DOMAIN_TID, DOMAIN_CELL);
    domain_task.address_space = TaskAddressSpace::Domain(space);
    insert(domain_task);
    insert(mk_task(SAS_OWNER_TID, SAS_OWNER_CELL));
    insert(mk_task(SAS_GRANTEE_TID, SAS_GRANTEE_CELL));

    let probe = probe();

    remove(DOMAIN_TID);
    remove(SAS_OWNER_TID);
    remove(SAS_GRANTEE_TID);

    report("S22-RV64-GRANT-GATE-ALLOC", probe.alloc);
    report("S22-RV64-GRANT-GATE-REGISTER", probe.register);
    report("S22-RV64-GRANT-GATE-SHARE", probe.share);
    report("S22-RV64-GRANT-GATE-SLICE", probe.slice);
    report("S22-RV64-GRANT-GATE-SAS", probe.sas);
    report("S22-RV64-GRANT-GATE-FRAMES", probe.frames);

    if probe.all() {
        log::info!("S22-RV64-GRANT-GATE: PASS");
    } else {
        log::error!("S22-RV64-GRANT-GATE: FAIL");
    }
}
