//! IPC System Calls (Inspired by Tock OS)
//!
//! This module defines the interface between "Cells/Silos" and the Kernel.
//! See [docs/architecture/03-driver-strategy.md] for the full rationale.

use super::tcb::TaskState;
use crate::sync::Spinlock;
use abi::syscall::ViSpawnArgs;
use alloc::string::String;
/// Only the private-root lifecycle helpers below name `Arc`; every other
/// target/feature combination has nothing to import.
#[cfg(cellos_domain_grants)]
use alloc::sync::Arc;
use alloc::vec::Vec;
// use log::info;
use super::copy_glue::TaskCopyView;
use crate::memory::region::{DomainRecord, Receiver, Region, RegionOwner, RegionTable};
use types::*;

mod ipc;
#[allow(unused_imports)]
pub(crate) use ipc::*;
#[allow(unused_imports)]
pub(in crate::task) use ipc::*;
#[allow(unused_imports)]
use ipc::*;
mod grant;
#[allow(unused_imports)]
pub(crate) use grant::*;
#[allow(unused_imports)]
pub(in crate::task) use grant::*;
#[allow(unused_imports)]
use grant::*;
mod spawn;
#[allow(unused_imports)]
pub(crate) use spawn::*;
#[allow(unused_imports)]
pub(in crate::task) use spawn::*;
#[allow(unused_imports)]
use spawn::*;
mod fs;
#[allow(unused_imports)]
pub(crate) use fs::*;
#[allow(unused_imports)]
pub(in crate::task) use fs::*;
#[allow(unused_imports)]
use fs::*;
mod dev;
mod rename_wire;
#[allow(unused_imports)]
pub(crate) use dev::*;
#[allow(unused_imports)]
pub(in crate::task) use dev::*;
#[allow(unused_imports)]
use dev::*;
mod vm;
#[allow(unused_imports)]
pub(crate) use vm::*;
#[allow(unused_imports)]
pub(in crate::task) use vm::*;
#[allow(unused_imports)]
use vm::*;
mod authority;
#[allow(unused_imports)]
pub(crate) use authority::*;
#[allow(unused_imports)]
pub(in crate::task) use authority::*;
#[allow(unused_imports)]
use authority::*;
mod decode;
#[allow(unused_imports)]
pub(crate) use decode::*;
#[allow(unused_imports)]
pub(in crate::task) use decode::*;
#[allow(unused_imports)]
use decode::*;
mod misc;
#[allow(unused_imports)]
pub(crate) use misc::*;
#[allow(unused_imports)]
pub(in crate::task) use misc::*;
#[allow(unused_imports)]
use misc::*;

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_ARMED_CALLER: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_ENTERED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_PROBED_BUSY: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_NO_EARLY_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_PROBE_TIMED_OUT: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

static DMA_PUBLICATIONS: Spinlock<[usize; MAX_IN_FLIGHT_DMA_PUBLICATIONS]> = Spinlock::ranked(
    [0; MAX_IN_FLIGHT_DMA_PUBLICATIONS],
    crate::sync::rank::DMA_PUBLICATIONS,
);

impl DmaPublicationGuard {
    fn commit(mut self) {
        if let Some(quota) = self.quota.take() {
            quota.commit();
        }
    }
}

impl Drop for DmaPublicationGuard {
    fn drop(&mut self) {
        // Roll quota back before making retirement eligible to proceed.
        drop(self.quota.take());
        let mut publications = DMA_PUBLICATIONS.lock();
        debug_assert_eq!(publications[self.slot], self.tid);
        publications[self.slot] = 0;
    }
}

// ADR-0033 CP1 §5-§6: client-memory DMA export leases (437/438/439). A child
// module so its ownership proof reads the grant tables under their own locks.
pub(crate) mod dma_export;

// ── Zero-Copy Grant Table ─────────────────────────────────────────────────────

/// The `GrantAlloc` rows and the `ShmAlloc` frames ([`RegionTable`]).
///
/// An SHM region is keyed by its frame's address and owned by the Cell
/// incarnation that allocated it. `ShmMap` accepts only a handle that is an SHM
/// row here AND belongs to the caller's live incarnation (or names the
/// caller's cell as its receiver), so a cell can neither forge a physical
/// address ("ShmMap kernel_text_phys") nor map a peer's, a dead predecessor's
/// or a grant's frame. Root retirement reclaims the dead incarnation's frames
/// ([`release_shm_for`]).
static PAGE_GRANT_TABLE: Spinlock<Option<RegionTable>> =
    Spinlock::ranked(None, crate::sync::rank::PAGE_GRANT_TABLE);

#[cfg(all(feature = "native-domains", cellos_wide))]
fn task_domain_space(
    tid: usize,
) -> Option<alloc::sync::Arc<crate::memory::address_space::AddressSpace>> {
    let guard = super::SCHEDULER.lock();
    guard.as_ref().and_then(|sched| {
        sched
            .tasks
            .get(&tid)
            .and_then(|task| match &task.address_space {
                super::tcb::TaskAddressSpace::Domain(space) => Some(alloc::sync::Arc::clone(space)),
                super::tcb::TaskAddressSpace::Sas => None,
            })
    })
}

/// The phase-03 AArch64 reopen is confined to test images, and the shapes that
/// stay denied keep the phase-01 sentinels byte-for-byte.
///
/// A production AArch64 build (no `test-hooks`) has no domain grant lifecycle,
/// so [`domain_grant_capable`] is always `false` and every domain shape takes
/// the sentinel branch ahead of the lifecycle: `Ok(0)` for allocation, `usize::MAX`
/// for a slice, `Err(PermissionDenied)` for free/unregister. Const-evaluated, so
/// a build that widened the cfg by accident fails to compile rather than booting
/// a domain grant path.
///
/// `GrantShare` has no sentinel branch — its gate lives in the lifecycle-only
/// block, so on a closed target it is reachable only for a domain task, and no
/// domain can be admitted on a closed target (`switch_ordering_qualified`).
#[cfg(all(
    feature = "native-domains",
    target_arch = "aarch64",
    not(feature = "test-hooks")
))]
const _: () = {
    assert!(!domain_grant_lifecycle_supported());
    assert!(GRANT_DENY_ALLOC == 0);
    assert!(GRANT_DENY_SLICE == usize::MAX);
    assert!(matches!(GRANT_DENY_ERROR, SyscallError::PermissionDenied));
};

/// A private root, on the targets that have one.
///
/// The `memory::address_space` module exists only behind `native-domains` on a
/// private-root target, so the helpers below name this alias and every disabled
/// variant stays compilable without the feature.
#[cfg(all(feature = "native-domains", cellos_wide))]
type DomainRoot = alloc::sync::Arc<crate::memory::address_space::AddressSpace>;
#[cfg(not(all(feature = "native-domains", cellos_wide)))]
type DomainRoot = ();

/// Map a domain owner's freshly allocated backing into its own root, RW+NX,
/// and hand the run back for the row's lifecycle record to own.
///
/// The frames stay supervisor-only in the SAS/global root: this path never
/// publishes USER access there, so a SAS cell cannot reach a domain owner's
/// backing even if the grant later collapses. If one page fails, every page
/// already mapped is undone and the run is released behind the owner's tag
/// ([`release_owner_mapped_backing`]); nothing is handed back.
#[cfg(cellos_domain_grants)]
fn map_domain_owner_pages(
    space: &DomainRoot,
    backing: crate::memory::frame_run::FrameRun,
) -> Result<crate::memory::frame_run::FrameRun, crate::memory::address_space::AddressSpaceError> {
    const PAGE_SIZE: usize = 4096;
    let rights = crate::memory::paging::Flags::from_bits(
        crate::memory::paging::Flags::READ | crate::memory::paging::Flags::WRITE,
    );
    let base = backing.base();
    for index in 0..backing.pages() {
        let va = base + index * PAGE_SIZE;
        match space.map_grant_page(va, va, rights) {
            Ok(()) => {
                // The owner's own hart writes this PTE and then uses the
                // pointer, so it must invalidate the page locally first.
                crate::hal::paging::flush_tlb_page(va);
            }
            Err(error) => {
                release_owner_mapped_backing(space, backing, index);
                return Err(error);
            }
        }
    }
    Ok(backing)
}

/// Take a domain owner's backing back after its first `mapped` pages were
/// published in the owner's own root, and release it.
///
/// The pages are unmapped from that root first. A remote hart may still
/// resolve the owner root's stale entries until the owner's tag is confirmed,
/// so the frames are freed only after one bounded probe confirms it; an
/// unconfirmed probe quarantines the run behind the tag (`Batch::await_tag`)
/// for the reaper, never frees it (census F8). The probe runs once: an undo
/// whose own probe failed already left its tag pending, and a second blocking
/// wait would buy nothing. The frames never lost the kernel's own alias, so
/// nothing is restored.
#[cfg(cellos_domain_grants)]
fn release_owner_mapped_backing(
    space: &DomainRoot,
    mut backing: crate::memory::frame_run::FrameRun,
    mapped: usize,
) {
    const PAGE_SIZE: usize = 4096;
    let base = backing.base();
    for undo in 0..mapped {
        let _ = space.unmap_grant_page(base + undo * PAGE_SIZE);
    }
    // The owner root's leaves are gone; the shared root never had one.
    let unpublished = backing.unpublish();
    debug_assert!(unpublished.is_ok(), "domain backing released twice");
    let mut batch = crate::memory::frame_run::Batch::new();
    batch.retire(backing);
    if mapped > 0 && crate::memory::tlb_shootdown::confirm_tag_invalidation(space.asid()).is_err() {
        batch.await_tag(space.asid());
    }
    let _ = batch.complete();
}

/// Allocate `n_pages` contiguous frames, zero them through the kernel's own
/// alias of RAM, and leave the SAS/global root untouched.
///
/// Domain owners must not have their backing published USER in the shared root
/// (the phase-01 leak). The kernel already reaches every usable frame
/// supervisor-only — the RV64/AArch64 identity map, the x86_64 HHDM — so
/// zeroing needs no remap at all; the caller then maps the owner's own private
/// root over the returned run. That alias is never removed, so the run is
/// `KernelAlias::Intact`: its release has nothing to unpublish or restore in
/// the kernel root (census §7.8).
#[cfg(cellos_domain_grants)]
fn alloc_grant_pages_supervisor_only(n_pages: usize) -> Option<crate::memory::frame_run::FrameRun> {
    use crate::memory::frame::phys_to_virt;
    use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind, RunOwner};
    const PAGE_SIZE: usize = 4096;
    let paddr = crate::memory::frame::allocate_contiguous(n_pages)?;
    // SAFETY: `phys_to_virt` names the kernel's supervisor alias of RAM (the
    // identity map on RV64/AArch64, the HHDM on x86_64), present in the root the
    // syscall runs on, and the run was just allocated to this call alone. x86
    // RAM has no identity alias, so the frame address itself would fault there.
    unsafe {
        core::ptr::write_bytes(phys_to_virt(paddr) as *mut u8, 0, n_pages * PAGE_SIZE);
    }
    Some(FrameRun::mapped(
        paddr,
        n_pages,
        RunOwner::Kernel,
        RunKind::Leaf,
        KernelAlias::Intact,
    ))
}

/// A record the lifecycle cannot drain: it names a private-root endpoint but is
/// not carried by a managed domain record. Pre-gate residue, or a target where
/// the lifecycle is not implemented at all.
#[cfg(all(feature = "native-domains", cellos_wide))]
fn residual_domain_record(
    owner: usize,
    shared_to: Option<(usize, GrantPerm)>,
    managed: bool,
) -> bool {
    if managed && domain_grant_lifecycle_supported() {
        return false;
    }
    domain_grant_task(owner) || shared_to.is_some_and(|(tid, _)| domain_grant_task(tid))
}

// ── Registered Grant Table (GrantRegister / GrantUnregister, syscalls 215/216) ──

static REG_GRANT_TABLE: Spinlock<Option<RegionTable>> =
    Spinlock::ranked(None, crate::sync::rank::REG_GRANT_TABLE);

// ── Shared allocation/deallocation helpers ────────────────────────────────────

// ── Grant Reaper ──────────────────────────────────────────────────────────────

/// Complete one owner-reclaimed domain row: revoke, then either release the
/// record's backing or leave an ownerless `Revoking` row for the housekeeping
/// pass ([`sweep_deferred_domain_grants`]).
///
/// An unacknowledged invalidation never frees a frame. The row loses its owner
/// so no caller can free it out from under the retry.
#[cfg(cellos_domain_grants)]
fn release_deferred_domain_row(key: usize, record: DomainRecord, base: usize, registered: bool) {
    if revoke_domain_record(&record, base, "task-exit") {
        // Only the revoker whose removal takes the row frees its frames: a
        // concurrent `GrantFree` or sweep may have completed the same record.
        let release = |row| release_domain_backing(&record, row, true);
        if registered {
            release_if_row_taken(
                reg_grant_table_lock(),
                key,
                |grant| carries_record(&grant.domain, &record),
                release,
            );
        } else {
            release_if_row_taken(
                grant_table_lock(),
                key,
                |grant| carries_record(&grant.domain, &record),
                release,
            );
        }
        return;
    }
    // The key names this record's row only while the row still carries it.
    if registered {
        let mut tbl = reg_grant_table_lock().lock();
        if let Some(grant) = tbl
            .as_mut()
            .and_then(|map| map.get_mut(&key))
            .filter(|grant| carries_record(&grant.domain, &record))
        {
            grant.orphan();
        }
    } else {
        let mut tbl = grant_table_lock().lock();
        if let Some(grant) = tbl
            .as_mut()
            .and_then(|map| map.get_mut(&key))
            .filter(|grant| carries_record(&grant.domain, &record))
        {
            grant.orphan();
        }
    }
    // The housekeeping pass retries the revoke until the tags confirm.
    crate::task::note_housekeeping();
}

#[cfg(not(cellos_domain_grants))]
fn release_deferred_domain_row(
    _key: usize,
    _record: DomainRecord,
    _base: usize,
    _registered: bool,
) {
}

/// Test-hooks: the owner and lifecycle state of the domain row `grant_id`, in
/// either grant table; `None` once no row carries that id.
#[cfg(all(cellos_domain_grants, feature = "test-hooks"))]
#[allow(dead_code)] // reason: only the x86_64 grant fixture (`task::grant_negative_selftest`) reads it
pub(crate) fn domain_row_for_test(
    grant_id: usize,
) -> Option<(usize, super::domain_grant::DomainGrantState)> {
    let page = {
        let tbl = grant_table_lock().lock();
        tbl.as_ref()
            .and_then(|map| map.get(&grant_id))
            .and_then(|grant| Some((grant.owner.tid, grant.domain.as_ref()?.state())))
    };
    page.or_else(|| {
        let tbl = reg_grant_table_lock().lock();
        tbl.as_ref()
            .and_then(|map| map.get(&grant_id))
            .and_then(|grant| Some((grant.owner.tid, grant.domain.as_ref()?.state())))
    })
}

/// Result of a System Call
pub type SyscallResult = core::result::Result<usize, SyscallError>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SyscallError {
    /// The other end of a stream is gone: a pipe write with no reader left.
    BrokenPipe,
    InvalidDriverId,
    InvalidCommand,
    BufferTooSmall,
    PermissionDenied,
    FileNotFound,
    TryAgain,
    Unknown,
    NotSupported,
    InvalidInput,
    OutOfMemory,
}

/// Encode the additive spawn-OOM result while preserving all other legacy errors.
fn encode_syscall_result(
    result: SyscallResult,
    error_sentinel: usize,
    supports_typed_oom: bool,
) -> usize {
    match result {
        Ok(value) => value,
        Err(SyscallError::OutOfMemory) if supports_typed_oom => {
            error_sentinel - abi::syscall::result_sentinel::OUT_OF_MEMORY
        }
        // A broken pipe is a distinct, actionable condition for a stream write, so
        // it gets its own sentinel rather than collapsing into the generic error.
        Err(SyscallError::BrokenPipe) => {
            error_sentinel - abi::syscall::result_sentinel::BROKEN_PIPE
        }
        Err(_) => error_sentinel - abi::syscall::result_sentinel::GENERIC,
    }
}

/// Syscalls whose callers decode the typed out-of-memory sentinel
/// (`error_sentinel - 1`); every other syscall keeps the single generic error.
fn supports_typed_oom(syscall: &Syscall) -> bool {
    syscall_to_vi(syscall).is_some_and(abi::syscall::ViSyscall::typed_oom)
}

/// Tid of trusted init, published once by `loader::governed_spawn::spawn_init`
/// right after init's task is committed; zero before. It keys the
/// `MACHINE_PIN` registration restriction, so that restriction never trusts a
/// spawner-chosen task name. Tids are never reused, so a dead init's tid
/// cannot be taken over.
static TRUSTED_INIT_TID: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

impl Drop for StagedSpawnArgvCleanup {
    fn drop(&mut self) {
        crate::cell::state_stash::discard_spawn_argv(self.caller_id);
    }
}

/// The 16550 mechanism exists only where a 16550 exists.
///
/// These shims keep the serial syscalls compilable on every architecture while
/// making them inert off x86: a machine with no port-I/O mechanism cannot offer a
/// port, so the handlers fail closed instead of reaching for hardware that is not
/// there.
mod serial_port {
    #[cfg(target_arch = "x86_64")]
    pub(super) fn present(index: u32) -> bool {
        crate::hal::uart_16550::port_base_at(index as usize).is_some()
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn present(_index: u32) -> bool {
        false
    }

    /// The declared port at `index` as `(base, irq)`, or `None` when the board
    /// profile does not declare it. Off x86 there is no port profile to consult.
    #[cfg(target_arch = "x86_64")]
    pub(super) fn declared(index: u32) -> Option<(u16, u8)> {
        crate::arch::board::soc()
            .serial_port(index as usize)
            .map(|port| (port.base, port.irq))
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn declared(_index: u32) -> Option<(u16, u8)> {
        None
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn write_byte(index: u32, byte: u8) -> bool {
        crate::hal::uart_16550::write_to(index as usize, byte)
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn write_byte(_index: u32, _byte: u8) -> bool {
        false
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn read_byte(index: u32) -> Option<u8> {
        crate::hal::uart_16550::read_from(index as usize)
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn read_byte(_index: u32) -> Option<u8> {
        None
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn configure_baud(index: u32, baud: u32) -> bool {
        crate::hal::uart_16550::configure_baud(index as usize, baud)
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn configure_baud(_index: u32, _baud: u32) -> bool {
        false
    }
}

#[cfg(all(feature = "native-domains", cellos_wide))]
fn sas_record_end(
    sched: &crate::sched::scheduler::Scheduler,
    page_grants: Option<&RegionTable>,
    registered_grants: Option<&RegionTable>,
    caller_id: usize,
    ptr: usize,
) -> Option<usize> {
    let (cell_id, generation, root_tid) = {
        let caller = sched.tasks.get(&caller_id)?;
        if matches!(caller.state, TaskState::Retiring | TaskState::Terminated) {
            return None;
        }
        (caller.cell_id, caller.cell_generation, caller.root_tid)
    };
    let stack_end = sched
        .tasks
        .values()
        .filter(|task| {
            !matches!(task.state, TaskState::Retiring | TaskState::Terminated)
                && task.cell_id == cell_id
                && task.cell_generation == generation
                && task.root_tid == root_tid
        })
        .filter_map(|task| {
            task.user_stack.as_ref().and_then(|stack| {
                record_end_containing(stack.usable_start(), stack.usable_bytes(), ptr)
            })
        })
        .max();
    let segment_end = sched.tasks.get(&root_tid).and_then(|root| {
        (!matches!(root.state, TaskState::Retiring | TaskState::Terminated)
            && root.cell_id == cell_id
            && root.cell_generation == generation
            && root.root_tid == root_tid)
            .then_some(root.segment_mem.as_ref())?
            .and_then(|segments| segments.writable_page_end_containing(ptr))
    });
    let page_grant_end = page_grants.and_then(|grants| {
        grants
            .values()
            .filter(|grant| grant.owner == RegionOwner::task(caller_id, cell_id, generation))
            .filter_map(|grant| record_end_containing(grant.base(), grant.size, ptr))
            .max()
    });
    let registered_grant_end = registered_grants.and_then(|grants| {
        grants
            .values()
            .filter(|grant| grant.owner == RegionOwner::task(caller_id, cell_id, generation))
            .filter_map(|grant| record_end_containing(grant.base(), grant.size, ptr))
            .max()
    });
    [stack_end, segment_end, page_grant_end, registered_grant_end]
        .into_iter()
        .flatten()
        .max()
}

/// Verify contiguous coverage by live caller-owned writable records.
///
/// Each loop consumes at least one record. A gap, peer record, stale root, or
/// overflow fails closed without allocating or consulting page writability as
/// an ownership signal.
#[cfg(all(feature = "native-domains", cellos_wide))]
fn sas_caller_owned_span(
    sched: &crate::sched::scheduler::Scheduler,
    page_grants: Option<&RegionTable>,
    registered_grants: Option<&RegionTable>,
    caller_id: usize,
    ptr: usize,
    len: usize,
) -> bool {
    let Some(end) = ptr.checked_add(len) else {
        return false;
    };
    let mut cursor = ptr;
    while cursor < end {
        let Some(next) = sas_record_end(sched, page_grants, registered_grants, caller_id, cursor)
        else {
            return false;
        };
        if next <= cursor {
            return false;
        }
        cursor = next.min(end);
    }
    true
}

impl SessionOutputPin {
    fn reserve(
        caller: usize,
        ptr: usize,
        len: usize,
        view: &TaskCopyView,
    ) -> Result<Self, SyscallError> {
        // Same gate as `preflight_user_output`: the SAS ownership lease exists
        // only in builds with a domain backend.
        #[cfg(all(feature = "native-domains", cellos_wide))]
        {
            if !view.is_sas() {
                return Ok(Self { span: None });
            }
            let page_grants = grant_table_lock().lock();
            let registered = reg_grant_table_lock().lock();
            let guard = super::SCHEDULER.lock();
            let sched = guard.as_ref().ok_or(SyscallError::InvalidInput)?;
            if !sas_caller_owned_span(
                sched,
                page_grants.as_ref(),
                registered.as_ref(),
                caller,
                ptr,
                len,
            ) {
                return Err(SyscallError::InvalidInput);
            }
            view.validate_writable(ptr, len)
                .map_err(|_| SyscallError::InvalidInput)?;
            crate::memory::pin::pin(ptr, len, caller).map_err(|_| SyscallError::OutOfMemory)?;
            Ok(Self {
                span: Some((ptr, len, caller)),
            })
        }
        #[cfg(not(all(feature = "native-domains", cellos_wide)))]
        {
            let _ = caller;
            view.validate_writable(ptr, len)
                .map_err(|_| SyscallError::InvalidInput)?;
            Ok(Self { span: None })
        }
    }
}
impl Drop for SessionOutputPin {
    fn drop(&mut self) {
        if let Some((ptr, len, caller)) = self.span {
            // A killed parked syscall never resumes: the canonical quiescent
            // DMA/grant retirement funnel acknowledges its ordinary pin.
            crate::memory::pin::rollback_pin(ptr, len, caller);
        }
    }
}

impl MsgPayload {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => *len as usize,
            Self::Heap(vec) => vec.len(),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline { len, bytes } => &bytes[..*len as usize],
            Self::Heap(vec) => vec.as_slice(),
        }
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, ()> {
        if slice.len() <= MSG_PAYLOAD_INLINE {
            let mut bytes = [0u8; MSG_PAYLOAD_INLINE];
            bytes[..slice.len()].copy_from_slice(slice);
            Ok(Self::Inline {
                len: slice.len() as u8,
                bytes,
            })
        } else {
            let mut vec = alloc::vec::Vec::new();
            vec.try_reserve_exact(slice.len()).map_err(|_| ())?;
            vec.extend_from_slice(slice);
            Ok(Self::Heap(vec))
        }
    }
}

impl core::ops::Deref for MsgPayload {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

/// GrantShare for a private-root pair: publish the exact
/// `(target identity, generation, root, rights)` tuple into the receiver root,
/// then record the grantee in `grant`.
///
/// A redundant share of the same tuple is a no-op; any change republishes after
/// revoking the old receiver PTE and shooting it down. Unsupported rights
/// (write-only) and a non-live or non-domain target are refused with the
/// established share failure.
#[cfg(cellos_domain_grants)]
fn domain_grant_share_row(
    grant: &mut Region,
    caller_id: usize,
    target_tid: usize,
    target_root: &Arc<crate::memory::address_space::AddressSpace>,
    perm: GrantPerm,
) -> Result<(), SyscallError> {
    let base = grant.base();
    let Some(record) = grant.domain.as_ref() else {
        return Err(SyscallError::PermissionDenied);
    };
    if caller_id != grant.owner.tid || record.state() != super::domain_grant::DomainGrantState::Live
    {
        return Err(SyscallError::PermissionDenied);
    }
    let Some(rights) = super::domain_grant::rights_for(perm) else {
        log::warn!(
            "[grant] GrantShare {base:#x} refused: write-only has no domain page representation"
        );
        return Err(SyscallError::PermissionDenied);
    };
    let Some((cell, generation)) = live_task_binding(target_tid) else {
        return Err(SyscallError::PermissionDenied);
    };
    if !record.receiver_matches(cell, generation, target_root, rights) {
        record
            .publish(target_root, cell, generation, rights)
            .map_err(|error| {
                log::warn!("[grant] GrantShare {base:#x} receiver publish refused: {error:?}");
                SyscallError::PermissionDenied
            })?;
    }
    grant.set_shared_to(Some((target_tid, perm)));
    Ok(())
}

/// Drain the receiver a `GrantShare` is about to supersede, before the caller
/// takes the grant-table lock and before `DomainGrant::publish` takes the
/// record's gate (Phase 15, risk R4).
///
/// A re-share (a new receiver, or the same one with other rights) revokes the
/// old receiver PTE and waits for every online CPU to acknowledge the tag
/// invalidation. Inside `publish` that wait ran under the table lock and the
/// gate, so every other grant syscall on every CPU queued behind it, and gate
/// holds of over 10 ms were measured at four TCG CPUs. Draining here moves the
/// wait into its own gate hold under no table lock. The drain is still
/// serialized with every publication by the record's gate (audit H-5), and
/// `publish` re-checks the tuple and drains again if anything republished in
/// between, so this only moves where the wait happens.
///
/// It drains only for a share that would itself drain: the caller owns a `Live`
/// record, the rights are representable, and the target is a live private
/// root. Every other shape leaves the record untouched and is refused by
/// `domain_grant_share_row` exactly as before.
#[cfg(cellos_domain_grants)]
fn drain_superseded_receiver(
    caller_id: usize,
    grant_id: usize,
    target_tid: usize,
    target_root: &DomainRoot,
    perm: GrantPerm,
) {
    let owned = |owner: usize, domain: &Option<DomainRecord>| {
        (owner == caller_id)
            .then(|| clone_domain_record(domain))
            .flatten()
    };
    let record = {
        let tbl = grant_table_lock().lock();
        tbl.as_ref()
            .and_then(|map| map.get(&grant_id))
            .map(|grant| owned(grant.owner.tid, &grant.domain))
    };
    let record = match record {
        Some(record) => record,
        None => {
            let tbl = reg_grant_table_lock().lock();
            tbl.as_ref()
                .and_then(|map| map.get(&grant_id))
                .and_then(|grant| owned(grant.owner.tid, &grant.domain))
        }
    };
    let Some(record) = record else {
        return;
    };
    let (Some(rights), Some((cell, generation))) = (
        super::domain_grant::rights_for(perm),
        live_task_binding(target_tid),
    ) else {
        return;
    };
    if record.state() != super::domain_grant::DomainGrantState::Live
        || !target_root.is_live()
        || record.receiver_matches(cell, generation, target_root, rights)
    {
        return;
    }
    // An unconfirmed drain leaves the old receiver for `publish` to retry
    // under the gate, which then refuses the share as it always did.
    let _ = record.drain_receiver();
}

/// Resolve one grant row for a capable domain receiver.
///
/// Returns the resolved base on success and the established `usize::MAX`
/// "not authorized" sentinel for every refusal: an unresolvable or non-domain
/// receiver, a SAS-owned record, an unresolvable or dying owner, a record that
/// is not `Live`, missing or unsupported rights, a partial receiver mapping, or
/// an unacknowledged rollback invalidation.
#[cfg(cellos_domain_grants)]
#[allow(clippy::too_many_arguments)]
fn domain_grant_slice_row(
    owner: usize,
    base: usize,
    size: usize,
    shared_to: Option<(usize, GrantPerm)>,
    domain: Option<&Arc<super::domain_grant::DomainGrant>>,
    caller_id: usize,
    expected_owner_tid: usize,
    receiver: &Arc<crate::memory::address_space::AddressSpace>,
    size_out: Option<*mut usize>,
    lease_grant_id: Option<usize>,
) -> Result<usize, SyscallError> {
    // Same sentinel as an id absent from both tables (see `grant_owner_expected`).
    if !grant_owner_expected(expected_owner_tid, owner) {
        return Ok(usize::MAX);
    }
    let Some(record) = domain else {
        // A SAS-owned record's frames are not mapped in a private root, and a
        // mapping made here could not be revoked from the SAS side.
        return Ok(usize::MAX);
    };
    let live = record.state() == super::domain_grant::DomainGrantState::Live;
    // Test-hooks CONTROL (`GuardedCheck::SliceNeedsLive`): the stale-grant-id
    // fixture must catch a slice that resolves a record no longer `Live`.
    #[cfg(feature = "test-hooks")]
    let live = live
        || crate::task::fixture_controls::disabled(
            crate::task::fixture_controls::GuardedCheck::SliceNeedsLive,
        );
    if !record.owner_is_live() || !live {
        return Ok(usize::MAX);
    }
    // The owner resolves its own backing without a second mapping: the owner
    // domain page was published with the record.
    if caller_id == owner {
        if lease_grant_id.is_some_and(|grant_id| {
            !lease_grant_slice_locked(caller_id, grant_id, owner, base, size)
        }) {
            return Ok(usize::MAX);
        }
        super::user_out::write_resolved_optional_usize(size_out, size);
        return Ok(base);
    }
    let Some((shared_tid, perm)) = shared_to else {
        return Ok(usize::MAX);
    };
    let grantee = shared_tid == caller_id;
    // Test-hooks CONTROL (`GuardedCheck::SliceNeedsGrantee`): the forgery
    // fixture must catch a slice resolved for a domain the grant never named.
    #[cfg(feature = "test-hooks")]
    let grantee = grantee
        || crate::task::fixture_controls::disabled(
            crate::task::fixture_controls::GuardedCheck::SliceNeedsGrantee,
        );
    if !grantee {
        return Ok(usize::MAX);
    }
    let Some(rights) = super::domain_grant::rights_for(perm) else {
        return Ok(usize::MAX);
    };
    let Some((cell, generation)) = live_task_binding(caller_id) else {
        return Ok(usize::MAX);
    };
    // The lease is published before the receiver mapping and taken back if the
    // publish fails, all under the resolving table lock, so neither a failed
    // slice nor a racing GrantFree can observe one without the other. The
    // lease keeps the frames out of the allocator; at owner death the domain
    // reaper still revokes this receiver mapping, so a leased domain receiver
    // faults rather than reads reused memory.
    if lease_grant_id
        .is_some_and(|grant_id| !lease_grant_slice_locked(caller_id, grant_id, owner, base, size))
    {
        return Ok(usize::MAX);
    }
    match record.publish(receiver, cell, generation, rights) {
        Ok(()) => {
            super::user_out::write_resolved_optional_usize(size_out, size);
            Ok(base)
        }
        Err(error) => {
            if let Some(grant_id) = lease_grant_id {
                crate::memory::pin::unlease_grant_hold(caller_id, owner, grant_id);
            }
            log::warn!(
                "[grant] GrantSlice {base:#x} refused for domain task {caller_id}: {error:?}"
            );
            Ok(usize::MAX)
        }
    }
}

/// GrantSlice entry point for a capable domain receiver: resolve the row in
/// either table and apply the lifecycle-aware resolver under the same lock that
/// linearizes teardown.
#[cfg(cellos_domain_grants)]
fn resolve_domain_grant_slice(
    caller_id: usize,
    grant_id: usize,
    size_out_ptr: usize,
    expected_owner_tid: usize,
    lease: bool,
    receiver: &Arc<crate::memory::address_space::AddressSpace>,
) -> Result<usize, SyscallError> {
    let lease_grant_id = lease.then_some(grant_id);
    let size_out = super::user_out::resolve_optional_usize_slot(caller_id, size_out_ptr)?;
    {
        let tbl = grant_table_lock().lock();
        if let Some(grant) = tbl.as_ref().and_then(|map| map.get(&grant_id)) {
            return domain_grant_slice_row(
                grant.owner.tid,
                grant.base(),
                grant.size,
                grant.shared_to(),
                grant.domain.as_ref(),
                caller_id,
                expected_owner_tid,
                receiver,
                size_out,
                lease_grant_id,
            );
        }
    }
    let tbl = reg_grant_table_lock().lock();
    let Some(grant) = tbl.as_ref().and_then(|map| map.get(&grant_id)) else {
        return Ok(usize::MAX);
    };
    domain_grant_slice_row(
        grant.owner.tid,
        grant.base(),
        grant.size,
        grant.shared_to(),
        grant.domain.as_ref(),
        caller_id,
        expected_owner_tid,
        receiver,
        size_out,
        lease_grant_id,
    )
}

/// Return the syscall allowlist only for an active caller.
///
/// Task ID zero is the explicit kernel-context sentinel. Every nonzero caller
/// must still have a dispatch-visible, non-terminal task record; a missing or
/// terminal user task is denied rather than inheriting kernel authority.
fn syscall_allowlist_for(caller_id: usize) -> Option<u64> {
    if caller_id == 0 {
        return Some(u64::MAX);
    }

    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&caller_id))
        .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
        .map(|task| task.syscall_allowlist)
}

/// The `ViSyscall` a decoded [`Syscall`] corresponds to, from the generated
/// [`opcode_of`] (`libs/abi/abi/syscalls.toml`). `None` for raw opcodes
/// outside `ViSyscall` (500–503, 100/110/111, 502, bench 504) and for kernel
/// variants with no opcode. Total over every api row, unlike the hand-written
/// map it replaces (architecture review H-5).
fn syscall_to_vi(syscall: &Syscall) -> Option<abi::syscall::ViSyscall> {
    match abi::syscall::ViSyscall::from(opcode_of(syscall)) {
        abi::syscall::ViSyscall::Unknown => None,
        vi => Some(vi),
    }
}

// `opcode_of` and `RAW_OPCODES`, generated by `kernel/build.rs`.
include!(concat!(env!("OUT_DIR"), "/syscall_opcodes.rs"));

mod incarnation_tids {
    /// Root plus the per-Cell thread cap.
    const CAP: usize = crate::sched::scheduler::MAX_THREADS_PER_CELL + 1;

    pub(super) struct Tids {
        ids: [usize; CAP],
        len: usize,
    }

    impl Default for Tids {
        fn default() -> Self {
            Self {
                ids: [0; CAP],
                len: 0,
            }
        }
    }

    impl Tids {
        pub(super) fn push(&mut self, id: usize) {
            // The thread cap bounds an incarnation; a surplus id is a broken
            // invariant, and dropping it would skip a teardown.
            assert!(self.len < CAP, "Cell incarnation exceeds its thread cap");
            self.ids[self.len] = id;
            self.len += 1;
        }

        pub(super) fn iter(&self) -> core::slice::Iter<'_, usize> {
            self.ids[..self.len].iter()
        }
    }
}

/// One `CapRevoke` at a time: the pending mask on a root is a single word, and
/// two overlapping revokes must not clear each other's bits. A concurrent
/// caller gets `TryAgain`.
static REVOKE_IN_PROGRESS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

impl RevokeSerial {
    fn acquire() -> Option<Self> {
        use core::sync::atomic::Ordering;
        REVOKE_IN_PROGRESS
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for RevokeSerial {
    fn drop(&mut self) {
        REVOKE_IN_PROGRESS.store(false, core::sync::atomic::Ordering::Release);
    }
}

impl Drop for PendingRevoke {
    fn drop(&mut self) {
        if let Some(sched) = super::SCHEDULER.lock().as_mut() {
            if let Some(root) = sched.tasks.get_mut(&self.root_tid) {
                if (root.cell_id, root.cell_generation) == self.incarnation {
                    root.revoke_pending &= !self.mask;
                }
            }
        }
    }
}

/// Run a decoded syscall for an in-kernel caller (selftests, fixtures) through
/// the same admission gate a trap uses — `check_allowlist` on the opcode the
/// variant corresponds to ([`opcode_of`]) — then dispatch it. This is the one
/// allowlist gate (architecture review H-5); a denial is audited.
///
/// `caller_id` is the ID of the task the call is made as.
pub fn handle_syscall(caller_id: usize, syscall: Syscall) -> SyscallResult {
    if let Err(detail) = admission(opcode_of(&syscall), caller_id) {
        // A live caller's refusal is audited with its real cause, as the
        // variant gate this replaces did; a missing task is refused silently.
        if syscall_allowlist_for(caller_id).is_some() {
            audit_syscall_denied(caller_id, detail);
        }
        return Err(SyscallError::PermissionDenied);
    }
    dispatch_admitted(caller_id, syscall)
}

/// Dispatch a syscall the kernel itself issues on a Cell's behalf (for example
/// the GPU scanout path allocating a Grant for the GPU Cell). Kernel origin is
/// trusted: the Cell's allowlist is not consulted. Never reachable from a trap.
pub(in crate::task) fn handle_syscall_kernel_origin(
    caller_id: usize,
    syscall: Syscall,
) -> SyscallResult {
    dispatch_admitted(caller_id, syscall)
}

/// Dispatch an already-admitted syscall to its handler. Callers: the trap entry
/// after `check_allowlist(raw opcode)`, [`handle_syscall`] after the same gate,
/// and [`handle_syscall_kernel_origin`].
fn dispatch_admitted(caller_id: usize, syscall: Syscall) -> SyscallResult {
    #[cfg(feature = "p01-trace")]
    let mut trace = {
        let route = match &syscall {
            Syscall::SpawnFromElf { .. } => 1,
            Syscall::SpawnFromPath { .. } => 2,
            Syscall::SpawnPinned { .. } => 3,
            _ => 0,
        };
        if route == 0 {
            None
        } else {
            let parent = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .map(|task| crate::p01_measurement::Owner::of(task));
            parent.and_then(|owner| crate::p01_measurement::ingress(owner, route))
        }
    };
    let result = handle_syscall_inner(
        caller_id,
        syscall,
        #[cfg(feature = "p01-trace")]
        &mut trace,
    );
    #[cfg(feature = "p01-trace")]
    {
        crate::p01_measurement::outcome(trace, result);
        crate::p01_measurement::safe_point();
    }
    result
}

fn handle_syscall_inner(
    caller_id: usize,
    syscall: Syscall,
    #[cfg(feature = "p01-trace")] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    if syscall_allowlist_for(caller_id).is_none() {
        log::warn!("[kernel] syscall denied for non-live tid {}", caller_id);
        return Err(SyscallError::PermissionDenied);
    }

    let _staged_spawn_argv_cleanup = staged_spawn_argv_cleanup(&syscall, caller_id);
    #[cfg(feature = "p01-trace")]
    {
        // These non-fixture routes do not have historical per-object provenance
        // in this observer. Preserve the uncertainty before any source can lose it.
        let class = match &syscall {
            Syscall::GrantAlloc { .. }
            | Syscall::GrantShare { .. }
            | Syscall::GrantSlice { .. }
            | Syscall::GrantFree { .. }
            | Syscall::GrantRegister { .. }
            | Syscall::GrantUnregister { .. }
            | Syscall::GrantCacheSyncBegin { .. }
            | Syscall::GrantCacheSyncComplete { .. }
            | Syscall::ShmAlloc { .. }
            | Syscall::ShmMap { .. } => Some(3),
            Syscall::GrantDma { .. } => Some(5),
            Syscall::CreateVm { .. }
            | Syscall::CreateVcpu { .. }
            | Syscall::MapGuestMemory { .. } => Some(8),
            _ => None,
        };
        if let Some(class) = class {
            if let Some(owner) = crate::p01_resources::owner_for_tid(caller_id) {
                crate::p01_resources::unknown(owner, class);
            }
        }
    }

    // Every public receive form ends the prior VFS request. Masked dependency
    // receives preserve the outer grant authority and bounded reply token.
    if let Syscall::Recv { mask, .. }
    | Syscall::RecvTimeout { mask, .. }
    | Syscall::TryRecv { mask, .. }
    | Syscall::RecvScatter { mask, .. } = &syscall
    {
        let dropped = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .and_then(|task| task.begin_receive_context(*mask))
        };
        finish_vfs_context_drop(caller_id, dropped);
        // Every receive form, masked or not, ends the caller's `GrantSlice`
        // leases: the holder has finished with the pointers it resolved.
        if crate::memory::pin::grant_leases_live() {
            free_released_grant_lease_frames(
                caller_id,
                crate::memory::pin::release_grant_holder_leases(caller_id),
            );
        }
    }
    end_grant_leases_at_send(caller_id, &syscall);

    match syscall {
        Syscall::CellSessionV1 {
            request_ptr,
            request_len,
            result_ptr,
            result_capacity,
        } => spawn::cell_session_v1(
            caller_id,
            request_ptr,
            request_len,
            result_ptr,
            result_capacity,
        ),
        // --- Hubris ABI Implementation ---
        Syscall::IpcSubmit { target, ptr, len } => ipc::ipc_submit(caller_id, target, ptr, len),
        Syscall::IpcTake {
            token,
            ptr,
            len,
            status_ptr,
        } => ipc::ipc_take(caller_id, token, ptr, len, status_ptr),
        Syscall::IpcWait { ticks } => ipc::ipc_wait(caller_id, ticks),
        Syscall::IpcCancel { token } => ipc::ipc_cancel(caller_id, token),
        Syscall::IpcCurrent => ipc::ipc_current(caller_id),
        Syscall::IpcReply { token, ptr, len } => ipc::ipc_reply(caller_id, token, ptr, len),
        Syscall::Send {
            target,
            msg_ptr,
            msg_len,
        } => ipc::send(caller_id, target, msg_ptr, msg_len),
        Syscall::TrySend {
            target,
            msg_ptr,
            msg_len,
        } => ipc::try_send(caller_id, target, msg_ptr, msg_len),
        Syscall::Recv {
            mask,
            buf_ptr,
            buf_len,
            attest_caller,
        } => ipc::recv(caller_id, mask, buf_ptr, buf_len, attest_caller),
        // ── Scatter/gather IPC ────────────────────────────────────────────────
        Syscall::SendGather {
            target,
            iovec_ptr,
            iovec_count,
        } => ipc::send_gather(caller_id, target, iovec_ptr, iovec_count),
        Syscall::RecvScatter {
            mask,
            iovec_ptr,
            iovec_count,
        } => ipc::recv_scatter(caller_id, mask, iovec_ptr, iovec_count),
        Syscall::RecvTimeout {
            mask,
            buf_ptr,
            buf_len,
            deadline,
            attest_caller,
            timeout_is_error,
        } => ipc::recv_timeout(
            caller_id,
            mask,
            buf_ptr,
            buf_len,
            deadline,
            attest_caller,
            timeout_is_error,
        ),
        Syscall::TryRecv {
            mask,
            buf_ptr,
            buf_len,
            attest_caller,
        } => ipc::try_recv(caller_id, mask, buf_ptr, buf_len, attest_caller),
        Syscall::Spawn { entry, arg } => spawn::spawn(caller_id, entry, arg),
        Syscall::SetTlsBase { base } => spawn::set_tls_base(caller_id, base),
        Syscall::PipeCreate { capacity, out_ptr } => fs::pipe_create(caller_id, capacity, out_ptr),
        Syscall::PipeRead {
            handle,
            buf_ptr,
            len,
            timeout_ticks,
        } => fs::pipe_read(caller_id, handle, buf_ptr, len, timeout_ticks),
        Syscall::PipeWrite {
            handle,
            buf_ptr,
            len,
            timeout_ticks,
        } => fs::pipe_write(caller_id, handle, buf_ptr, len, timeout_ticks),
        Syscall::PipeClose { handle } => fs::pipe_close(caller_id, handle),
        Syscall::PipeShare { handle, target_tid } => fs::pipe_share(caller_id, handle, target_tid),
        Syscall::Wait { pid } => ipc::wait(caller_id, pid),
        Syscall::ShmAlloc { size } => grant::shm_alloc(caller_id, size),
        Syscall::ShmMap { handle, target_pid } => grant::shm_map(caller_id, handle, target_pid),
        Syscall::FutexWait {
            addr,
            expected,
            timeout_ticks,
        } => ipc::futex_wait(caller_id, addr, expected, timeout_ticks),
        Syscall::FutexWake { addr, count } => ipc::futex_wake(caller_id, addr, count),
        Syscall::Log { msg_ptr, msg_len } => ipc::log(caller_id, msg_ptr, msg_len),
        Syscall::Exit { code } => spawn::exit(caller_id, code),

        Syscall::ForceExit { tid } => spawn::force_exit(caller_id, tid),

        Syscall::CapRevoke {
            target_tid,
            cap_mask,
        } => authority::cap_revoke(caller_id, target_tid, cap_mask),

        Syscall::NotifyOnExit { watched } => spawn::notify_on_exit(caller_id, watched),

        Syscall::QueryRetirement {
            root_tid,
            out_ptr,
            out_len,
        } => spawn::query_retirement(caller_id, root_tid, out_ptr, out_len),

        Syscall::LookupBlockRoute {
            index,
            out_ptr,
            out_len,
            reserved,
        } => dev::lookup_block_route(caller_id, index, out_ptr, out_len, reserved),

        Syscall::QueryMachineFacts {
            out_ptr,
            out_len,
            a2,
            a3,
        } => dev::query_machine_facts(caller_id, out_ptr, out_len, a2, a3),

        Syscall::RegisterService {
            service_id,
            tid,
            flags,
        } => authority::register_service(caller_id, service_id, tid, flags),
        Syscall::LookupServiceBound {
            service_id,
            out_ptr,
            out_len,
        } => authority::lookup_service_bound(caller_id, service_id, out_ptr, out_len),
        Syscall::Heartbeat { interval } => ipc::heartbeat(caller_id, interval),

        // --- Legacy Implementation ---
        Syscall::Yield => spawn::yield_now(),
        Syscall::ServiceLookup { name_ptr, name_len } => {
            authority::service_lookup(caller_id, name_ptr, name_len)
        }
        Syscall::Open { path_ptr, path_len } => fs::open(caller_id, path_ptr, path_len),
        Syscall::Read {
            fd,
            buf_ptr,
            buf_len,
        } => fs::read(caller_id, fd, buf_ptr, buf_len),
        Syscall::Close { fd } => fs::close(fd),
        Syscall::ReadDir {
            fd,
            buf_ptr,
            buf_len,
        } => fs::read_dir(caller_id, fd, buf_ptr, buf_len),
        Syscall::Fstat {
            fd,
            out_ptr,
            out_len,
        } => fs::fstat(caller_id, fd, out_ptr, out_len),
        Syscall::Rename {
            old_ptr,
            old_len,
            new_ptr,
            new_len,
        } => fs::rename(caller_id, old_ptr, old_len, new_ptr, new_len),
        // Syscall::Remove removed
        Syscall::ChDir { path_ptr, path_len } => fs::ch_dir(caller_id, path_ptr, path_len),
        Syscall::GetCwd { buf_ptr, buf_len } => fs::get_cwd(caller_id, buf_ptr, buf_len),
        Syscall::AnonMemory {
            op,
            addr,
            len,
            flags,
        } => grant::anon_memory(caller_id, op, addr, len, flags),
        Syscall::Write {
            fd,
            buf_ptr,
            buf_len,
        } => fs::write(caller_id, fd, buf_ptr, buf_len),
        Syscall::MkDir { path_ptr, path_len } => fs::mk_dir(caller_id, path_ptr, path_len),
        Syscall::SpawnFromPath { path_ptr, path_len } => spawn::spawn_from_path(
            caller_id,
            path_ptr,
            path_len,
            #[cfg(feature = "p01-trace")]
            trace,
        ),

        Syscall::SpawnSetDirs { carrier_ptr } => spawn::spawn_set_dirs(caller_id, carrier_ptr),

        Syscall::QueryDirHandles {
            cell_id,
            buf_ptr,
            buf_len,
        } => fs::query_dir_handles(caller_id, cell_id, buf_ptr, buf_len),

        Syscall::ResolveCellOwner {
            cell_id,
            generation,
            out_ptr,
            out_len,
        } => authority::resolve_cell_owner(caller_id, cell_id, generation, out_ptr, out_len),

        Syscall::WatchCellOwner {
            cell_id,
            generation,
            out_ptr,
            out_len,
        } => authority::watch_cell_owner(caller_id, cell_id, generation, out_ptr, out_len),

        Syscall::ResolveCellOwnerRecord {
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        } => authority::resolve_cell_owner_record(
            caller_id,
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        ),

        Syscall::ResolveCellImage {
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        } => authority::resolve_cell_image(caller_id, request_ptr, request_len, out_ptr, out_len),

        Syscall::WatchCellOwnerRecord {
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        } => authority::watch_cell_owner_record(
            caller_id,
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        ),

        Syscall::CancelCellOwnerWatch { token } => {
            authority::cancel_cell_owner_watch(caller_id, token)
        }

        Syscall::SpawnFromElf {
            grant_id,
            len,
            path_ptr,
            path_len,
        } => spawn::spawn_from_elf(
            caller_id,
            grant_id,
            len,
            path_ptr,
            path_len,
            #[cfg(feature = "p01-trace")]
            trace,
        ),

        Syscall::SpawnPinned {
            path_ptr,
            path_len,
            priority,
            core_id,
        } => spawn::spawn_pinned(
            caller_id,
            path_ptr,
            path_len,
            priority,
            core_id,
            #[cfg(feature = "p01-trace")]
            trace,
        ),

        Syscall::OpenCap { path_ptr, path_len } => fs::open_cap(caller_id, path_ptr, path_len),

        Syscall::ReadCap {
            cap_id,
            buf_ptr,
            buf_len,
        } => fs::read_cap(caller_id, cap_id, buf_ptr, buf_len),

        Syscall::SeekCap {
            cap_id,
            offset,
            whence,
        } => fs::seek_cap(caller_id, cap_id, offset, whence),

        Syscall::WriteCap {
            cap_id,
            buf_ptr,
            buf_len,
        } => fs::write_cap(caller_id, cap_id, buf_ptr, buf_len),

        Syscall::StatCap { cap_id } => fs::stat_cap(caller_id, cap_id),

        Syscall::TruncateCap { cap_id, len } => fs::truncate_cap(caller_id, cap_id, len),

        Syscall::SyncCap { cap_id } => fs::sync_cap(caller_id, cap_id),

        Syscall::GrantDma { bdf, phys, size } => grant::grant_dma(caller_id, bdf, phys, size),

        Syscall::CloseCap { cap_id } => fs::close_cap(caller_id, cap_id),

        Syscall::SpawnFromMem { args_ptr } => spawn::spawn_from_mem(
            caller_id,
            args_ptr,
            #[cfg(feature = "p01-trace")]
            trace,
        ),

        Syscall::Create { path_ptr, path_len } => fs::create(caller_id, path_ptr, path_len),
        Syscall::SetTimer { deadline } => ipc::set_timer(deadline),

        Syscall::GetProcs { buf_ptr, buf_len } => ipc::get_procs(caller_id, buf_ptr, buf_len),

        Syscall::GetProcs2 { buf_ptr, buf_len } => ipc::get_procs2(caller_id, buf_ptr, buf_len),

        Syscall::MemInfo { out_ptr, out_len } => ipc::mem_info(caller_id, out_ptr, out_len),

        #[cfg(feature = "bench-counters")]
        Syscall::BenchCounters { out_ptr, out_len } => {
            misc::bench_counters(caller_id, out_ptr, out_len)
        }

        Syscall::TelemetrySnapshot {
            out_ptr,
            out_len,
            flags,
            reserved,
        } => spawn::telemetry_snapshot(caller_id, out_ptr, out_len, flags, reserved),

        Syscall::Seek { fd, offset, whence } => fs::seek(fd, offset, whence),

        Syscall::FileOp { op, arg1, arg2 } => fs::file_op(caller_id, op, arg1, arg2),

        Syscall::GetTime { op } => ipc::get_time(op),
        Syscall::GetCpuCount => dev::get_cpu_count(),
        // Compositor-only; the source is copied through the caller's copy view
        // into a GPU-Cell-owned Grant before the hand-off, so the GPU Cell never
        // reads compositor memory; the framebuffer fallback stages it the same
        // way (H-26).
        Syscall::GpuFlush {
            data_ptr,
            data_len,
            xy,
            wh,
        } => dev::gpu_flush(caller_id, data_ptr, data_len, xy, wh),
        Syscall::GpuCursor {
            op,
            data_ptr,
            xy,
            hot,
        } => dev::gpu_cursor(caller_id, op, data_ptr, xy, hot),
        Syscall::GpuGetResolution => dev::gpu_get_resolution(),
        Syscall::StateStash {
            key,
            buf_ptr,
            buf_len,
        } => spawn::state_stash(caller_id, key, buf_ptr, buf_len),
        Syscall::StateRestore {
            key,
            buf_ptr,
            buf_len,
        } => spawn::state_restore(caller_id, key, buf_ptr, buf_len),
        // 412: StateStashClear — owner (open), sealed destination (consume) or a
        // supervisor (abort). Absent keys succeed; a foreign entry is refused.
        Syscall::StateStashClear { key } => spawn::state_stash_clear(caller_id, key),

        // ── Supervisor Primitives (P03) ───────────────────────────────────────

        // 413: FreezeCell — stop a running Cell from being scheduled.
        // The frozen cell still exists; its queued IPC is preserved.
        Syscall::FreezeCell { target_tid } => authority::freeze_cell(caller_id, target_tid),

        // 422: PauseService — atomically hide the expected provider from new
        // lookups without stopping it, so it can process Snapshot IPC.
        Syscall::PauseService {
            service_id,
            expected_tid,
        } => authority::pause_service(caller_id, service_id, expected_tid),

        // 414: plain resume or atomic old-provider -> replacement cutover.
        Syscall::ResumeCell {
            target_tid,
            source_tid,
            service_id,
            reserved,
        } => authority::resume_cell(caller_id, target_tid, source_tid, service_id, reserved),

        // 415: KillCell — terminate a Cell and reclaim its resources.
        Syscall::KillCell {
            target_tid,
            exit_code,
        } => authority::kill_cell(caller_id, target_tid, exit_code),

        // 419: QueryHotswapReady — non-blocking check of a cell's hotswap_ready flag.
        // Returns 1 if the cell has called sys_hotswap_ready(), 0 if not yet,
        // usize::MAX if the tid is not found.
        Syscall::QueryHotswapReady { target_tid } => {
            spawn::query_hotswap_ready(caller_id, target_tid)
        }

        Syscall::SpawnReplacement {
            old_tid,
            path_ptr,
            path_len,
        } => spawn::spawn_replacement(caller_id, old_tid, path_ptr, path_len),

        // ── Driver Cell Registration (P00) ───────────────────────────────────

        // 416: RegisterBlockDriver — bind the caller's owned block device route.
        Syscall::RegisterBlockDriver { kind, id, reserved } => {
            dev::register_block_driver(caller_id, kind, id, reserved)
        }

        // 417: RegisterNicDriver — announce caller as the active NIC driver.
        Syscall::RegisterNicDriver => dev::register_nic_driver(caller_id),

        // 423: RegisterUsbHidProducer — announce caller as the USB HID event
        // producer, publishing service::USB_HID_PRODUCER for the input service's
        // producer gate. Requires UsbDriverCap and never touches the singleton
        // NIC role, so an xHCI cell on a PC with e1000/virtio-net cannot steal
        // the network route.
        Syscall::RegisterUsbHidProducer => dev::register_usb_hid_producer(caller_id),

        // 418: FindPcieDevice — query ECAM table for a device by class triple.
        // Writes a `PcieDeviceInfo` record to `out_ptr` and returns 1 if found.
        // Requires PcieDriverCap and a class row for the caller's measured
        // program in `catalog::PCI_DRIVERS`; also records BDF ownership in
        // resource_registry. Every Ethernet driver claims through 424.
        Syscall::FindPcieDevice {
            class,
            subclass,
            prog_if,
            out_ptr,
        } => dev::find_pcie_device(caller_id, class, subclass, prog_if, out_ptr),

        // 424: FindPcieDeviceByVendor — query ECAM table for an exact
        // (vendor_id, device_id). Same hand-out semantics as 418 (PcieDriverCap,
        // BDF ownership, 48-byte `PcieDeviceInfo`), but the match names the
        // device, so two Driver Cells that share a class triple each claim their
        // own controller without a sibling-exit race.
        Syscall::FindPcieDeviceByVendor {
            vendor_id,
            device_id,
            out_ptr,
        } => dev::find_pcie_device_by_vendor(caller_id, vendor_id, device_id, out_ptr),

        // 434/435 (ADR-0031 CP1 §3–§4): the owned function's record, and its
        // MSI-X/MSI vectors. PcieDriverCap plus BDF ownership (418/424) gate both.
        Syscall::QueryPcieFunction {
            bdf,
            out_ptr,
            out_len,
            reserved,
        } => dev::query_pcie_function_syscall(caller_id, bdf, out_ptr, out_len, reserved),
        Syscall::AllocPcieIrq {
            bdf,
            want,
            mode,
            reserved,
        } => dev::alloc_pcie_irq_syscall(caller_id, bdf, want, mode, reserved),
        Syscall::SystemPower { action, a1, a2, a3 } => {
            dev::system_power_syscall(caller_id, action, a1, a2, a3)
        }

        // 437-439 (ADR-0033 CP1 §5): client-memory DMA export leases. 437/439
        // gate on own memory / lease party; 438 also needs bit 48,
        // PcieDriverCap and BDF ownership (checked in `dma_export::map`).
        Syscall::ExportDmaRange {
            va,
            len,
            holder_service,
            flags,
        } => grant::export_dma_range(caller_id, va, len, holder_service, flags),
        Syscall::MapDmaExport {
            export_id,
            bdf,
            out_ptr,
            out_len,
        } => grant::map_dma_export(caller_id, export_id, bdf, out_ptr, out_len),
        Syscall::RevokeDmaExport {
            export_id,
            a1,
            a2,
            a3,
        } => grant::revoke_dma_export(caller_id, export_id, a1, a2, a3),

        // 425-428: the serial-port surface. The kernel owns the 16550 mechanism
        // (it probed and programmed every port at boot); a cell holding the
        // `serial_port` capability drives only the ports that probe accepted, so
        // there is no way to reach port space the machine did not prove it has.
        // Index 0 is the console; the extras are polled, not interrupt-driven.
        Syscall::SerialPortInfo { index, out_ptr } => {
            dev::serial_port_info(caller_id, index, out_ptr)
        }
        Syscall::SerialWrite { index, ptr, len } => dev::serial_write(caller_id, index, ptr, len),
        Syscall::SerialRead { index, ptr, len } => dev::serial_read(caller_id, index, ptr, len),
        Syscall::SerialConfigure { index, baud } => dev::serial_configure(caller_id, index, baud),

        // 234: WaitIrq — block until hardware IRQ fires (Driver Cell).
        // ISR calls irq_wait::signal_irq (atomic only; no lock, no scheduler access).
        // Scheduler sweep (pick_next) does the actual Ready transition.
        Syscall::WaitIrq { irq, mmio_base } => dev::wait_irq(caller_id, irq, mmio_base),

        // 235: RegisterPcieBar — Platform Cell announces a discovered PCIe BAR.
        // Populates the MMIO allowlist only. BDF ownership belongs to the Driver
        // Cell that claims the device through FindPcieDevice.
        Syscall::RegisterPcieBar { bdf: _, base, len } => {
            dev::register_pcie_bar(caller_id, base, len)
        }

        // 236: RegisterPciDevice — Platform Cell announces a device with class/BAR info.
        // Populates PCI_DEVICES so sys_find_pcie_device queries work without kernel ECAM scan.
        // After each registration, attempt deferred IOMMU init (no-op until IOMMU device appears).
        // Every function that answers is recorded, as the bus-0 scan records it, so
        // the machine fingerprint (433) does not depend on the driver table. `1`
        // means recorded but an Ethernet ID no Driver Cell drives: the handout
        // queries never offer it, and the Platform Cell names its ID. A BDF with no
        // function inside the admitted window is `FileNotFound` and not recorded.
        Syscall::RegisterPciDevice {
            bdf,
            cls,
            bar0_base,
            bar0_size,
        } => dev::register_pci_device(caller_id, bdf, cls, bar0_base, bar0_size),

        // 237: ReadLog — drain the user-log (source 0) or audit (source 1) ring
        // into a cell-provided buffer. The caller must have allowlist bit 54
        // (ReadLog) set in its manifest; the audit ring also needs SupervisorCap.
        // Returns the number of bytes actually copied (0 when nothing available).
        Syscall::ReadLog {
            buf_ptr,
            max,
            source,
        } => ipc::read_log(caller_id, buf_ptr, max, source),

        Syscall::BlkFlush => dev::blk_flush(caller_id),
        Syscall::Shutdown => dev::shutdown(caller_id),
        Syscall::BlkRead { sector, buf_ptr } => dev::blk_read(caller_id, sector, buf_ptr),
        Syscall::BlkWrite { sector, buf_ptr } => dev::blk_write(caller_id, sector, buf_ptr),
        Syscall::HotSwapReady => spawn::hot_swap_ready(caller_id),

        Syscall::Snapshot => spawn::snapshot(caller_id),

        // ── Zero-Copy Grant Syscalls (Phase 01, Storage 2.0) ─────────────────
        Syscall::GrantAlloc { size } => grant::grant_alloc(caller_id, size),
        Syscall::GrantShare {
            grant_id,
            target_cell,
            perm,
        } => grant::grant_share(caller_id, grant_id, target_cell, perm),

        Syscall::GrantSlice {
            grant_id,
            size_out_ptr,
            expected_owner_tid,
            flags,
        } => grant::grant_slice(caller_id, grant_id, size_out_ptr, expected_owner_tid, flags),

        Syscall::GrantFree { grant_id } => grant::grant_free(caller_id, grant_id),

        Syscall::GrantCacheSyncBegin {
            grant_id,
            offset,
            len,
        } => grant::grant_cache_sync_begin(caller_id, grant_id, offset, len),

        Syscall::GrantCacheSyncComplete { token } => {
            grant::grant_cache_sync_complete(caller_id, token)
        }

        Syscall::RegisterDisplayFramebuffer {
            base,
            size,
            packed_dimensions,
            pitch,
        } => dev::register_display_framebuffer(caller_id, base, size, packed_dimensions, pitch),

        Syscall::GrantRegister { size } => grant::grant_register(caller_id, size),
        Syscall::GrantUnregister { reg_id } => grant::grant_unregister(caller_id, reg_id),

        Syscall::WaitForEvent { mask, deadline } => ipc::wait_for_event(caller_id, mask, deadline),

        Syscall::WaitCompletion {
            mask,
            deadline,
            out_ptr,
        } => ipc::wait_completion(caller_id, mask, deadline, out_ptr),

        Syscall::RequestMmio { base, len } => dev::request_mmio(caller_id, base, len),

        // 442: ProbeVirtioMmio — the kernel reads a slot's identity registers so
        // a Driver Cell can pick its slot before claiming it; a claim is
        // exclusive and never released, so probing by claiming would lock
        // sibling drivers out of their slots.
        Syscall::ProbeVirtioMmio { base, reserved } => {
            dev::probe_virtio_mmio(caller_id, base, reserved)
        }

        Syscall::GetRandom { buf_ptr, len } => ipc::get_random(caller_id, buf_ptr, len),

        Syscall::BlkReadAsync { sector, grant_id } => {
            dev::blk_read_async(caller_id, sector, grant_id)
        }

        // ── Hypervisor syscalls 220-225 (HypervisorCap ZST-gated) ────────────────
        Syscall::CreateVm { guest_pages } => vm::create_vm(caller_id, guest_pages),

        Syscall::CreateVcpu { vm_id, entry_pc } => vm::create_vcpu(caller_id, vm_id, entry_pc),

        Syscall::MapGuestMemory {
            vm_id,
            ipa,
            size,
            writable,
        } => vm::map_guest_memory(caller_id, vm_id, ipa, size, writable),

        Syscall::RunVcpu {
            vm_id,
            vcpu_id,
            budget_ns,
            out_ptr,
        } => vm::run_vcpu(caller_id, vm_id, vcpu_id, budget_ns, out_ptr),

        Syscall::VcpuRegs {
            vm_id,
            vcpu_id,
            buf_ptr,
            mode,
        } => vm::vcpu_regs(caller_id, vm_id, vcpu_id, buf_ptr, mode),

        Syscall::InjectIrq {
            vm_id,
            vcpu_id,
            intid,
        } => vm::inject_irq(caller_id, vm_id, vcpu_id, intid),

        Syscall::WriteGuestMemory {
            vm_id,
            gpa,
            src_ptr,
            len,
        } => vm::write_guest_memory(caller_id, vm_id, gpa, src_ptr, len),

        Syscall::ReadGuestMemory {
            vm_id,
            gpa,
            dst_ptr,
            len,
        } => vm::read_guest_memory(caller_id, vm_id, gpa, dst_ptr, len),
    }
}

#[cfg(not(target_arch = "riscv32"))]
use crate::hal::arch::ViTrapFrame;
use abi::syscall::ViSyscall;

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
fn with_test_sum<T>(operation: impl FnOnce() -> T) -> T {
    // Test-only dispatch must reproduce the real trap entry's SUM state.
    let saved_sstatus = crate::arch::user_access::test_open_saved();
    let result = operation();
    if saved_sstatus & 0x40000 == 0 {
        // Restore exactly the caller hart's prior SUM bit.
        crate::arch::user_access::test_close();
    }
    result
}

/// Exercise the production raw-opcode decoder and handler without a trap frame.
///
/// Test hooks use this only for in-kernel QEMU fixtures whose calls would
/// otherwise need a hand-written U-mode trampoline. RV64's real syscall entry
/// runs with `sstatus.SUM` set; mirror that per-hart state around the handler.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn dispatch_raw_for_test(
    caller_id: usize,
    syscall_id: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> SyscallResult {
    check_allowlist(syscall_id, caller_id)?;
    let syscall = map_syscall(syscall_id, a0, a1, a2, a3).ok_or(SyscallError::InvalidCommand)?;
    with_test_sum(|| dispatch_admitted(caller_id, syscall))
}

/// Per-cell syscall allowlist gate.
///
/// Reads the caller's `syscall_allowlist` bitmask and returns
/// `Err(SyscallError::PermissionDenied)` if the opcode's bit is not set, or if
/// the opcode is one of the S2-T telemetry calls and the caller's CapSet lacks
/// the matching right (`observe_right_held`).
/// The SCHEDULER lock is acquired and released here — callers must NOT hold it.
fn check_allowlist(syscall_id: usize, caller_id: usize) -> Result<(), SyscallError> {
    admission(syscall_id, caller_id).map_err(|_| SyscallError::PermissionDenied)
}

/// Audit detail of an admission refusal: the allowlist bit that was missing, or
/// [`DENIED_RIGHT`] `|` the opcode's bit when the bit was present but the edge
/// withholds its observation right, or `u32::MAX` for an unknown opcode or a
/// caller that is not live.
const DENIED_RIGHT: u32 = 0x100;

/// The admission rules behind [`check_allowlist`]; `Err` carries the audit
/// detail of the refusal.
fn admission(syscall_id: usize, caller_id: usize) -> Result<(), u32> {
    let sc = ViSyscall::from(syscall_id);
    // `bench-counters` raw 504 borrows MemInfo's gate exactly: allowlist bit 56
    // and the `observe_memory` right. Without the feature 504 stays unknown.
    #[cfg(feature = "bench-counters")]
    let gate_sc = if syscall_id == RAW_GATE_AS.0 {
        RAW_GATE_AS.1
    } else {
        sc
    };
    #[cfg(not(feature = "bench-counters"))]
    let gate_sc = sc;
    let bit = gate_sc.allowlist_bit();
    // Raw opcodes outside `ViSyscall` carry their gate bit in the table
    // (`RAW_OPCODES`: 500/501/503 → bit 36; 100/110/111/502 always permitted).
    let raw = RAW_OPCODES.iter().find(|(id, _)| *id == syscall_id);
    let raw_bit: Option<u8> = raw.and_then(|(_, bit)| *bit);

    // Raw opcodes with a dedicated `map_syscall` fallback mapping. These are
    // intentionally absent from `ViSyscall` (Law 1: keeps experimental ids out
    // of the stable ABI) so they decode as `Unknown` — but they are NOT unknown:
    // 500/501/503 are gated by bit 36 below + the ZST BlockIoCap at the handler;
    // 502 and the legacy raw ops (3/100/110/111) predate the bitmap and stay
    // always-permitted, matching their pre-Phase-31b behavior.
    let known_raw = raw.is_some() || (cfg!(feature = "bench-counters") && syscall_id == 504);

    let Some(allowlist) = syscall_allowlist_for(caller_id) else {
        log::warn!(
            "[kernel] syscall opcode {} denied for non-live tid {}",
            syscall_id,
            caller_id
        );
        return Err(u32::MAX);
    };

    // Deny truly-unknown opcodes that land in the legacy inner-match fallback —
    // their allowlist_bit() returns None, so without this guard they bypass the
    // check. Exit (60) and Yield (104) are always permitted unconditionally.
    // Known-raw ids are exempt: blocking them here made every allowlist-declaring
    // cell lose raw block I/O (broke the VFS FAT32 mount silently since Phase 31b).
    // Every deny below logs: a silent dispatch-level denial cost a full day of
    // triage when the shell's missing `Read` bit bricked serial input with no
    // kernel output at all.
    if sc == ViSyscall::Unknown
        && !known_raw
        && !matches!(syscall_id, 60 | 104)
        && allowlist != u64::MAX
    {
        log::warn!(
            "[kernel] unknown opcode {} denied for tid {} (allowlist={:#018x})",
            syscall_id,
            caller_id,
            allowlist
        );
        return Err(u32::MAX);
    }
    if let Some(b) = bit {
        if allowlist & (1u64 << b) == 0 {
            log::warn!(
                "[kernel] syscall {:?} (bit {}) denied for tid {} (allowlist={:#018x})",
                sc,
                b,
                caller_id,
                allowlist
            );
            return Err(u32::from(b));
        }
    }
    if let Some(b) = raw_bit {
        if allowlist & (1u64 << b) == 0 {
            log::warn!(
                "[kernel] raw opcode {} denied for tid {} (no bit {})",
                syscall_id,
                caller_id,
                b
            );
            return Err(u32::from(b));
        }
    }
    if !observe_right_held(gate_sc, caller_id) {
        log::warn!(
            "[kernel] syscall {:?} denied for tid {}: edge withholds the S2-T observation right",
            sc,
            caller_id
        );
        return Err(DENIED_RIGHT | gate_sc.allowlist_bit().map_or(0xFF, u32::from));
    }
    Ok(())
}

#[cfg(not(target_arch = "riscv32"))]
#[no_mangle]
#[allow(non_snake_case)] // ABI name required by the HAL trap vector — cannot be snake_case
pub extern "Rust" fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame) {
    #[cfg(all(feature = "x86-idt-cpl3-test", target_arch = "x86_64"))]
    if crate::hal::idt::handle_cpl3_probe_syscall(frame) {
        return;
    }

    #[cfg(feature = "bench-counters")]
    {
        crate::bench_counters::syscall();
        // The SYSCALL stub parks the interrupted CR3 in `scause` and, exactly
        // when it differs from a recorded kernel CR3, writes CR3 on entry and
        // again on exit. Mirror that test here (synthetic in-kernel frames
        // carry 0 and are not counted).
        #[cfg(target_arch = "x86_64")]
        {
            let kernel_cr3 = crate::hal::domain::kernel_cr3();
            if frame.scause != 0 && kernel_cr3 != 0 && frame.scause != kernel_cr3 {
                crate::bench_counters::domain_syscall_cr3();
            }
        }
    }

    let syscall_id = frame.regs[17];

    // Watchdog progress signal: a syscall proves the caller is making progress
    // (ViCell cells are poll-based — try_recv/yield every loop iteration), so
    // reset its CPU-monopoly counter.
    {
        let hart_id = crate::sched::hart_local::current_hart_id();
        let cid = crate::sched::hart_local::ready::current_task_id_for(hart_id);
        if cid > 0 {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&cid) {
                    t.sched_stats.run_ticks = 0;
                    t.sched_stats.rt_overrun_warned = false;
                }
            }
        }
    }

    let a0 = frame.regs[10];
    let a1 = frame.regs[11];
    let a2 = frame.regs[12];
    let a3 = frame.regs[13];

    let Some(syscall) = map_syscall(syscall_id, a0, a1, a2, a3) else {
        frame.regs[10] = usize::MAX;
        return;
    };

    let caller_id = super::current_task_id();

    if check_allowlist(syscall_id, caller_id).is_err() {
        frame.regs[10] = usize::MAX;
        return;
    }

    // sstatus.SUM (bit 18) enables S-mode access to user pages
    // for the duration of handle_syscall. Disabled immediately after to prevent
    // inadvertent user-page reads on subsequent kernel faults.
    #[cfg(target_arch = "riscv64")]
    crate::arch::user_access::syscall_open();

    let supports_typed_oom = supports_typed_oom(&syscall);
    let typed_errors = has_typed_errors(&syscall);
    let pcie_function = is_pcie_function_call(&syscall);
    let power_call = matches!(syscall, Syscall::SystemPower { .. });
    let dma_export_call = is_dma_export_call(&syscall);
    let result = dispatch_admitted(caller_id, syscall);

    #[cfg(target_arch = "riscv64")]
    crate::arch::user_access::syscall_close();

    frame.regs[10] = if pcie_function {
        encode_pcie_function_result(result)
    } else if power_call {
        encode_system_power_result(result)
    } else if dma_export_call {
        encode_dma_export_result(result)
    } else if typed_errors {
        encode_ipc_result(result)
    } else {
        encode_syscall_result(result, usize::MAX, supports_typed_oom)
    };
}

#[cfg(not(target_arch = "riscv32"))]
const _: crate::hal::SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(target_arch = "riscv32")]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "Rust" fn ViCell_syscall_dispatch(frame: &mut crate::hal::arch::ViTrapFrame) {
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::syscall();
    // Promote u32 register slots to usize (= u32 on rv32) for arch-agnostic helpers.
    let syscall_id = frame.regs[17] as usize;

    // Watchdog: syscall proves the cell is making forward progress.
    {
        let hart_id = crate::sched::hart_local::current_hart_id();
        let cid = crate::sched::hart_local::ready::current_task_id_for(hart_id);
        if cid > 0 {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&cid) {
                    t.sched_stats.run_ticks = 0;
                    t.sched_stats.rt_overrun_warned = false;
                }
            }
        }
    }

    let a0 = frame.regs[10] as usize;
    let a1 = frame.regs[11] as usize;
    let a2 = frame.regs[12] as usize;
    let a3 = frame.regs[13] as usize;

    let Some(syscall) = map_syscall(syscall_id, a0, a1, a2, a3) else {
        frame.regs[10] = u32::MAX;
        return;
    };

    let caller_id = super::current_task_id();

    if check_allowlist(syscall_id, caller_id).is_err() {
        frame.regs[10] = u32::MAX;
        return;
    }

    // sstatus.SUM (bit 18) — same bit position on RV32 and RV64 per the RISC-V
    // Privileged Spec §4.1.1. Disabled after handle_syscall.
    crate::arch::user_access::syscall_open();

    let supports_typed_oom = supports_typed_oom(&syscall);
    let typed_errors = has_typed_errors(&syscall);
    let result = dispatch_admitted(caller_id, syscall);

    crate::arch::user_access::syscall_close();

    frame.regs[10] = if typed_errors {
        encode_ipc_result(result) as u32
    } else {
        encode_syscall_result(result, u32::MAX as usize, supports_typed_oom) as u32
    };
}

#[cfg(target_arch = "riscv32")]
const _: crate::hal::SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(test)]
mod golden_tests;

#[cfg(test)]
mod grant_owner_tests;

#[cfg(test)]
mod grant_lease_tests;

#[cfg(test)]
#[path = "grant-row-tests.rs"]
mod grant_row_tests;

#[cfg(test)]
mod tests;
