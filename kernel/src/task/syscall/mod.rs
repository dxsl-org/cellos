//! IPC System Calls (Inspired by Tock OS)
//!
//! This module defines the interface between "Cells/Silos" and the Kernel.
//! See [docs/architecture/03-driver-strategy.md] for the full rationale.

use super::tcb::TaskState;
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use alloc::string::String;
/// Only the private-root lifecycle helpers below name `Arc`; every other
/// target/feature combination has nothing to import.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
use alloc::sync::Arc;
use alloc::vec::Vec;
use api::syscall::ViSpawnArgs;
// use log::info;
use super::copy_glue::TaskCopyView;
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

static SHM_HANDLES: Spinlock<BTreeMap<usize, ShmRecord>> = Spinlock::new(BTreeMap::new());

static DMA_PUBLICATIONS: Spinlock<[usize; MAX_IN_FLIGHT_DMA_PUBLICATIONS]> =
    Spinlock::new([0; MAX_IN_FLIGHT_DMA_PUBLICATIONS]);

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

static PAGE_GRANT_TABLE: Spinlock<Option<BTreeMap<usize, PageGrant>>> = Spinlock::new(None);

#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
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
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
type DomainRoot = alloc::sync::Arc<crate::memory::address_space::AddressSpace>;
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
)))]
type DomainRoot = ();

/// Map a domain owner's freshly allocated backing into its own root, RW+NX,
/// undoing every page already mapped if one fails.
///
/// The frames stay supervisor-only in the SAS/global root: this path never
/// publishes USER access there, so a SAS cell cannot reach a domain owner's
/// backing even if the grant later collapses.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn map_domain_owner_pages(
    space: &DomainRoot,
    base: usize,
    n_pages: usize,
) -> Result<(), crate::memory::address_space::AddressSpaceError> {
    const PAGE_SIZE: usize = 4096;
    let rights = crate::memory::paging::Flags::from_bits(
        crate::memory::paging::Flags::READ | crate::memory::paging::Flags::WRITE,
    );
    let mut mapped = 0usize;
    for index in 0..n_pages {
        let va = base + index * PAGE_SIZE;
        match space.map_grant_page(va, va, rights) {
            Ok(()) => {
                // The owner's own hart writes this PTE and then uses the
                // pointer, so it must invalidate the page locally first.
                crate::hal::paging::flush_tlb_page(va);
                mapped += 1;
            }
            Err(error) => {
                for undo in 0..mapped {
                    let _ = space.unmap_grant_page(base + undo * PAGE_SIZE);
                }
                // The unmaps above queued this tag's invalidation if it could not
                // be confirmed; the deferred-release reaper completes it, so the
                // rollback only re-probes once instead of waiting a budget.
                let _ = crate::memory::tlb_shootdown::confirm_tag_invalidation(space.asid());
                return Err(error);
            }
        }
    }
    Ok(())
}

/// Allocate `n_pages` contiguous frames, zero them through the supervisor
/// identity map, and leave the SAS/global root untouched.
///
/// Domain owners must not have their backing published USER in the shared root
/// (the phase-01 leak). The boot identity map already covers every usable
/// frame supervisor RWX, so zeroing needs no remap at all; the caller then maps
/// the owner's own private root over the returned base.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn alloc_grant_pages_supervisor_only(n_pages: usize) -> Option<usize> {
    use crate::memory::frame::FRAME_ALLOCATOR;
    const PAGE_SIZE: usize = 4096;
    let paddr = {
        let mut guard = FRAME_ALLOCATOR.lock();
        guard
            .as_mut()
            .and_then(|alloc| alloc.allocate_contiguous(n_pages))?
    };
    // SAFETY: every usable frame is identity-mapped supervisor RWX in the
    // current (kernel) root, and the syscall runs on that root.
    unsafe {
        core::ptr::write_bytes(paddr as *mut u8, 0, n_pages * PAGE_SIZE);
    }
    Some(paddr)
}

/// The lifecycle record behind a table row, where this target can hold one.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
type DomainRecord = Arc<super::domain_grant::DomainGrant>;

/// On every other target a row can never carry a lifecycle record:
/// [`clone_domain_record`] is the only producer and always yields `None`, so the
/// stubs below are unreachable rather than a second implementation.
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
type DomainRecord = ();

/// A record the lifecycle cannot drain: it names a private-root endpoint but is
/// not carried by a managed domain record. Pre-gate residue, or a target where
/// the lifecycle is not implemented at all.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
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

static REG_GRANT_TABLE: Spinlock<Option<BTreeMap<usize, RegGrant>>> = Spinlock::new(None);

// ── Shared allocation/deallocation helpers ────────────────────────────────────

// ── Grant Reaper ──────────────────────────────────────────────────────────────

/// Complete one owner-reclaimed domain row: revoke, then either release the
/// frames or leave an ownerless `Revoking` row for [`sweep_deferred_domain_grants`].
///
/// An unacknowledged invalidation never frees a frame. The row loses its owner
/// so no caller can free it out from under the retry.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn release_deferred_domain_row(
    key: usize,
    record: DomainRecord,
    base: usize,
    size: usize,
    registered: bool,
) {
    let pages = grant_pages_for_size(size);
    if revoke_domain_record(&record, base, "task-exit") {
        if registered {
            let mut tbl = reg_grant_table_lock().lock();
            tbl.as_mut().and_then(|map| map.remove(&key));
        } else {
            let mut tbl = grant_table_lock().lock();
            tbl.as_mut().and_then(|map| map.remove(&key));
        }
        if !withhold_or_free(base, pages) {
            free_grant_pages(base, pages);
        }
        return;
    }
    if registered {
        let mut tbl = reg_grant_table_lock().lock();
        if let Some(grant) = tbl.as_mut().and_then(|map| map.get_mut(&key)) {
            grant.owner = 0;
            grant.owner_cell = CellId(0);
            grant.owner_generation = 0;
            grant.shared_to = None;
        }
    } else {
        let mut tbl = grant_table_lock().lock();
        if let Some(grant) = tbl.as_mut().and_then(|map| map.get_mut(&key)) {
            grant.owner = 0;
            grant.owner_cell = CellId(0);
            grant.owner_generation = 0;
            grant.shared_to = None;
        }
    }
}

#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
fn release_deferred_domain_row(
    _key: usize,
    _record: DomainRecord,
    _base: usize,
    _size: usize,
    _registered: bool,
) {
}

/// Pages leaked for the life of the boot because the quarantine was full.
static LEAKED_PAGES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

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
            error_sentinel - api::syscall::result_sentinel::OUT_OF_MEMORY
        }
        // A broken pipe is a distinct, actionable condition for a stream write, so
        // it gets its own sentinel rather than collapsing into the generic error.
        Err(SyscallError::BrokenPipe) => {
            error_sentinel - api::syscall::result_sentinel::BROKEN_PIPE
        }
        Err(_) => error_sentinel - api::syscall::result_sentinel::GENERIC,
    }
}

/// Syscalls whose callers decode the typed out-of-memory sentinel
/// (`error_sentinel - 1`); every other syscall keeps the single generic error.
fn supports_typed_oom(syscall: &Syscall) -> bool {
    syscall_to_vi(syscall).is_some_and(api::syscall::ViSyscall::typed_oom)
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
        crate::board::selected_x86_64_soc()
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

#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn sas_record_end(
    sched: &super::scheduler::Scheduler,
    page_grants: Option<&BTreeMap<usize, PageGrant>>,
    registered_grants: Option<&BTreeMap<usize, RegGrant>>,
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
            .filter(|grant| {
                grant.owner == caller_id
                    && grant.owner_cell == cell_id
                    && grant.owner_generation == generation
            })
            .filter_map(|grant| record_end_containing(grant.base, grant.size, ptr))
            .max()
    });
    let registered_grant_end = registered_grants.and_then(|grants| {
        grants
            .values()
            .filter(|grant| {
                grant.owner == caller_id
                    && grant.owner_cell == cell_id
                    && grant.owner_generation == generation
            })
            .filter_map(|grant| record_end_containing(grant.base, grant.size, ptr))
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
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn sas_caller_owned_span(
    sched: &super::scheduler::Scheduler,
    page_grants: Option<&BTreeMap<usize, PageGrant>>,
    registered_grants: Option<&BTreeMap<usize, RegGrant>>,
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
        #[cfg(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )
        ))]
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
        #[cfg(not(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )
        )))]
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
/// then record `shared_to`.
///
/// A redundant share of the same tuple is a no-op; any change republishes after
/// revoking the old receiver PTE and shooting it down. Unsupported rights
/// (write-only) and a non-live or non-domain target are refused with the
/// established share failure.
// reason: the eight values are the share's exact inputs — row identity (owner,
// base, domain, shared_to) plus the receiver binding (caller, target tid/root,
// perm). Grouping them into a struct would only move the same list.
#[allow(clippy::too_many_arguments)]
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn domain_grant_share_row(
    owner: usize,
    base: usize,
    domain: Option<&Arc<super::domain_grant::DomainGrant>>,
    shared_to: &mut Option<(usize, GrantPerm)>,
    caller_id: usize,
    target_tid: usize,
    target_root: &Arc<crate::memory::address_space::AddressSpace>,
    perm: GrantPerm,
) -> Result<(), SyscallError> {
    let Some(record) = domain else {
        return Err(SyscallError::PermissionDenied);
    };
    if caller_id != owner || record.state() != super::domain_grant::DomainGrantState::Live {
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
    *shared_to = Some((target_tid, perm));
    Ok(())
}

/// Resolve one grant row for a capable domain receiver.
///
/// Returns the resolved base on success and the established `usize::MAX`
/// "not authorized" sentinel for every refusal: an unresolvable or non-domain
/// receiver, a SAS-owned record, an unresolvable or dying owner, a record that
/// is not `Live`, missing or unsupported rights, a partial receiver mapping, or
/// an unacknowledged rollback invalidation.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
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
    if !record.owner_is_live() || record.state() != super::domain_grant::DomainGrantState::Live {
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
    if shared_tid != caller_id {
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
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
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
                grant.owner,
                grant.base,
                grant.size,
                grant.shared_to,
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
        grant.owner,
        grant.base,
        grant.size,
        grant.shared_to,
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
/// [`opcode_of`] (`libs/api/abi/syscalls.toml`). `None` for raw opcodes
/// outside `ViSyscall` (500–503, 100/110/111, 502, bench 504) and for kernel
/// variants with no opcode. Total over every api row, unlike the hand-written
/// map it replaces (architecture review H-5).
fn syscall_to_vi(syscall: &Syscall) -> Option<api::syscall::ViSyscall> {
    match api::syscall::ViSyscall::from(opcode_of(syscall)) {
        api::syscall::ViSyscall::Unknown => None,
        vi => Some(vi),
    }
}

// `opcode_of` and `RAW_OPCODES`, generated by `kernel/build.rs`.
include!(concat!(env!("OUT_DIR"), "/syscall_opcodes.rs"));

mod incarnation_tids {
    /// Root plus the per-Cell thread cap.
    const CAP: usize = super::super::scheduler::MAX_THREADS_PER_CELL + 1;

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
    #[cfg(cellos_p01_trace)]
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
        #[cfg(cellos_p01_trace)]
        &mut trace,
    );
    #[cfg(cellos_p01_trace)]
    {
        crate::p01_measurement::outcome(trace, result);
        crate::p01_measurement::safe_point();
    }
    result
}

fn handle_syscall_inner(
    caller_id: usize,
    syscall: Syscall,
    #[cfg(cellos_p01_trace)] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    if syscall_allowlist_for(caller_id).is_none() {
        log::warn!("[kernel] syscall denied for non-live tid {}", caller_id);
        return Err(SyscallError::PermissionDenied);
    }

    let _staged_spawn_argv_cleanup = staged_spawn_argv_cleanup(&syscall, caller_id);
    #[cfg(cellos_p01_trace)]
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
            | Syscall::ShmMap { .. }
            | Syscall::Grant { .. }
            | Syscall::Map { .. } => Some(3),
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
        } => {
            const MAX: usize = api::cell_session::MAX_REQUEST_LEN;
            if !(api::cell_session::HEADER_LEN..=MAX).contains(&request_len)
                || result_capacity < api::cell_session::HEADER_LEN
            {
                return Err(SyscallError::InvalidInput);
            }
            let request_end = request_ptr
                .checked_add(request_len)
                .ok_or(SyscallError::InvalidInput)?;
            let result_end = result_ptr
                .checked_add(result_capacity)
                .ok_or(SyscallError::InvalidInput)?;
            if request_ptr < result_end && result_ptr < request_end {
                return Err(SyscallError::InvalidInput);
            }
            validate_user_buf(request_ptr, request_len, MAX)?;
            validate_user_buf(result_ptr, result_capacity, MAX_USER_BUF)?;
            preflight_user_output(caller_id, result_ptr, result_capacity)?;
            let view = caller_copy_view(caller_id)?;
            let mut request = [0u8; MAX];
            let _reply_pin =
                SessionOutputPin::reserve(caller_id, result_ptr, result_capacity, &view)?;
            view.read_into(request_ptr, &mut request[..request_len])
                .map_err(|_| SyscallError::InvalidInput)?;
            let header = api::cell_session::RequestHeaderV1::decode(&request[..request_len]);
            let mut result = [0u8; 1536];
            if let Ok(header) = header {
                let required = super::cell_session::required_result(
                    header.selector,
                    &request[api::cell_session::HEADER_LEN..request_len],
                );
                if result_capacity < required {
                    api::cell_session::ResultHeaderV1 {
                        selector: header.selector,
                        status: api::cell_session::Status::BufferTooSmall,
                        total_len: required as u32,
                        request_id: header.request_id,
                    }
                    .encode(&mut result)
                    .map_err(|_| SyscallError::InvalidInput)?;
                    view.write_bytes(result_ptr, &result[..api::cell_session::HEADER_LEN])
                        .map_err(|_| SyscallError::InvalidInput)?;
                    return Ok(0);
                }
            }
            let len =
                super::cell_session::dispatch(caller_id, &request[..request_len], &mut result);
            view.write_bytes(result_ptr, &result[..len])
                .map_err(|_| SyscallError::InvalidInput)?;
            // Only a reply that reached the caller may be acknowledged.
            super::cell_session::delivered(caller_id, &request[..request_len]);
            Ok(0)
        }
        // --- Hubris ABI Implementation ---
        Syscall::IpcSubmit { target, ptr, len } => {
            super::async_ipc::submit(caller_id, target, ptr, len).map_err(ipc_error)
        }
        Syscall::IpcTake {
            token,
            ptr,
            len,
            status_ptr,
        } => super::async_ipc::take(caller_id, token, ptr, len, status_ptr)
            .map(usize::from)
            .map_err(ipc_error),
        Syscall::IpcWait { ticks } => super::async_ipc::wait(caller_id, ticks)
            .map(usize::from)
            .map_err(ipc_error),
        Syscall::IpcCancel { token } => super::async_ipc::cancel(caller_id, token)
            .map(|()| 0)
            .map_err(ipc_error),
        Syscall::IpcCurrent => Ok(super::async_ipc::current(caller_id)),
        Syscall::IpcReply { token, ptr, len } => {
            // An explicit reply is the same holder-quiescence boundary as Send.
            // A stale/cancelled token may already be gone from the client, but
            // must still release only this exact current VFS request's lease.
            let vfs_release = if token != 0 && super::async_ipc::current(caller_id) == token {
                match current_vfs_grant_lookup(caller_id) {
                    VfsGrantLookup::Active(context) => Some(context),
                    VfsGrantLookup::NotVfs | VfsGrantLookup::MissingContext => None,
                }
            } else {
                None
            };
            let out = super::async_ipc::reply(caller_id, token, ptr, len)
                .map(|()| 0)
                .map_err(ipc_error);
            if let Some(context) = vfs_release {
                finish_vfs_send_release(caller_id, context.grant_owner, Some(context));
            }
            out
        }
        Syscall::Send {
            target,
            msg_ptr,
            msg_len,
        } => {
            crate::audit::log_event(
                crate::audit::AuditEvent::IpcSend,
                &crate::audit::encode_u32x2(caller_id as u32, target as u32),
            );
            let vfs_release = match current_vfs_grant_lookup(caller_id) {
                VfsGrantLookup::Active(context) => Some(context),
                VfsGrantLookup::NotVfs | VfsGrantLookup::MissingContext => None,
            };
            if let Some(outcome) =
                super::async_ipc::reply_current(caller_id, target, msg_ptr, msg_len)
            {
                let out = outcome.map(|()| 0).map_err(ipc_error);
                finish_vfs_send_release(caller_id, target, vfs_release);
                return out;
            }
            let res = super::ipc_send(caller_id, target, msg_ptr, msg_len);
            let out = match res {
                Ok(0) => Ok(0),
                Ok(1) => {
                    super::yield_cpu(); // Blocked
                    if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                        Ok(sched
                            .tasks
                            .get(&caller_id)
                            .and_then(|t| t.reply_value)
                            .unwrap_or(0))
                    } else {
                        Ok(0)
                    }
                }
                Err(super::IpcSendError::Backpressure) => Err(SyscallError::TryAgain),
                Err(super::IpcSendError::TargetGone) => Err(SyscallError::InvalidCommand),
                _ => Ok(0),
            };
            finish_vfs_send_release(caller_id, target, vfs_release);
            out
        }
        Syscall::TrySend {
            target,
            msg_ptr,
            msg_len,
        } => {
            // Non-blocking: deliver if target in Recv, else drop. Never blocks.
            // Target 0 is resolved to caller_id for self-wake without an extra tid lookup.
            let target_id = if target == 0 { caller_id } else { target };
            match super::ipc_try_send(caller_id, target_id, msg_ptr, msg_len) {
                Ok(()) => Ok(0), // delivered (or queued for input service / same-cell wake)
                Err(()) => Ok(usize::MAX), // dropped (target not ready or gone)
            }
        }
        Syscall::Recv {
            mask,
            buf_ptr,
            buf_len,
            attest_caller,
        } => {
            // Identity trailer, when requested, is written at each delivery point
            // AFTER the payload copy and AFTER the scheduler lock is dropped —
            // `attested_identity_of` takes that lock itself.
            let attest = |sender_tid: usize| {
                if attest_caller {
                    write_caller_identity(caller_id, buf_ptr, buf_len, sender_tid);
                }
            };

            let death_info = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().and_then(|sched| {
                    let t = sched.tasks.get_mut(&caller_id)?;
                    let owner_death = super::Task::owner_death_matches_receive_mask(mask)
                        .then(|| t.pending_owner_deaths.first().copied())
                        .flatten()
                        .map(|(_, dead_tid, reason)| (true, dead_tid, reason));
                    let death = owner_death.or_else(|| {
                        t.pending_deaths
                            .first()
                            .copied()
                            .map(|(dead_tid, reason)| (false, dead_tid, reason))
                    });
                    death.map(|(is_owner, dead_tid, reason)| {
                        let caller_view = TaskCopyView::of(t);
                        (is_owner, dead_tid, reason, caller_view)
                    })
                })
            };

            if let Some((is_owner, dead_tid, reason, caller_view)) = death_info {
                if buf_len >= core::mem::size_of::<u64>() {
                    validate_user_buf(buf_ptr, core::mem::size_of::<u64>(), MAX_USER_BUF)?;
                    let reason_bytes = (reason as u64).to_ne_bytes();
                    caller_view
                        .write_bytes(buf_ptr, &reason_bytes)
                        .map_err(|_| SyscallError::InvalidInput)?;
                }
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    if let Some(t) = sched.tasks.get_mut(&caller_id) {
                        if is_owner {
                            if !t.pending_owner_deaths.is_empty() {
                                t.pending_owner_deaths.remove(0);
                            }
                        } else if !t.pending_deaths.is_empty() {
                            t.pending_deaths.remove(0);
                        }
                    }
                }
                attest(dead_tid);
                return Ok(dead_tid);
            }

            // Hot-swap pending-message drain — guaranteed delivery path.
            // Payload snapshot is fallible (never an infallible Vec::from);
            // wire records carry scalar identity and gate the sender wake.
            let pending_msg_info: Result<Option<_>, ()> = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().map_or(Ok(None), |sched| {
                    let Some(t) = sched.tasks.get_mut(&caller_id) else {
                        return Ok(None);
                    };
                    let Some(pos) = t
                        .pending_msgs
                        .iter()
                        .position(|m| mask == 0 || m.sender_tid == mask)
                    else {
                        return Ok(None);
                    };
                    let msg = &t.pending_msgs.as_slice()[pos];
                    let sender_tid = msg.sender_tid;
                    let wire_header = msg.wire_header();
                    // Allocation failure is a real error, not "no message":
                    // encoding it as None would block despite queued data.
                    // Stage only what fits the caller's buffer; the record is
                    // removed whole, so the rest would be copied for nothing.
                    let payload = msg.payload();
                    let data = MsgPayload::from_slice(&payload[..payload.len().min(buf_len)])?;
                    let caller_view = TaskCopyView::of(t);
                    let (sender_cell_id, sender_generation) = match wire_header {
                        Some(header) => (header.sender_cell_id, header.sender_generation),
                        None => sender_cell_context_in_sched(sched, sender_tid),
                    };
                    let info = (
                        pos,
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header,
                        caller_view,
                        data,
                    );
                    if let Some(header) = wire_header {
                        super::async_ipc::dispatch(sched, caller_id, header);
                    }
                    Ok(Some(info))
                })
            };
            let (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                caller_view,
                data,
            ) = match pending_msg_info {
                Err(()) => return Err(SyscallError::OutOfMemory),
                Ok(None) => {
                    // A receive masked to a peer that is already gone, with no
                    // message of its left to deliver, would park forever: the
                    // exit-time wake (scheduler `exit_task`) fired before this
                    // receive existed. Complete it as that death, reason 0 like
                    // NotifyOnExit's already-dead target.
                    if mask != 0 {
                        let peer_gone = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
                            sched.tasks.get(&mask).is_none_or(|peer| {
                                matches!(
                                    peer.state,
                                    super::tcb::TaskState::Retiring
                                        | super::tcb::TaskState::Terminated
                                )
                            })
                        });
                        if peer_gone {
                            if buf_len >= core::mem::size_of::<u64>() {
                                validate_user_buf(
                                    buf_ptr,
                                    core::mem::size_of::<u64>(),
                                    MAX_USER_BUF,
                                )?;
                                let view = caller_copy_view(caller_id)?;
                                view.write_bytes(buf_ptr, &0u64.to_ne_bytes())
                                    .map_err(|_| SyscallError::InvalidInput)?;
                            }
                            attest(mask);
                            return Ok(mask);
                        }
                    }
                    let res = super::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                    match res {
                        Ok(0) => {
                            super::yield_cpu();
                            // Peek the next event without removing it,
                            // copy payload, then commit the exact record.
                            let snap = {
                                let mut guard = super::SCHEDULER.lock();
                                guard
                                    .as_mut()
                                    .map_or(Ok(ResumeSnapshot::Wake { sender_tid: 0 }), |sched| {
                                        snapshot_resume_dispatched(sched, caller_id, mask)
                                    })
                            };
                            let snap = match snap {
                                Err(()) => return Err(SyscallError::OutOfMemory),
                                Ok(s) => s,
                            };
                            // Copy-out before any commit.
                            let sender = match &snap {
                                ResumeSnapshot::Death {
                                    sender_tid, reason, ..
                                } => {
                                    if buf_len >= core::mem::size_of::<u64>() {
                                        validate_user_buf(
                                            buf_ptr,
                                            core::mem::size_of::<u64>(),
                                            MAX_USER_BUF,
                                        )?;
                                        let reason_bytes = (*reason as u64).to_ne_bytes();
                                        let view = caller_copy_view(caller_id)?;
                                        view.write_bytes(buf_ptr, &reason_bytes)
                                            .map_err(|_| SyscallError::InvalidInput)?;
                                    }
                                    *sender_tid
                                }
                                ResumeSnapshot::Message {
                                    sender_tid,
                                    payload,
                                    ..
                                } => {
                                    let copy_len = core::cmp::min(payload.len(), buf_len);
                                    if copy_len > 0 {
                                        validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                                        let view = caller_copy_view(caller_id)?;
                                        view.write_bytes(buf_ptr, &payload[..copy_len])
                                            .map_err(|_| SyscallError::InvalidInput)?;
                                        #[cfg(feature = "bench-counters")]
                                        crate::bench_counters::ipc_bytes(copy_len);
                                    }
                                    *sender_tid
                                }
                                ResumeSnapshot::Wake { sender_tid } => *sender_tid,
                            };
                            // Copy succeeded: commit the exact record.
                            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                    commit_resume(task, &snap);
                                    if let ResumeSnapshot::Message {
                                        sender_tid,
                                        sender_cell_id,
                                        sender_generation,
                                        wire_header,
                                        ..
                                    } = &snap
                                    {
                                        task.set_received_request_context(
                                            *sender_tid,
                                            *sender_cell_id,
                                            *sender_generation,
                                            wire_header.map_or(0, |header| header.async_op),
                                        );
                                    }
                                }
                                if let ResumeSnapshot::Message {
                                    sender_tid,
                                    wire_header: Some(header),
                                    ..
                                } = snap
                                {
                                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                                }
                            }
                            attest(sender);
                            return Ok(sender);
                        }
                        Ok(id) => {
                            attest(id);
                            return Ok(id);
                        }
                        Err(_) => return Err(SyscallError::InvalidCommand),
                    }
                }
                Ok(Some(snapshot)) => snapshot,
            };
            let copy_len = core::cmp::min(data.len(), buf_len);
            if copy_len > 0 {
                validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                caller_view
                    .write_bytes(buf_ptr, &data[..copy_len])
                    .map_err(|_| SyscallError::InvalidInput)?;
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::ipc_bytes(copy_len);
            }
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    let _ = t.pending_msgs.remove_delivered(
                        pos,
                        sender_tid,
                        wire_header.map(|header| header.delivery_id),
                    );
                    t.set_received_request_context(
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header.map_or(0, |header| header.async_op),
                    );
                }
                if let Some(header) = wire_header {
                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                }
            }
            attest(sender_tid);
            Ok(sender_tid)
        }
        // ── Scatter/gather IPC ────────────────────────────────────────────────
        Syscall::SendGather {
            target,
            iovec_ptr,
            iovec_count,
        } => {
            const MAX_IOVEC: usize = 8;
            const IOVEC_ENTRY: usize = core::mem::size_of::<usize>() * 2;
            if iovec_count == 0 || iovec_count > MAX_IOVEC {
                return Err(SyscallError::InvalidInput);
            }
            let caller_view = caller_copy_view(caller_id)?;
            let mut iovec_entries = alloc::vec::Vec::with_capacity(iovec_count);
            let mut total = 0usize;
            for i in 0..iovec_count {
                let base = iovec_ptr + i * IOVEC_ENTRY;
                let mut entry_bytes = [0u8; IOVEC_ENTRY];
                caller_view
                    .read_into(base, &mut entry_bytes)
                    .map_err(|_| SyscallError::InvalidInput)?;
                let ptr = usize::from_ne_bytes(
                    entry_bytes[..core::mem::size_of::<usize>()]
                        .try_into()
                        .unwrap(),
                );
                let len = usize::from_ne_bytes(
                    entry_bytes[core::mem::size_of::<usize>()..]
                        .try_into()
                        .unwrap(),
                );
                validate_user_buf(ptr, len, MAX_USER_BUF)?;
                total = total.checked_add(len).ok_or(SyscallError::BufferTooSmall)?;
                if total > MAX_USER_BUF {
                    return Err(SyscallError::BufferTooSmall);
                }
                iovec_entries.push((ptr, len));
            }
            let mut gathered = alloc::vec::Vec::new();
            gathered
                .try_reserve_exact(total)
                .map_err(|_| SyscallError::OutOfMemory)?;
            gathered.resize(total, 0);
            let mut pos = 0;
            for (ptr, len) in iovec_entries {
                if len > 0 {
                    caller_view
                        .read_into(ptr, &mut gathered[pos..pos + len])
                        .map_err(|_| SyscallError::InvalidInput)?;
                    pos += len;
                }
            }
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::ipc_bytes(total);
            super::ipc_post_nonblock(caller_id, target, &gathered)
                .map_err(|_| SyscallError::TryAgain)?;
            Ok(0)
        }
        Syscall::RecvScatter {
            mask,
            iovec_ptr,
            iovec_count,
        } => {
            const MAX_IOVEC: usize = 8;
            const IOVEC_ENTRY: usize = core::mem::size_of::<usize>() * 2;
            if iovec_count == 0 || iovec_count > MAX_IOVEC {
                return Err(SyscallError::InvalidInput);
            }
            let caller_view = caller_copy_view(caller_id)?;
            let mut iovec_entries = alloc::vec::Vec::with_capacity(iovec_count);
            let mut total = 0usize;
            for i in 0..iovec_count {
                let base = iovec_ptr + i * IOVEC_ENTRY;
                let mut entry_bytes = [0u8; IOVEC_ENTRY];
                caller_view
                    .read_into(base, &mut entry_bytes)
                    .map_err(|_| SyscallError::InvalidInput)?;
                let ptr = usize::from_ne_bytes(
                    entry_bytes[..core::mem::size_of::<usize>()]
                        .try_into()
                        .unwrap(),
                );
                let len = usize::from_ne_bytes(
                    entry_bytes[core::mem::size_of::<usize>()..]
                        .try_into()
                        .unwrap(),
                );
                validate_user_buf(ptr, len, MAX_USER_BUF)?;
                total = total.checked_add(len).ok_or(SyscallError::BufferTooSmall)?;
                if total > MAX_USER_BUF {
                    return Err(SyscallError::BufferTooSmall);
                }
                iovec_entries.push((ptr, len));
            }
            // Multi-range receive transaction: snapshot/stage the message
            // payload in kernel memory, then stage and commit all destination
            // ranges via the copy boundary's atomic scatter write.
            // No destination is touched and no message is consumed if ANY
            // iovec range is invalid, unmapped, or lacks write permissions.
            let drained: Result<Option<_>, ()> = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().map_or(Ok(None), |sched| {
                    let Some(t) = sched.tasks.get_mut(&caller_id) else {
                        return Ok(None);
                    };
                    let Some(pos) = t
                        .pending_msgs
                        .iter()
                        .position(|m| mask == 0 || m.sender_tid == mask)
                    else {
                        return Ok(None);
                    };
                    let record = &t.pending_msgs.as_slice()[pos];
                    let sender_tid = record.sender_tid;
                    let wire_header = record.wire_header();
                    let payload = record.payload();
                    // Stage only what the iovecs can take (see `Recv`).
                    let tmp = MsgPayload::from_slice(&payload[..payload.len().min(total)])?;
                    if let Some(header) = wire_header {
                        super::async_ipc::dispatch(sched, caller_id, header);
                    }
                    Ok(Some((pos, sender_tid, wire_header, tmp)))
                })
            };
            let (pos, sender_tid, wire_header, tmp) = match drained {
                Err(()) => return Err(SyscallError::OutOfMemory),
                Ok(None) => return Err(SyscallError::TryAgain),
                Ok(Some(snapshot)) => snapshot,
            };
            let copy_total = core::cmp::min(tmp.len(), total);
            caller_view
                .write_scatter(&iovec_entries, &tmp[..copy_total])
                .map_err(|_| SyscallError::InvalidInput)?;
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::ipc_bytes(copy_total);
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    let _ = t.pending_msgs.remove_delivered(
                        pos,
                        sender_tid,
                        wire_header.map(|header| header.delivery_id),
                    );
                    if let Some(header) = wire_header {
                        // Commit caller context together with removal so a
                        // reply after scatter carries the delivered sender's
                        // identity and request generation.
                        t.set_received_request_context(
                            sender_tid,
                            header.sender_cell_id,
                            header.sender_generation,
                            header.async_op,
                        );
                    }
                }
                if let Some(header) = wire_header {
                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                }
            }
            Ok(sender_tid)
        }
        Syscall::RecvTimeout {
            mask,
            buf_ptr,
            buf_len,
            deadline,
        } => {
            // Queued death records first, exactly as the plain `Recv` arm delivers
            // them: a supervised loop that polls with a deadline (a supervisor with
            // backoff timers) must still see its children exit.
            if let Some(dead_tid) = take_queued_death(caller_id, mask, buf_ptr, buf_len)? {
                return Ok(dead_tid);
            }
            let mut vfs_context_drop = None;
            let pending_msg_info: Result<Option<_>, ()> = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().map_or(Ok(None), |sched| {
                    let Some(t) = sched.tasks.get_mut(&caller_id) else {
                        return Ok(None);
                    };
                    vfs_context_drop = t.begin_receive_context(mask);
                    let Some(pos) = t
                        .pending_msgs
                        .iter()
                        .position(|m| mask == 0 || m.sender_tid == mask)
                    else {
                        return Ok(None);
                    };
                    let record = &t.pending_msgs.as_slice()[pos];
                    let sender_tid = record.sender_tid;
                    let wire_header = record.wire_header();
                    // Fallible snapshot: allocation failure must not be
                    // mistaken for "no message" (the receiver would block
                    // despite queued data).
                    let payload = record.payload();
                    // Stage only what fits the caller's buffer (see `Recv`).
                    let data = MsgPayload::from_slice(&payload[..payload.len().min(buf_len)])?;
                    let caller_view = TaskCopyView::of(t);
                    let (sender_cell_id, sender_generation) = match wire_header {
                        Some(header) => (header.sender_cell_id, header.sender_generation),
                        None => sender_cell_context_in_sched(sched, sender_tid),
                    };
                    let info = (
                        pos,
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header,
                        caller_view,
                        data,
                    );
                    if let Some(header) = wire_header {
                        super::async_ipc::dispatch(sched, caller_id, header);
                    }
                    Ok(Some(info))
                })
            };
            finish_vfs_context_drop(caller_id, vfs_context_drop);

            let (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                caller_view,
                data,
            ) = match pending_msg_info {
                Err(()) => return Err(SyscallError::OutOfMemory),
                Ok(None) => {
                    // Fast path: check for a pending message immediately.
                    let res = super::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                    match res {
                        Ok(0) => {
                            // Blocked with deadline:None — install the absolute deadline.
                            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                    if let super::tcb::TaskState::Recv {
                                        deadline: ref mut d,
                                        ..
                                    } = task.state
                                    {
                                        *d = Some(deadline);
                                    }
                                }
                            }
                            // Yield so the scheduler runs other tasks and can fire the timeout.
                            super::yield_cpu();
                            let snap = {
                                let mut guard = super::SCHEDULER.lock();
                                guard
                                    .as_mut()
                                    .map_or(Ok(ResumeSnapshot::Wake { sender_tid: 0 }), |sched| {
                                        snapshot_resume_dispatched(sched, caller_id, mask)
                                    })
                            };
                            let snap = match snap {
                                Err(()) => return Err(SyscallError::OutOfMemory),
                                Ok(s) => s,
                            };
                            let sender = match &snap {
                                ResumeSnapshot::Death {
                                    sender_tid, reason, ..
                                } => {
                                    if buf_len >= core::mem::size_of::<u64>() {
                                        validate_user_buf(
                                            buf_ptr,
                                            core::mem::size_of::<u64>(),
                                            MAX_USER_BUF,
                                        )?;
                                        let reason_bytes = (*reason as u64).to_ne_bytes();
                                        let view = caller_copy_view(caller_id)?;
                                        view.write_bytes(buf_ptr, &reason_bytes)
                                            .map_err(|_| SyscallError::InvalidInput)?;
                                    }
                                    *sender_tid
                                }
                                ResumeSnapshot::Message {
                                    sender_tid,
                                    payload,
                                    ..
                                } => {
                                    let copy_len = core::cmp::min(payload.len(), buf_len);
                                    if copy_len > 0 {
                                        validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                                        let view = caller_copy_view(caller_id)?;
                                        view.write_bytes(buf_ptr, &payload[..copy_len])
                                            .map_err(|_| SyscallError::InvalidInput)?;
                                        #[cfg(feature = "bench-counters")]
                                        crate::bench_counters::ipc_bytes(copy_len);
                                    }
                                    *sender_tid
                                }
                                ResumeSnapshot::Wake { sender_tid } => *sender_tid,
                            };
                            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                    commit_resume(task, &snap);
                                    if let ResumeSnapshot::Message {
                                        sender_tid,
                                        sender_cell_id,
                                        sender_generation,
                                        wire_header,
                                        ..
                                    } = &snap
                                    {
                                        task.set_received_request_context(
                                            *sender_tid,
                                            *sender_cell_id,
                                            *sender_generation,
                                            wire_header.map_or(0, |header| header.async_op),
                                        );
                                    }
                                }
                                if let ResumeSnapshot::Message {
                                    sender_tid,
                                    wire_header: Some(header),
                                    ..
                                } = snap
                                {
                                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                                }
                            }
                            return Ok(sender);
                        }
                        Ok(id) => return Ok(id),
                        Err(_) => return Err(SyscallError::InvalidCommand),
                    }
                }
                Ok(Some(snapshot)) => snapshot,
            };
            let copy_len = core::cmp::min(data.len(), buf_len);
            if copy_len > 0 {
                validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                caller_view
                    .write_bytes(buf_ptr, &data[..copy_len])
                    .map_err(|_| SyscallError::InvalidInput)?;
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::ipc_bytes(copy_len);
            }
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    let _ = t.pending_msgs.remove_delivered(
                        pos,
                        sender_tid,
                        wire_header.map(|header| header.delivery_id),
                    );
                    t.set_received_request_context(
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header.map_or(0, |header| header.async_op),
                    );
                }
                if let Some(header) = wire_header {
                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                }
            }
            Ok(sender_tid)
        }
        Syscall::TryRecv {
            mask,
            buf_ptr,
            buf_len,
            attest_caller,
        } => {
            // A queued owner death is delivered first, exactly as `Recv` does
            // for a wildcard receive: reason at the start of the buffer, zero
            // trailer for the dead (unattributable) root, dead root TID returned.
            // `NotifyOnExit` deaths stay out of `TryRecv` on purpose.
            let owner_death = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().and_then(|sched| {
                    let t = sched.tasks.get_mut(&caller_id)?;
                    let (token, root_tid, reason) = peek_owner_death_for_try_recv(t, mask)?;
                    Some((token, root_tid, reason, TaskCopyView::of(t)))
                })
            };
            if let Some((token, root_tid, reason, caller_view)) = owner_death {
                // Validate and copy before removing: a failed copy leaves the
                // event queued, the same two-phase order as `Recv`.
                if buf_len >= core::mem::size_of::<u64>() {
                    validate_user_buf(buf_ptr, core::mem::size_of::<u64>(), MAX_USER_BUF)?;
                    caller_view
                        .write_bytes(buf_ptr, &(reason as u64).to_ne_bytes())
                        .map_err(|_| SyscallError::InvalidInput)?;
                }
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    if let Some(t) = sched.tasks.get_mut(&caller_id) {
                        remove_owner_death(t, token);
                    }
                }
                if attest_caller {
                    write_caller_identity(caller_id, buf_ptr, buf_len, root_tid);
                }
                return Ok(root_tid);
            }
            let mut vfs_context_drop = None;
            let pending_msg_info: Result<Option<_>, ()> = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().map_or(Ok(None), |sched| {
                    let Some(t) = sched.tasks.get_mut(&caller_id) else {
                        return Ok(None);
                    };
                    vfs_context_drop = t.begin_receive_context(mask);
                    let Some(pos) = t
                        .pending_msgs
                        .iter()
                        .position(|m| mask == 0 || m.sender_tid == mask)
                    else {
                        return Ok(None);
                    };
                    let msg = &t.pending_msgs.as_slice()[pos];
                    let sender_tid = msg.sender_tid;
                    let wire_header = msg.wire_header();
                    let delivery_id = wire_header.as_ref().map(|h| h.delivery_id);
                    // Extract cell context before dropping t to avoid
                    // holding a mutable borrow of sched.tasks while calling
                    // sender_cell_context_in_sched (which borrows sched immutably).
                    let inline_cell_context = wire_header
                        .as_ref()
                        .map(|h| (h.sender_cell_id, h.sender_generation));
                    let caller_view = TaskCopyView::of(t);
                    let payload = msg.payload();
                    // Stage only what fits the caller's buffer (see `Recv`).
                    let data = MsgPayload::from_slice(&payload[..payload.len().min(buf_len)])?;
                    // t is no longer borrowed here.
                    let (sender_cell_id, sender_generation) = inline_cell_context
                        .unwrap_or_else(|| sender_cell_context_in_sched(sched, sender_tid));
                    let info = (
                        pos,
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header,
                        delivery_id,
                        caller_view,
                        data,
                    );
                    if let Some(header) = wire_header {
                        super::async_ipc::dispatch(sched, caller_id, header);
                    }
                    Ok(Some(info))
                })
            };
            finish_vfs_context_drop(caller_id, vfs_context_drop);

            if let Some((
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                delivery_id,
                caller_view,
                data,
            )) = match pending_msg_info {
                Err(()) => return Err(SyscallError::OutOfMemory),
                Ok(v) => v,
            } {
                let copy_len = core::cmp::min(data.len(), buf_len);
                if copy_len > 0 {
                    validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                    caller_view
                        .write_bytes(buf_ptr, &data[..copy_len])
                        .map_err(|_| SyscallError::InvalidInput)?;
                    #[cfg(feature = "bench-counters")]
                    crate::bench_counters::ipc_bytes(copy_len);
                }
                if attest_caller {
                    write_caller_identity(caller_id, buf_ptr, buf_len, sender_tid);
                }
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    if let Some(t) = sched.tasks.get_mut(&caller_id) {
                        let _ = t
                            .pending_msgs
                            .remove_delivered(pos, sender_tid, delivery_id);
                        t.set_received_request_context(
                            sender_tid,
                            sender_cell_id,
                            sender_generation,
                            wire_header.map_or(0, |header| header.async_op),
                        );
                    }
                    if let Some(header) = wire_header {
                        super::wake_sender_token(sched, sender_tid, caller_id, header);
                    }
                }
                return Ok(sender_tid);
            }

            // Non-blocking Recv (scan Sending tasks)
            let res = super::ipc_try_recv(caller_id, mask, buf_ptr, buf_len);
            match res {
                Ok(id) => {
                    if attest_caller && id > 0 {
                        write_caller_identity(caller_id, buf_ptr, buf_len, id);
                    }
                    Ok(id)
                }
                Err(_) => Err(SyscallError::InvalidCommand),
            }
        }
        Syscall::Spawn { entry, arg } => {
            // Build and fully inherit the child under one scheduler lock before
            // publication. A runnable thread must never observe Task defaults
            // (SAS, permissive allowlist, or zero PKU state) between construction
            // and its creator's authority transfer.
            match super::spawn_with_arg(caller_id, "thread", entry, arg) {
                Ok(tid) => Ok(tid),
                Err(ViError::OutOfMemory) => Err(SyscallError::TryAgain),
                Err(_) => Err(SyscallError::Unknown),
            }
        }
        Syscall::SetTlsBase { base } => {
            // Self-only by construction: the ABI has no target task parameter, so
            // this can only ever change the caller's own thread pointer, and the
            // base carries no authority — the kernel never dereferences it. A bad
            // value is contained by the cell's own tier (LBI on Tier 1, an MMU
            // fault on Tier 2).
            //
            // The previous value is returned so a cell can read its base back
            // without touching the register, which the kernel owns (a direct user
            // write is overwritten on the next resume).
            if let Some(task) = super::SCHEDULER
                .lock()
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
            {
                let previous = task.tls_base;
                super::tls::set_base(task, base);
                Ok(previous)
            } else {
                Err(SyscallError::PermissionDenied)
            }
        }
        Syscall::PipeCreate { capacity, out_ptr } => {
            // The ring is charged to the creating cell, so a pipe is not a way to
            // hold memory outside the per-cell quota.
            let owner_cell = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .map(|task| task.cell_id.0 as usize)
                .ok_or(SyscallError::PermissionDenied)?;
            let (read_handle, write_handle) = super::pipe::create(capacity, owner_cell)?;
            let mut out = [0u8; 16];
            let mut offset = 0usize;
            for word in [read_handle.to_raw(), write_handle.to_raw()] {
                let bytes = word.to_ne_bytes();
                out[offset..offset + bytes.len()].copy_from_slice(&bytes);
                offset += bytes.len();
            }
            if let Err(error) = write_user_slice(caller_id, out_ptr, &out[..offset], 64) {
                super::pipe::close_end(read_handle, caller_id);
                super::pipe::close_end(write_handle, caller_id);
                return Err(error);
            }
            let installed = super::SCHEDULER
                .lock()
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| {
                    #[cfg(cellos_p01_trace)]
                    let _semantic = crate::memory::heap::p01_owner::creation(
                        Some(crate::p01_resources::owner(task)),
                        crate::memory::heap::p01_owner::TASK_FIELDS,
                    );
                    task.pipe_handles.insert(read_handle);
                    task.pipe_handles.insert(write_handle);
                })
                .is_some();
            if !installed {
                super::pipe::close_end(read_handle, caller_id);
                super::pipe::close_end(write_handle, caller_id);
                return Err(SyscallError::PermissionDenied);
            }
            Ok(0)
        }
        Syscall::PipeRead {
            handle,
            buf_ptr,
            len,
            timeout_ticks,
        } => {
            const CHUNK: usize = 512;
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            if handle.end != super::pipe::PipeEnd::Read || !caller_owns_pipe(caller_id, handle) {
                audit_syscall_denied(caller_id, handle.to_raw() as u32);
                return Err(SyscallError::PermissionDenied);
            }
            if len == 0 {
                return Ok(0);
            }
            let deadline = if timeout_ticks == 0 {
                None
            } else {
                Some(super::system_ticks() as u64 + timeout_ticks)
            };
            let count = len.min(CHUNK);
            let mut scratch = [0u8; CHUNK];
            loop {
                match super::pipe::try_read(handle, &mut scratch[..count], caller_id) {
                    Some(super::pipe::ReadOutcome::Read(read)) => {
                        write_user_slice(caller_id, buf_ptr, &scratch[..read], MAX_USER_BUF)?;
                        return Ok(read);
                    }
                    // EOF: the ring is empty and no writer end remains.
                    Some(super::pipe::ReadOutcome::Eof) => return Ok(0),
                    Some(super::pipe::ReadOutcome::Empty) => {
                        if deadline.is_some_and(|d| super::system_ticks() as u64 >= d) {
                            return Ok(0);
                        }
                        // Enqueue and park under one lock: a writer that drains the
                        // queue takes `SCHEDULER` to wake us, so either it sees our
                        // entry and wakes a parked task, or we see its bytes on the
                        // next `try_read`. Lock order stays SCHEDULER -> PIPES.
                        if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                            match sched.tasks.get_mut(&caller_id) {
                                Some(task) => {
                                    super::pipe::park_reader(handle, caller_id);
                                    task.state =
                                        super::tcb::TaskState::PipeRead { handle, deadline };
                                }
                                None => return Err(SyscallError::PermissionDenied),
                            }
                        }
                        super::yield_cpu();
                    }
                    None => return Err(SyscallError::InvalidInput),
                }
            }
        }
        Syscall::PipeWrite {
            handle,
            buf_ptr,
            len,
            timeout_ticks,
        } => {
            const CHUNK: usize = 512;
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            if handle.end != super::pipe::PipeEnd::Write || !caller_owns_pipe(caller_id, handle) {
                audit_syscall_denied(caller_id, handle.to_raw() as u32);
                return Err(SyscallError::PermissionDenied);
            }
            if len == 0 {
                // A zero-length write closes the writer end: readers then observe
                // EOF once they have drained the ring.
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    if let Some(task) = sched.tasks.get_mut(&caller_id) {
                        task.pipe_handles.remove(&handle);
                    }
                }
                super::pipe::close_end(handle, caller_id);
                return Ok(0);
            }
            let deadline = if timeout_ticks == 0 {
                None
            } else {
                Some(super::system_ticks() as u64 + timeout_ticks)
            };
            let count = len.min(CHUNK);
            let mut scratch = [0u8; CHUNK];
            {
                let view = caller_copy_view_for(caller_id)?;
                view.read_into(buf_ptr, &mut scratch[..count])
                    .map_err(|_| SyscallError::InvalidInput)?;
            }
            loop {
                match super::pipe::try_write(handle, &scratch[..count], caller_id) {
                    Some(super::pipe::WriteOutcome::Wrote(written)) => return Ok(written),
                    // No reader end remains: the stream is broken, not full.
                    Some(super::pipe::WriteOutcome::NoReader) => {
                        return Err(SyscallError::BrokenPipe);
                    }
                    Some(super::pipe::WriteOutcome::Full) => {
                        if deadline.is_some_and(|d| super::system_ticks() as u64 >= d) {
                            return Ok(0);
                        }
                        if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                            match sched.tasks.get_mut(&caller_id) {
                                Some(task) => {
                                    super::pipe::park_writer(handle, caller_id);
                                    task.state =
                                        super::tcb::TaskState::PipeWrite { handle, deadline };
                                }
                                None => return Err(SyscallError::PermissionDenied),
                            }
                        }
                        super::yield_cpu();
                    }
                    None => return Err(SyscallError::InvalidInput),
                }
            }
        }
        Syscall::PipeClose { handle } => {
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            let owned = super::SCHEDULER
                .lock()
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| task.pipe_handles.remove(&handle))
                .unwrap_or(false);
            if !owned {
                audit_syscall_denied(caller_id, handle.to_raw() as u32);
                return Err(SyscallError::PermissionDenied);
            }
            super::pipe::close_end(handle, caller_id);
            Ok(0)
        }
        Syscall::PipeShare { handle, target_tid } => {
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            // Owner-only: a task cannot hand out an endpoint it does not hold. That
            // ownership rule is the whole authorization model for pipes. The
            // endpoint count and target table change together while SCHEDULER is
            // held, preserving the documented SCHEDULER -> PIPES lock order.
            let mut sched = super::SCHEDULER.lock();
            let sched = sched.as_mut().ok_or(SyscallError::Unknown)?;
            let owned = sched
                .tasks
                .get(&caller_id)
                .is_some_and(|task| task.pipe_handles.contains(&handle));
            if !owned {
                crate::audit::log_event(
                    crate::audit::AuditEvent::SyscallDenied,
                    &crate::audit::encode_subject(
                        crate::audit::Subject::of(sched, caller_id),
                        handle.to_raw() as u32,
                        0,
                    ),
                );
                return Err(SyscallError::PermissionDenied);
            }
            let already_granted = sched
                .tasks
                .get(&target_tid)
                .ok_or(SyscallError::InvalidInput)?
                .pipe_handles
                .contains(&handle);
            if already_granted {
                return Ok(0);
            }
            if !super::pipe::duplicate_end(handle) {
                return Err(SyscallError::InvalidInput);
            }
            let target = sched
                .tasks
                .get_mut(&target_tid)
                .expect("target task remained live while holding SCHEDULER");
            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                Some(crate::p01_resources::owner(target)),
                crate::memory::heap::p01_owner::TASK_FIELDS,
            );
            target.pipe_handles.insert(handle);
            Ok(0)
        }
        Syscall::Wait { pid } => {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                let already_reaped = if let Some(target) = sched.tasks.get_mut(&pid) {
                    #[cfg(cellos_p01_trace)]
                    let _semantic = crate::memory::heap::p01_owner::creation(
                        Some(crate::p01_resources::owner(target)),
                        crate::memory::heap::p01_owner::TASK_FIELDS,
                    );
                    target.waiters.push(caller_id);
                    false
                } else if let Some(zombie) = sched.zombies.iter_mut().find(|task| task.id == pid) {
                    // It exited but a hart may still hold its saved context.
                    // The zombie reaper transfers these waiters only after the
                    // exact selected/executing quiescence proof.
                    #[cfg(cellos_p01_trace)]
                    let _semantic = crate::memory::heap::p01_owner::creation(
                        Some(crate::p01_resources::owner(zombie)),
                        crate::memory::heap::p01_owner::TASK_FIELDS,
                    );
                    zombie.waiters.push(caller_id);
                    false
                } else {
                    true
                };
                if already_reaped {
                    return Err(SyscallError::InvalidDriverId);
                }

                if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                    caller.state = TaskState::Waiting { target: pid };
                }
            }
            super::yield_cpu();
            // Resume with exit code published by the quiescent zombie reaper.
            if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                return Ok(sched
                    .tasks
                    .get(&caller_id)
                    .and_then(|task| task.reply_value)
                    .unwrap_or(0));
            }
            Ok(0)
        }
        Syscall::ShmAlloc { size } => grant::shm_alloc(caller_id, size),
        Syscall::ShmMap { handle, target_pid } => grant::shm_map(caller_id, handle, target_pid),
        Syscall::FutexWait {
            addr,
            expected,
            timeout_ticks,
        } => {
            // The wait parks the caller inside `futex::wait`; the outcome
            // (woken / value mismatch / timed out) is the syscall's return value,
            // so it needs no error encoding. An invalid address is a recoverable
            // error, never a kernel fault.
            super::futex::wait(caller_id, addr, expected, timeout_ticks)
        }
        Syscall::FutexWake { addr, count } => {
            if let Ok(n) = super::futex::wake(caller_id, addr, count) {
                Ok(n)
            } else {
                Err(SyscallError::Unknown) // Should not fail typically
            }
        }
        Syscall::Log { msg_ptr, msg_len } => {
            if let Ok(msg) = read_user_string(caller_id, msg_ptr, msg_len, MAX_LOG_MSG) {
                #[cfg(cellos_p01_trace)]
                {
                    let owner = super::SCHEDULER
                        .lock()
                        .as_ref()
                        .and_then(|sched| sched.tasks.get(&caller_id))
                        .map(|task| {
                            let owner = crate::p01_measurement::Owner::of(task);
                            // The temporary user-copy buffer belongs to this Log
                            // invocation, including while a stage census reads it.
                            if let Ok(layout) = core::alloc::Layout::array::<u8>(msg.capacity()) {
                                crate::memory::heap::p01_owner::runtime_buffer(
                                    msg.as_ptr() as usize,
                                    layout,
                                    owner,
                                    crate::memory::heap::p01_owner::TASK_IO,
                                );
                            } else {
                                crate::p01_measurement::invalidate(crate::p01_measurement::COUNTER);
                            }
                            owner
                        });
                    if let Some(owner) = owner {
                        crate::p01_measurement::stage(owner, &msg);
                    }
                }
                crate::task::print_user_log(&msg, caller_id);
            }
            Ok(0)
        }
        Syscall::Grant {
            target,
            ptr,
            len,
            flags,
        } => grant::grant(caller_id, target, ptr, len, flags),
        Syscall::Map { grant_id } => grant::map(caller_id, grant_id),
        Syscall::Exit { code } => {
            // TID zero is the explicit kernel-context sentinel. It has no Cell
            // generation to retire; a kernel scheduling point remains explicit
            // rather than fabricating a user-exit record.
            if caller_id == 0 {
                super::yield_cpu();
                return Ok(0);
            }

            #[cfg(feature = "bench-counters")]
            crate::bench_counters::note_exit(caller_id);

            // Do not inspect the task table, log, audit, or call
            // `Scheduler::exit_task` while the exiting Cell is still charged.
            // A root exit can retire every member and grow scheduler vectors;
            // the fixed record is therefore the entire victim-attributed path.
            let exit = super::hart_local::DeferredExit {
                tid: caller_id,
                cell_id: super::hart_local::current_cell_id(),
                generation: super::hart_local::current_cell_generation(),
                code,
            };
            super::hart_local::defer_exit(exit);
            super::hart_local::set_current_cell_id(0);

            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            super::retirement_selftest::observe_exit_deferred_record_commit(exit);

            // `yield_cpu` consumes the fixed record under normal scheduler
            // locking and routes roots through quiescent retirement. A worker
            // still reaches the task-local branch of `exit_task`.
            super::yield_cpu();
            Ok(0)
        }

        Syscall::ForceExit { tid } => {
            // Self-kill rejected before touching the scheduler (cheap early check).
            if tid == caller_id {
                return Err(SyscallError::InvalidCommand);
            }

            // Single SCHEDULER lock: SpawnCap gate + all cleanup in one scope.
            // Two separate acquisitions would create a TOCTOU window where the target
            // self-exits between them, causing a spurious InvalidCommand return.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                // Gate 1: only SpawnCap holders (init/shell) may force-terminate tasks.
                let has_spawn =
                    sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).spawn;
                if !has_spawn {
                    return Err(SyscallError::PermissionDenied);
                }

                // Gate 2: protect system service cells (VFS=block_io_cap, net=network_cap).
                // Killing them mid-I/O leaves driver state inconsistent; use hot-swap instead.
                let target_rights = sched.cell_rights(tid);
                let target_is_system = target_rights.block_io || target_rights.network;
                if target_is_system {
                    return Err(SyscallError::PermissionDenied);
                }

                // Gate 3: protect Frozen cells — they are mid-swap and cannot be killed
                // by external actors; only the supervisor-owned cutover path may terminate
                // them via the internal exit_task path after a successful swap.
                let target_is_frozen = sched
                    .tasks
                    .get(&tid)
                    .map(|t| matches!(t.state, TaskState::Frozen { .. }))
                    .unwrap_or(false);
                if target_is_frozen {
                    return Err(SyscallError::PermissionDenied);
                }

                let task = match sched.tasks.get_mut(&tid) {
                    Some(t) => t,
                    // Target self-exited between the lock boundary — already dead; mission done.
                    None => return Ok(0),
                };
                task.exit_code = Some(usize::MAX); // sentinel: force-killed

                // exit_task: zombie move + ready-queue purge + stuck-sender unblock,
                // and wakes sys_wait(tid) waiters with the force-kill sentinel.
                sched.exit_task(tid, usize::MAX);
            } else {
                return Err(SyscallError::InvalidCommand);
            }

            crate::audit::log_event(
                crate::audit::AuditEvent::CellExit,
                &crate::audit::encode_u32x2(tid as u32, 0xFFFF_FFFFu32), // force-kill marker
            );

            log::info!(
                "[kernel] ForceExit: task {} killed by task {}",
                tid,
                caller_id
            );

            Ok(0) // non-blocking — caller keeps running; do NOT yield_cpu
        }

        Syscall::CapRevoke {
            target_tid,
            cap_mask,
        } => authority::cap_revoke(caller_id, target_tid, cap_mask),

        Syscall::NotifyOnExit { watched } => {
            // Privileged: only SpawnCap holders (supervisors like init) may watch
            // arbitrary tasks — same authority gate as ForceExit. The watcher's
            // next Recv returns `watched` when it dies (see exit_task delivery).
            //
            // Race: the watched task may have already exited before this call.
            // Subscribe while SCHEDULER still proves the target is live; exit_task
            // takes DEATH_SUBSCRIBERS under the same outer lock, so publication and
            // delivery are atomic with respect to task death.
            {
                let mut sched_opt = super::SCHEDULER.lock();
                let sched = match sched_opt.as_mut() {
                    Some(s) => s,
                    None => return Ok(0),
                };
                let has_spawn =
                    sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).spawn;
                if !has_spawn {
                    return Err(SyscallError::PermissionDenied);
                }
                if sched.tasks.get(&watched).is_some_and(|task| {
                    !matches!(task.state, TaskState::Retiring | TaskState::Terminated)
                }) {
                    super::scheduler::subscribe_death(watched, caller_id);
                } else {
                    // Task already dead — queue synthetic death so watcher never stalls.
                    if let Some(wt) = sched.tasks.get_mut(&caller_id) {
                        super::scheduler::queue_pending_death(wt, watched, 0);
                    }
                }
            }
            Ok(0)
        }

        Syscall::QueryRetirement {
            root_tid,
            out_ptr,
            out_len,
        } => {
            // Admitted past the allowlist; the verdict table is the gate: a
            // death subscriber of that root at its retirement, or a supervisor.
            if out_len != api::retirement::RETIREMENT_RECORD_LEN {
                return Err(SyscallError::InvalidInput);
            }
            let supervisor = caller_has_supervisor(caller_id);
            let record = crate::cell::retirement_verdict::query(caller_id, supervisor, root_tid)
                .map_err(|error| match error {
                    crate::cell::retirement_verdict::QueryError::NotFound => {
                        SyscallError::FileNotFound
                    }
                    crate::cell::retirement_verdict::QueryError::Denied => {
                        SyscallError::PermissionDenied
                    }
                })?;
            write_user_slice(
                caller_id,
                out_ptr,
                &record.encode(),
                api::retirement::RETIREMENT_RECORD_LEN,
            )?;
            Ok(api::retirement::RETIREMENT_RECORD_LEN)
        }

        Syscall::LookupBlockRoute {
            index,
            out_ptr,
            out_len,
            reserved,
        } => {
            // Admitted past the allowlist; the VFS (BlockIoCap) and init
            // (SpawnCap) are the route readers.
            if !(caller_has_block_io(caller_id) || caller_has_spawn(caller_id)) {
                return Err(SyscallError::PermissionDenied);
            }
            let len = api::block_route::BLOCK_ROUTE_RECORD_LEN;
            if out_len != len || reserved != 0 {
                return Err(SyscallError::InvalidInput);
            }
            let record = crate::task::drivers::driver_cell::block_route_record(index)
                .ok_or(SyscallError::FileNotFound)?;
            write_user_slice(caller_id, out_ptr, &record.encode(), len)?;
            Ok(len)
        }

        Syscall::QueryMachineFacts {
            out_ptr,
            out_len,
            a2,
            a3,
        } => {
            // Admitted past the allowlist; init (SpawnCap) is the reader.
            if !caller_has_spawn(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if out_len != api::machine_facts::MACHINE_FACTS_CAPACITY || a2 != 0 || a3 != 0 {
                return Err(SyscallError::InvalidInput);
            }
            let record = crate::machine_facts::query().ok_or(SyscallError::FileNotFound)?;
            write_user_slice(caller_id, out_ptr, &record, record.len())?;
            Ok(record.len())
        }

        Syscall::RegisterService { service_id, tid } => {
            authority::register_service(caller_id, service_id, tid)
        }
        Syscall::LookupService { service_id } => authority::lookup_service(service_id),
        Syscall::LookupServiceBound {
            service_id,
            out_ptr,
            out_len,
        } => authority::lookup_service_bound(caller_id, service_id, out_ptr, out_len),
        Syscall::Heartbeat { interval } => {
            // Open: a cell asserts its own liveness. Arms a deadline `interval` ticks
            // ahead; `pick_next` terminates the cell as HUNG if it lapses. interval=0
            // disables. Self-targeted only — a cell can only (re)arm its OWN deadline.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    t.heartbeat_deadline = if interval == 0 {
                        None
                    } else {
                        Some(super::system_ticks() as u64 + interval as u64)
                    };
                }
            }
            Ok(0)
        }

        Syscall::Reply { caller: _, result } => {
            super::ipc_reply(caller_id, result).map_err(|_| SyscallError::InvalidCommand)
        }

        Syscall::Lend {
            target,
            ptr,
            len,
            flags,
        } => grant::lend(caller_id, target, ptr, len, flags),

        Syscall::BorrowRead {
            lease_id,
            offset,
            ptr,
            len,
        } => grant::borrow_read(caller_id, lease_id, offset, ptr, len),
        Syscall::BorrowWrite {
            lease_id,
            offset,
            ptr,
            len,
        } => grant::borrow_write(caller_id, lease_id, offset, ptr, len),

        // --- Legacy Implementation ---
        Syscall::Yield => {
            super::yield_cpu();
            Ok(0)
        }
        Syscall::ServiceLookup { name_ptr, name_len } => {
            authority::service_lookup(caller_id, name_ptr, name_len)
        }
        Syscall::Open { path_ptr, path_len } => {
            let path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            if let Ok(fd) = super::file_open(caller_id, &path) {
                return Ok(fd);
            }
            Err(SyscallError::FileNotFound)
        }
        Syscall::Read {
            fd,
            buf_ptr,
            buf_len,
        } => {
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_bytes = super::file_read(fd, &mut kbuf);
            // A committed child's endpoint failure is explicit, never a short count.
            if read_bytes == usize::MAX {
                return Err(SyscallError::PermissionDenied);
            }
            if read_bytes > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..read_bytes], MAX_USER_BUF)?;
            }
            Ok(read_bytes)
        }
        Syscall::Close { fd } => {
            super::file_close(fd);
            Ok(0)
        }
        Syscall::ReadDir {
            fd,
            buf_ptr,
            buf_len,
        } => {
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_bytes =
                super::file_readdir(fd, &mut kbuf).map_err(|_| SyscallError::Unknown)?;
            if read_bytes > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..read_bytes], MAX_USER_BUF)?;
            }
            Ok(read_bytes)
        }
        Syscall::Fstat {
            fd,
            out_ptr,
            out_len,
        } => {
            let len = api::syscall::VI_FSTAT_V1_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            validate_user_buf(out_ptr, len, MAX_USER_BUF)?;
            preflight_user_output(caller_id, out_ptr, len)?;

            let metadata = super::file_fstat(caller_id, fd).map_err(|_| SyscallError::Unknown)?;
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &metadata as *const api::syscall::ViFstatV1 as *const u8,
                    len,
                )
            };
            write_user_slice(caller_id, out_ptr, bytes, MAX_USER_BUF)?;
            Ok(len)
        }
        Syscall::Rename {
            old_ptr,
            old_len,
            new_ptr,
            new_len,
        } => {
            let old_path = read_user_string(caller_id, old_ptr, old_len, MAX_LOG_MSG)?;
            let new_path = read_user_string(caller_id, new_ptr, new_len, MAX_LOG_MSG)?;
            let vfs_tid = crate::cell::service_registry::lookup(api::syscall::service::VFS)
                .ok_or(SyscallError::InvalidCommand)?;
            let request = api::ipc::VfsRequest::Rename {
                old: &old_path,
                new: &new_path,
            };
            let mut request_buf = [0u8; api::ipc::IPC_BUF_SIZE];
            let encoded = api::ipc::encode(&request, &mut request_buf)
                .map_err(|_| SyscallError::InvalidCommand)?;
            match super::ipc_send_kernel(caller_id, vfs_tid, encoded) {
                Ok(0) => {}
                Ok(1) => super::yield_cpu(),
                Err(super::IpcSendError::Backpressure) => return Err(SyscallError::TryAgain),
                Ok(_) => return Err(SyscallError::InvalidCommand),
                Err(super::IpcSendError::TargetGone) => return Err(SyscallError::InvalidCommand),
            }

            let mut response_buf = [0u8; api::ipc::IPC_BUF_SIZE];
            loop {
                let response = super::ipc_recv_kernel(caller_id, vfs_tid, &mut response_buf)
                    .map_err(|_| SyscallError::InvalidCommand)?;
                match response {
                    Some((_, len)) => {
                        return match api::ipc::decode::<api::ipc::VfsResponse>(&response_buf[..len])
                        {
                            Ok(api::ipc::VfsResponse::Ok) => Ok(0),
                            _ => Err(SyscallError::PermissionDenied),
                        };
                    }
                    None => super::yield_cpu(),
                }
            }
        }
        // Syscall::Remove removed
        Syscall::ChDir { path_ptr, path_len } => {
            let path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            if super::file_chdir(caller_id, &path).is_ok() {
                return Ok(0);
            }
            Err(SyscallError::FileNotFound)
        }
        Syscall::GetCwd { buf_ptr, buf_len } => {
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            if let Ok(len) = super::file_getcwd(caller_id, &mut kbuf) {
                write_user_slice(caller_id, buf_ptr, &kbuf[..len], MAX_USER_BUF)?;
                return Ok(len);
            }
            Err(SyscallError::BufferTooSmall)
        }
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
        } => {
            let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
            let written = super::file_write(fd, &bytes);
            if written == usize::MAX {
                return Err(SyscallError::PermissionDenied);
            }
            Ok(written)
        }
        Syscall::MkDir { path_ptr, path_len } => {
            let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            Err(SyscallError::PermissionDenied)
        }
        Syscall::Exec { path_ptr, path_len } => {
            let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            Err(SyscallError::NotSupported)
        }
        Syscall::SpawnFromPath { path_ptr, path_len } => {
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            #[cfg(cellos_p01_trace)]
            crate::p01_measurement::classify(trace, &path_str);
            if !path_str.starts_with('/') {
                return Err(SyscallError::InvalidInput);
            }
            let profile = authorize_launch_edge(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Path,
                &path_str,
            )?;
            let request = match governed_spawn_request(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Path,
                profile.child_ceiling,
                api::TaskPriority::Normal as u8,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            #[cfg(cellos_p01_trace)]
            let request = {
                let mut request = request;
                request.trace = *trace;
                request
            };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            #[cfg(cellos_p01_trace)]
            if let Err(error) = spawned {
                crate::p01_measurement::source_error(*trace, error);
            }
            // A successful spawn consumed any staged directory-handle set inside
            // task creation. Clearing unconditionally covers the failure paths:
            // a set left staged by a spawn that never produced a child would be
            // handed to whichever child this caller created next.
            crate::task::dir_inherit::clear_staged(caller_id);
            let task_id = spawned.map_err(|e| {
                log::warn!(
                    "[loader] SpawnFromPath refused: caller={} path={} error={:?}",
                    caller_id,
                    path_str,
                    e
                );
                match e {
                    types::ViError::NotFound => SyscallError::FileNotFound,
                    types::ViError::OutOfMemory => {
                        log::warn!(
                            "[loader] spawn OOM: op=SpawnFromPath caller={} path={}",
                            caller_id,
                            path_str
                        );
                        SyscallError::OutOfMemory
                    }
                    _ => SyscallError::InvalidInput,
                }
            })?;
            Ok(task_id)
        }

        Syscall::SpawnSetDirs { carrier_ptr } => {
            if !caller_has_spawn(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if carrier_ptr == 0 {
                crate::task::dir_inherit::clear_staged(caller_id);
                return Ok(0);
            }
            let carrier_size = core::mem::size_of::<api::dir_handles::ViSpawnDirHandles>();
            let bytes = read_user_slice(caller_id, carrier_ptr, carrier_size, MAX_USER_BUF)?;
            let carrier = unsafe {
                core::ptr::read_unaligned(
                    bytes.as_ptr() as *const api::dir_handles::ViSpawnDirHandles
                )
            };
            let set = api::dir_handles::DirHandleSet::from_carrier(&carrier).map_err(|e| {
                log::warn!(
                    "[dirs] tid {} named an invalid directory handle set: {:?}",
                    caller_id,
                    e
                );
                SyscallError::InvalidInput
            })?;
            crate::task::dir_inherit::stage(caller_id, set);
            Ok(0)
        }

        Syscall::QueryDirHandles {
            cell_id,
            buf_ptr,
            buf_len,
        } => {
            if !may_query_dir_handles(caller_id, cell_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let len = api::dir_attestation::DIR_ATTESTATION_LEN;
            if buf_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let record = crate::task::dir_inherit::attestation_for(cell_id)
                .ok_or(SyscallError::InvalidInput)?
                .to_bytes();
            write_user_slice(caller_id, buf_ptr, &record, MAX_USER_BUF)?;
            Ok(len)
        }

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
        } => {
            let elf = snapshot_grant_elf(caller_id, grant_id, len)?;
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            #[cfg(cellos_p01_trace)]
            crate::p01_measurement::classify(trace, &path_str);
            if !path_str.starts_with('/') {
                return Err(SyscallError::InvalidInput);
            }
            let profile = authorize_launch_edge(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Elf,
                &path_str,
            )?;
            log::info!(
                "[loader] SpawnFromElf: {} ({} bytes from grant)",
                path_str,
                len
            );
            let request = match governed_spawn_request(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Elf,
                profile.child_ceiling,
                api::TaskPriority::Normal as u8,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            #[cfg(cellos_p01_trace)]
            let request = {
                let mut request = request;
                request.trace = *trace;
                request
            };
            let spawned = crate::loader::spawn_gated(elf.bytes(), &path_str, request);
            #[cfg(cellos_p01_trace)]
            if let Err(error) = spawned {
                crate::p01_measurement::source_error(*trace, error);
            }
            crate::task::dir_inherit::clear_staged(caller_id);
            let task_id = spawned.map_err(|e| match e {
                types::ViError::NotFound => SyscallError::FileNotFound,
                types::ViError::OutOfMemory => {
                    log::warn!(
                        "[loader] spawn OOM: op=SpawnFromElf caller={} path={} elf_len={}",
                        caller_id,
                        path_str,
                        len
                    );
                    SyscallError::OutOfMemory
                }
                types::ViError::PermissionDenied => SyscallError::PermissionDenied,
                _ => SyscallError::InvalidInput,
            })?;
            Ok(task_id)
        }

        Syscall::SpawnPinned {
            path_ptr,
            path_len,
            priority,
            core_id,
        } => {
            if core_id != 0 {
                return Err(SyscallError::NotSupported);
            }
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            #[cfg(cellos_p01_trace)]
            crate::p01_measurement::classify(trace, &path_str);
            if !path_str.starts_with('/') {
                return Err(SyscallError::InvalidInput);
            }
            let profile = authorize_launch_edge(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Pinned,
                &path_str,
            )?;
            let request = match governed_spawn_request(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Pinned,
                profile.child_ceiling,
                priority,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            #[cfg(cellos_p01_trace)]
            let request = {
                let mut request = request;
                request.trace = *trace;
                request
            };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            #[cfg(cellos_p01_trace)]
            if let Err(error) = spawned {
                crate::p01_measurement::source_error(*trace, error);
            }
            crate::task::dir_inherit::clear_staged(caller_id);
            let task_id = spawned.map_err(|e| match e {
                types::ViError::NotFound => SyscallError::FileNotFound,
                types::ViError::OutOfMemory => {
                    let (quota_nulls, heap_nulls) = crate::memory::heap::oom_sources();
                    let (used, free) = crate::memory::heap::usage();
                    let charged = crate::memory::cell_quota::in_use(types::CellId(
                        crate::task::scheduler::current_cell_id() as u64,
                    ));
                    log::warn!(
                        "[loader] spawn OOM: op=SpawnPinned caller={} path={} heap_used={} heap_free={} caller_charged={} task_size={} null_from_quota={} null_from_heap={}",
                        caller_id,
                        path_str,
                        used,
                        free,
                        charged,
                        core::mem::size_of::<crate::task::tcb::Task>(),
                        quota_nulls,
                        heap_nulls
                    );
                    #[cfg(feature = "cell-scale-experiment")]
                    log::warn!(
                        "[heap] net per size class (bound, allocs, net_bytes): {:?}",
                        crate::memory::heap::size_hist::snapshot()
                    );
                    // Sizes of the per-spawn structures are logged by the alloc
                    // error handler when a layout cannot be satisfied at all.
                    SyscallError::OutOfMemory
                }
                _ => SyscallError::InvalidInput,
            })?;
            // Retained-cost readout: successive spawns show what one parked cell
            // costs the kernel heap (the number the D5 gate needs).
            let (used, free) = crate::memory::heap::usage();
            log::info!(
                "[heap] used={} KiB free={} KiB after SpawnPinned path={}",
                used / 1024,
                free / 1024,
                path_str
            );
            Ok(task_id)
        }

        Syscall::OpenCap { path_ptr, path_len } => {
            let path_str = read_user_string(caller_id, path_ptr, path_len, 256)?;
            use crate::fs::VIFS1;
            let file = {
                let mut guard = VIFS1.lock();
                guard
                    .as_mut()
                    .ok_or(SyscallError::FileNotFound)?
                    .open(&path_str, api::fs::OpenMode::Read)
                    .map_err(|_| SyscallError::FileNotFound)?
            };
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
            let auth = auths
                .get_mut(&(cell_id.0, cell_gen))
                .ok_or(SyscallError::PermissionDenied)?;
            let ticket = auth
                .allocate_file_slot(file, api::cap::CapPerms::FILE_READ.0)
                .map_err(|_| SyscallError::OutOfMemory)?;
            Ok(ticket as usize)
        }

        Syscall::ReadCap {
            cap_id,
            buf_ptr,
            buf_len,
        } => {
            if buf_len == 0 {
                return Ok(0);
            }
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_result = boxed_file.read(&mut kbuf);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            match read_result {
                Ok(n) => {
                    if n > 0 {
                        write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
                    }
                    Ok(n)
                }
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::SeekCap {
            cap_id,
            offset,
            whence,
        } => {
            let pos = match whence {
                0 => api::fs::SeekFrom::Start(offset as u64),
                1 => api::fs::SeekFrom::Current(offset as isize as i64),
                2 => api::fs::SeekFrom::End(offset as isize as i64),
                _ => return Err(SyscallError::InvalidInput),
            };
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let seek_result = boxed_file.seek(pos);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            match seek_result {
                Ok(new_pos) => Ok(new_pos as usize),
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::WriteCap {
            cap_id,
            buf_ptr,
            buf_len,
        } => {
            if buf_len == 0 {
                return Ok(0);
            }
            let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let write_result = boxed_file.write(&bytes);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            match write_result {
                Ok(n) => Ok(n),
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::StatCap { cap_id } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let result = boxed_file.size();
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            result
                .map(|s| s as usize)
                .map_err(|_| SyscallError::Unknown)
        }

        Syscall::TruncateCap { cap_id, len } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let result = boxed_file.truncate(len as u64);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
        }

        Syscall::SyncCap { cap_id } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let result = boxed_file.sync();
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
        }

        Syscall::GrantDma { bdf, phys, size } => grant::grant_dma(caller_id, bdf, phys, size),

        Syscall::CloseCap { cap_id } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
            let auth = auths
                .get_mut(&(cell_id.0, cell_gen))
                .ok_or(SyscallError::PermissionDenied)?;
            auth.close_ticket(cap_id as u64)
                .map_err(|_| SyscallError::PermissionDenied)?;
            Ok(0)
        }

        Syscall::SpawnFromMem { args_ptr } => {
            if args_ptr == 0 {
                return Err(SyscallError::InvalidInput);
            }
            let view = caller_copy_view(caller_id)?;
            let mut args_bytes = [0u8; core::mem::size_of::<ViSpawnArgs>()];
            view.read_into(args_ptr, &mut args_bytes)
                .map_err(|_| SyscallError::InvalidInput)?;
            let args =
                unsafe { core::ptr::read_unaligned(args_bytes.as_ptr() as *const ViSpawnArgs) };
            validate_user_buf(args.buffer_addr, args.buffer_size, MAX_USER_BUF)?;
            validate_user_buf(args.name_ptr, args.name_len, MAX_LOG_MSG)?;
            let elf_bytes = view
                .read_bytes(args.buffer_addr, args.buffer_size)
                .map_err(|_| SyscallError::InvalidInput)?;
            let name_bytes = view
                .read_bytes(args.name_ptr, args.name_len)
                .map_err(|_| SyscallError::InvalidInput)?;
            let name = core::str::from_utf8(&name_bytes).unwrap_or("unknown");
            let profile = authorize_launch_edge(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Mem,
                name,
            )?;
            let request = match governed_spawn_request(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Mem,
                profile.child_ceiling,
                api::TaskPriority::Normal as u8,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            let spawned =
                crate::loader::mem_spawn_gate::spawn_from_mem_gated(&elf_bytes, name, request);
            crate::task::dir_inherit::clear_staged(caller_id);
            spawned.map_err(|e| match e {
                types::ViError::PermissionDenied => SyscallError::PermissionDenied,
                types::ViError::OutOfMemory => {
                    log::warn!(
                        "[loader] spawn OOM: op=SpawnFromMem caller={} name={} elf_len={}",
                        caller_id,
                        name,
                        elf_bytes.len()
                    );
                    SyscallError::OutOfMemory
                }
                _ => SyscallError::InvalidInput,
            })
        }

        Syscall::Create { path_ptr, path_len } => {
            let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            Err(SyscallError::PermissionDenied)
        }
        Syscall::SetTimer { deadline } => {
            // Check if deadline passed
            let now = super::system_ticks();
            let wake_at = now + deadline;

            // Sleep!
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.current_task_mut() {
                    task.state = TaskState::Sleeping { until: wake_at };
                }
            }
            // Yield CPU safely
            super::yield_cpu();
            Ok(0)
        }

        Syscall::GetProcs { buf_ptr, buf_len } => {
            let bytes_len = buf_len
                .checked_mul(core::mem::size_of::<api::syscall::ProcessInfo>())
                .ok_or(SyscallError::InvalidInput)?;
            validate_user_buf(buf_ptr, bytes_len, MAX_USER_BUF)?;

            let rows = collect_process_rows(buf_len, snapshot_process_info);
            let raw_bytes = unsafe {
                core::slice::from_raw_parts(
                    rows.as_ptr() as *const u8,
                    rows.len() * core::mem::size_of::<api::syscall::ProcessInfo>(),
                )
            };
            write_user_slice(caller_id, buf_ptr, raw_bytes, MAX_USER_BUF)?;
            Ok(rows.len())
        }

        Syscall::GetProcs2 { buf_ptr, buf_len } => {
            let bytes_len = buf_len
                .checked_mul(core::mem::size_of::<api::syscall::ProcessInfoV2>())
                .ok_or(SyscallError::InvalidInput)?;
            validate_user_buf(buf_ptr, bytes_len, MAX_USER_BUF)?;

            let sample_ticks = super::system_ticks() as u64;
            let rows =
                collect_process_rows(buf_len, |task| snapshot_process_info_v2(task, sample_ticks));
            let raw_bytes = unsafe {
                core::slice::from_raw_parts(
                    rows.as_ptr() as *const u8,
                    rows.len() * core::mem::size_of::<api::syscall::ProcessInfoV2>(),
                )
            };
            write_user_slice(caller_id, buf_ptr, raw_bytes, MAX_USER_BUF)?;
            Ok(rows.len())
        }

        Syscall::MemInfo { out_ptr, out_len } => {
            let len = core::mem::size_of::<api::syscall::ViMemInfoV1>();
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            validate_user_buf(out_ptr, len, MAX_USER_BUF)?;
            let info = {
                let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
                let allocator = guard.as_ref().ok_or(SyscallError::OutOfMemory)?;
                api::syscall::ViMemInfoV1 {
                    total_frames: allocator.total_frames() as u64,
                    used_frames: allocator.used_frames() as u64,
                    free_frames: allocator.free_frames() as u64,
                    page_size: allocator.page_size() as u64,
                }
            };
            let raw_bytes = unsafe {
                core::slice::from_raw_parts(
                    &info as *const api::syscall::ViMemInfoV1 as *const u8,
                    len,
                )
            };
            write_user_slice(caller_id, out_ptr, raw_bytes, MAX_USER_BUF)?;
            Ok(len)
        }

        #[cfg(feature = "bench-counters")]
        Syscall::BenchCounters { out_ptr, out_len } => {
            misc::bench_counters(caller_id, out_ptr, out_len)
        }

        Syscall::TelemetrySnapshot {
            out_ptr,
            out_len,
            flags,
            reserved,
        } => {
            use api::telemetry::request;
            // The dispatch gate already required `observe_tasks` (X). Arguments
            // are refused before anything is sampled.
            if !super::telemetry::request_is_valid(out_ptr, out_len, flags, reserved) {
                return Err(SyscallError::InvalidInput);
            }
            validate_user_buf(out_ptr, out_len, MAX_USER_BUF)?;
            let memory_allowed = flags & request::SYSTEM_MEMORY != 0
                && caller_has_cap(caller_id, |rights| rights.observe_memory);
            let epoch = super::telemetry::provider_epoch();
            // Identity, membership and CPU in one SCHEDULER hold. The transient
            // rows are allocated there, charged to the caller like GetProcs2.
            let capture = {
                let mut guard = super::SCHEDULER.lock();
                let sched = guard.as_mut().ok_or(SyscallError::Unknown)?;
                super::telemetry::capture(
                    sched,
                    epoch,
                    super::telemetry::monotonic_ns(),
                    out_len,
                    flags & request::TASKS != 0,
                )
                .map_err(|_| SyscallError::OutOfMemory)?
            };
            // Frames are read under FRAME_ALLOCATOR only after SCHEDULER is released.
            let memory = if flags & request::SYSTEM_MEMORY == 0 {
                super::telemetry::SystemMemory::NotRequested
            } else if !memory_allowed {
                super::telemetry::SystemMemory::Denied
            } else {
                let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
                match guard.as_ref() {
                    Some(allocator) => {
                        super::telemetry::SystemMemory::Present(api::syscall::ViMemInfoV1 {
                            total_frames: allocator.total_frames() as u64,
                            used_frames: allocator.used_frames() as u64,
                            free_frames: allocator.free_frames() as u64,
                            page_size: allocator.page_size() as u64,
                        })
                    }
                    None => super::telemetry::SystemMemory::Unavailable,
                }
            };
            let bytes = capture.finish(memory, super::telemetry::monotonic_ns());
            write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
            Ok(bytes.len())
        }

        Syscall::Seek { fd, offset, whence } => {
            super::file_seek(fd, offset, whence).map_err(|_| SyscallError::Unknown)
        }

        Syscall::FileOp { op, arg1, arg2 } => {
            match op {
                0 => {
                    let path = read_user_string(caller_id, arg1, arg2, MAX_LOG_MSG)?;
                    super::file_remove(caller_id, &path).map_err(|_| SyscallError::PermissionDenied)
                }
                1 => {
                    // Rename - Stub
                    Err(SyscallError::NotSupported)
                }
                _ => Err(SyscallError::InvalidCommand),
            }
        }

        Syscall::GetTime { op } => {
            match op {
                // op=0: raw monotonic ticks (arch-specific frequency)
                0 => {
                    #[cfg(target_arch = "riscv64")]
                    let t = hal::common::timer::read_mtime() as usize;
                    #[cfg(target_arch = "aarch64")]
                    let t = hal::timer::read_ticks() as usize;
                    #[cfg(target_arch = "x86_64")]
                    let t = hal::clock::now_ns() as usize;
                    #[cfg(not(any(
                        target_arch = "riscv64",
                        target_arch = "aarch64",
                        target_arch = "x86_64"
                    )))]
                    let t = 0usize;
                    Ok(t)
                }
                // op=1: milliseconds since boot
                1 => {
                    // 10 MHz mtime on QEMU RV64 → 10_000 ticks/ms
                    #[cfg(target_arch = "riscv64")]
                    let ms = (hal::common::timer::read_mtime() / 10_000) as usize;
                    // Dynamic timer frequency on ARM64 (19.2 MHz on RPi3, 62.5 MHz on QEMU virt)
                    #[cfg(target_arch = "aarch64")]
                    let ms = {
                        let freq = hal::timer::read_freq();
                        let ticks_per_ms = if freq > 0 { freq / 1000 } else { 62_500 };
                        (hal::timer::read_ticks() / ticks_per_ms) as usize
                    };
                    // The x86 clocksource already returns nanoseconds; ÷ 1_000_000 → ms
                    #[cfg(target_arch = "x86_64")]
                    let ms = (hal::clock::now_ns() / 1_000_000) as usize;
                    #[cfg(not(any(
                        target_arch = "riscv64",
                        target_arch = "aarch64",
                        target_arch = "x86_64"
                    )))]
                    let ms = 0usize;
                    Ok(ms)
                }
                // op=4: scheduler ticks (10 ms preemption slices)
                4 => Ok(super::system_ticks()),
                // op=2: nanoseconds since Unix epoch (wall-clock)
                2 => {
                    #[cfg(target_arch = "riscv64")]
                    let ns = hal::common::rtc::now_epoch_ns() as usize;
                    #[cfg(target_arch = "aarch64")]
                    let ns = hal::rtc::now_epoch_ns() as usize;
                    #[cfg(target_arch = "x86_64")]
                    let ns = hal::rtc::now_epoch_ns() as usize;
                    #[cfg(not(any(
                        target_arch = "riscv64",
                        target_arch = "aarch64",
                        target_arch = "x86_64"
                    )))]
                    let ns = 0usize;
                    Ok(ns)
                }
                // op=3: seconds since Unix epoch (wall-clock)
                3 => {
                    #[cfg(target_arch = "riscv64")]
                    let s = (hal::common::rtc::now_epoch_ns() / 1_000_000_000) as usize;
                    #[cfg(target_arch = "aarch64")]
                    let s = (hal::rtc::now_epoch_ns() / 1_000_000_000) as usize;
                    #[cfg(target_arch = "x86_64")]
                    let s = (hal::rtc::now_epoch_ns() / 1_000_000_000) as usize;
                    #[cfg(not(any(
                        target_arch = "riscv64",
                        target_arch = "aarch64",
                        target_arch = "x86_64"
                    )))]
                    let s = 0usize;
                    Ok(s)
                }
                // op=5: monotonic tick counter frequency in Hz
                5 => {
                    #[cfg(target_arch = "riscv64")]
                    let freq = 10_000_000usize;
                    #[cfg(target_arch = "aarch64")]
                    let freq = hal::timer::read_freq() as usize;
                    #[cfg(target_arch = "x86_64")]
                    let freq = 1_000_000_000usize; // HPET is in nanoseconds (1 GHz)
                    #[cfg(not(any(
                        target_arch = "riscv64",
                        target_arch = "aarch64",
                        target_arch = "x86_64"
                    )))]
                    let freq = 10_000_000usize;
                    Ok(freq)
                }
                // Unknown op — return 0 for backward compatibility
                _ => Ok(0),
            }
        }
        Syscall::GetCpuCount => Ok(crate::task::smp::online_hart_count()),
        Syscall::AudioPlay { .. } => misc::audio_play(),
        // Compositor-only; the source is copied through the caller's copy view
        // into a GPU-Cell-owned Grant before the hand-off, so the GPU Cell never
        // reads compositor memory; the framebuffer fallback stages it the same
        // way (H-26).
        Syscall::GpuFlush {
            data_ptr,
            data_len,
            xy,
            wh,
        } => super::gpu_scanout::flush(caller_id, data_ptr, data_len, xy, wh),
        Syscall::GpuCursor {
            op,
            data_ptr,
            xy,
            hot,
        } => super::gpu_scanout::cursor(caller_id, op, data_ptr, xy, hot),
        Syscall::GpuGetResolution => {
            // Firmware scanout registration is authoritative when present;
            // generic VirtIO retains the legacy fallback before its driver
            // publishes a query IPC path.
            // Packed as (width << 32) | height, which only fits a 64-bit
            // register; RV32 Nano has no GPU cell so this path never fires there.
            #[cfg(not(target_arch = "riscv32"))]
            {
                if let Some((width, height)) =
                    crate::resource_registry::display_framebuffer_resolution()
                {
                    return Ok(((width as usize) << 32) | height as usize);
                }
                Ok(((1280usize) << 32) | 800usize)
            }
            #[cfg(target_arch = "riscv32")]
            {
                Ok(0)
            }
        }
        Syscall::NetTx {
            frame_ptr,
            frame_len,
        } => misc::net_tx(caller_id, frame_ptr, frame_len),
        Syscall::NetRx { buf_ptr, buf_len } => misc::net_rx(caller_id, buf_ptr, buf_len),
        Syscall::StateStash {
            key,
            buf_ptr,
            buf_len,
        } => {
            let raw_key = key as u64;
            let is_shell = caller_launch_state(caller_id)
                .is_some_and(|(name, _, _)| name.as_str() == "cellbox");
            if is_shell
                && (raw_key != SPAWN_ARGV_KEY
                    || buf_len > crate::cell::state_stash::MAX_SPAWN_ARGV_LEN)
            {
                return Err(SyscallError::PermissionDenied);
            }
            if raw_key == SPAWN_ARGV_KEY {
                let bytes = read_user_slice(
                    caller_id,
                    buf_ptr,
                    buf_len,
                    crate::cell::state_stash::MAX_SPAWN_ARGV_LEN,
                )?;
                return Ok(crate::cell::state_stash::stage_spawn_argv(
                    caller_id, &bytes,
                ));
            }
            let owner = stash_incarnation(caller_id)?;
            let bytes = read_user_slice(
                caller_id,
                buf_ptr,
                buf_len,
                crate::cell::state_stash::MAX_STASH_LEN,
            )?;
            let stored = crate::cell::state_stash::stash(owner, raw_key, &bytes);
            #[cfg(feature = "bench-counters")]
            if stored > 0 {
                crate::bench_counters::milestone("state-stashed", caller_id);
            }
            Ok(stored)
        }
        Syscall::StateRestore {
            key,
            buf_ptr,
            buf_len,
        } => {
            if key as u64 == SPAWN_ARGV_KEY {
                validate_user_buf(
                    buf_ptr,
                    buf_len,
                    crate::cell::state_stash::MAX_SPAWN_ARGV_LEN,
                )?;
                let Some(argv) = crate::cell::state_stash::take_inherited_argv(caller_id) else {
                    return Ok(0);
                };
                let n = argv.len().min(buf_len);
                if n > 0 {
                    write_user_slice(
                        caller_id,
                        buf_ptr,
                        &argv[..n],
                        crate::cell::state_stash::MAX_SPAWN_ARGV_LEN,
                    )?;
                }
                return Ok(n);
            }
            let reader = stash_incarnation(caller_id)?;
            // Empty buffer: length probe only (owner, destination or supervisor).
            if buf_len == 0 {
                let supervisor = caller_has_supervisor(caller_id);
                return Ok(crate::cell::state_stash::status(
                    reader, supervisor, key as u64,
                ));
            }
            validate_user_buf(buf_ptr, buf_len, crate::cell::state_stash::MAX_STASH_LEN)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let n = crate::cell::state_stash::restore(reader, key as u64, &mut kbuf);
            if n > 0 {
                write_user_slice(
                    caller_id,
                    buf_ptr,
                    &kbuf[..n],
                    crate::cell::state_stash::MAX_STASH_LEN,
                )?;
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::milestone("state-restored", caller_id);
            }
            Ok(n)
        }
        // 412: StateStashClear — owner (open), sealed destination (consume) or a
        // supervisor (abort). Absent keys succeed; a foreign entry is refused.
        Syscall::StateStashClear { key } => {
            if key as u64 == SPAWN_ARGV_KEY {
                crate::cell::state_stash::discard_spawn_argv(caller_id);
            } else {
                let caller = stash_incarnation(caller_id)?;
                let supervisor = caller_has_supervisor(caller_id);
                if !crate::cell::state_stash::clear(caller, supervisor, key as u64) {
                    return Err(SyscallError::PermissionDenied);
                }
            }
            Ok(0)
        }

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
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let ready = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|s| s.tasks.get(&target_tid).map(|t| t.hotswap_ready));
            match ready {
                Some(true) => Ok(1),
                Some(false) => Ok(0),
                None => Err(SyscallError::FileNotFound), // tid does not exist
            }
        }

        Syscall::SpawnReplacement {
            old_tid,
            path_ptr,
            path_len,
        } => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if old_tid == 0 || path_len == 0 || path_len > crate::loader::disk_layout::MAX_CELL_PATH
            {
                return Err(SyscallError::InvalidInput);
            }
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            if !path_str.starts_with('/') {
                return Err(SyscallError::InvalidInput);
            }
            let profile = authorize_launch_edge(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Path,
                &path_str,
            )?;
            let replacement = crate::cell::hotswap::reserve_frozen_replacement(old_tid)
                .ok_or(SyscallError::PermissionDenied)?;
            let old_incarnation = stash_incarnation(old_tid)?;
            let request =
                match governed_replacement_request(caller_id, profile.child_ceiling, replacement) {
                    Ok(request) => request,
                    Err(error) => {
                        crate::task::dir_inherit::clear_staged(caller_id);
                        return Err(error);
                    }
                };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            // Mirror the regular spawn contract: a successful replacement spawn
            // consumes any staged directory set, and a failed one must clear it
            // so it cannot attach to an unrelated later child.
            crate::task::dir_inherit::clear_staged(caller_id);
            let task_id = spawned.map_err(|e| match e {
                types::ViError::NotFound => SyscallError::FileNotFound,
                types::ViError::OutOfMemory => {
                    log::warn!(
                        "[loader] spawn OOM: op=SpawnReplacement caller={} old_tid={} path={}",
                        caller_id,
                        old_tid,
                        path_str
                    );
                    SyscallError::OutOfMemory
                }
                types::ViError::PermissionDenied => SyscallError::PermissionDenied,
                _ => SyscallError::InvalidInput,
            })?;
            // Hand the frozen incarnation's stash entries to exactly this
            // replacement: the old writer is fenced, only the new one may read.
            if let Ok(new_incarnation) = stash_incarnation(task_id) {
                crate::cell::state_stash::seal_for_replacement(old_incarnation, new_incarnation);
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::milestone("replacement-sealed", task_id);
            }
            Ok(task_id)
        }

        // ── Driver Cell Registration (P00) ───────────────────────────────────

        // 416: RegisterBlockDriver — bind the caller's owned block device route.
        Syscall::RegisterBlockDriver { kind, id, reserved } => {
            register_block_device_route(caller_id, kind, id, reserved)
        }

        // 417: RegisterNicDriver — announce caller as the active NIC driver.
        Syscall::RegisterNicDriver => register_driver_service(
            caller_id,
            api::syscall::service::NIC_DRIVER,
            crate::task::drivers::driver_cell::register_nic_driver,
            crate::task::drivers::driver_cell::deregister_nic_driver,
        ),

        // 423: RegisterUsbHidProducer — announce caller as the USB HID event
        // producer, publishing service::USB_HID_PRODUCER for the input service's
        // producer gate. Requires UsbDriverCap and never touches the singleton
        // NIC role, so an xHCI cell on a PC with e1000/virtio-net cannot steal
        // the network route.
        Syscall::RegisterUsbHidProducer => register_usb_hid_producer_role(caller_id),

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
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !caller_pci_query_allowed(caller_id, |program| {
                crate::catalog::pci_class_allowed(program, class, subclass, prog_if)
            }) {
                return Err(SyscallError::PermissionDenied);
            }
            let usable = |bdf| pcie_usable_by(caller_id, bdf);
            match crate::task::drivers::pcie_ecam::find_class_where(
                class, subclass, prog_if, usable,
            ) {
                None => Ok(0), // device not present → VirtIO fallback
                Some(dev) => offer_pcie_device(caller_id, out_ptr, dev),
            }
        }

        // 424: FindPcieDeviceByVendor — query ECAM table for an exact
        // (vendor_id, device_id). Same hand-out semantics as 418 (PcieDriverCap,
        // BDF ownership, 48-byte `PcieDeviceInfo`), but the match names the
        // device, so two Driver Cells that share a class triple each claim their
        // own controller without a sibling-exit race.
        Syscall::FindPcieDeviceByVendor {
            vendor_id,
            device_id,
            out_ptr,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !caller_pci_query_allowed(caller_id, |program| {
                crate::catalog::pci_id_allowed(program, vendor_id, device_id)
            }) {
                return Err(SyscallError::PermissionDenied);
            }
            let usable = |bdf| pcie_usable_by(caller_id, bdf);
            match crate::task::drivers::pcie_ecam::find_vendor_device(vendor_id, device_id, usable)
            {
                None => Ok(0),
                Some(dev) => offer_pcie_device(caller_id, out_ptr, dev),
            }
        }

        // 434/435 (ADR-0031 CP1 §3–§4): the owned function's record, and its
        // MSI-X/MSI vectors. PcieDriverCap plus BDF ownership (418/424) gate both.
        Syscall::QueryPcieFunction {
            bdf,
            out_ptr,
            out_len,
            reserved,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if out_len != api::pcie_function::PCIE_FUNCTION_RECORD_LEN || reserved != 0 {
                return Err(SyscallError::InvalidInput);
            }
            let record = query_pcie_function(caller_id, bdf)?;
            write_user_slice(caller_id, out_ptr, &record, record.len())
                .map_err(|_| SyscallError::InvalidInput)?;
            Ok(0)
        }
        Syscall::AllocPcieIrq {
            bdf,
            want,
            mode,
            reserved,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if reserved != 0 {
                return Err(SyscallError::InvalidInput);
            }
            alloc_pcie_irq(caller_id, bdf, want, mode)
        }
        Syscall::SystemPower { action, a1, a2, a3 } => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if a1 != 0 || a2 != 0 || a3 != 0 {
                return Err(SyscallError::InvalidInput);
            }
            system_power(caller_id, action)
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
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let Some((base, irq)) = serial_port::declared(index) else {
                return Ok(0);
            };
            let present = serial_port::present(index);
            let info = [base.to_le_bytes(), [irq, present as u8]].concat();
            write_user_slice(caller_id, out_ptr, &info, info.len())?;
            Ok(present as usize)
        }
        Syscall::SerialWrite { index, ptr, len } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !serial_port::present(index) {
                return Ok(usize::MAX);
            }
            // Bound one call: a cell cannot hold the console path for an
            // unbounded burst through a blocking port.
            const SERIAL_BURST_MAX: usize = 4096;
            let len = len.min(SERIAL_BURST_MAX);
            if len == 0 {
                return Ok(0);
            }
            let view = caller_copy_view(caller_id)?;
            let mut buf = [0u8; SERIAL_BURST_MAX];
            let buf = &mut buf[..len];
            view.read_into(ptr, buf)
                .map_err(|_| SyscallError::InvalidInput)?;
            let mut written = 0usize;
            for byte in buf.iter() {
                if !serial_port::write_byte(index, *byte) {
                    break;
                }
                written += 1;
            }
            Ok(written)
        }
        Syscall::SerialRead { index, ptr, len } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !serial_port::present(index) {
                return Ok(usize::MAX);
            }
            const SERIAL_BURST_MAX: usize = 4096;
            let len = len.min(SERIAL_BURST_MAX);
            if len == 0 {
                return Ok(0);
            }
            let mut buf = [0u8; SERIAL_BURST_MAX];
            let mut read = 0usize;
            while read < len {
                let Some(byte) = serial_port::read_byte(index) else {
                    break;
                };
                buf[read] = byte;
                read += 1;
            }
            write_user_slice(caller_id, ptr, &buf[..read], len)?;
            Ok(read)
        }
        Syscall::SerialConfigure { index, baud } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !serial_port::present(index) {
                return Ok(2);
            }
            if serial_port::configure_baud(index, baud) {
                Ok(0)
            } else {
                Ok(3)
            }
        }

        // 234: WaitIrq — block until hardware IRQ fires (Driver Cell).
        // ISR calls irq_wait::signal_irq (atomic only; no lock, no scheduler access).
        // Scheduler sweep (pick_next) does the actual Ready transition.
        Syscall::WaitIrq { irq, mmio_base } => {
            if !caller_has_pcie_driver(caller_id)
                && !caller_has_platform(caller_id)
                && !caller_has_usb_driver(caller_id)
            {
                return Err(SyscallError::PermissionDenied);
            }
            #[cfg(target_arch = "aarch64")]
            {
                use hal::aarch64::backend::{ArmIrqController, Board};
                if let Some(usb_irq) = <Board as ArmIrqController>::USB_ONE_SHOT_IRQ {
                    if caller_has_usb_driver(caller_id) && irq == usb_irq as u8 {
                        // One-shot unmask: re-enable the USB legacy IRQ after the
                        // driver has cleared GINTSTS.
                        <Board as ArmIrqController>::unmask_usb_irq();
                    }
                }
            }
            // The ISR dereferences a stored nonzero base (VirtIO InterruptACK),
            // so only an MMIO window the caller was granted is accepted.
            if !wait_irq_base_permitted(caller_id, mmio_base) {
                return Err(SyscallError::PermissionDenied);
            }
            // x86 IRQ numbers are MSI vectors from 435; a cell waits only on its
            // own, so it can neither steal nor drain another driver's interrupts.
            #[cfg(target_arch = "x86_64")]
            if !crate::task::drivers::pcie_msi::irq_owned_by(irq, caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // Lost-wakeup guard: if IRQ already fired before this call, return immediately.
            if crate::task::drivers::irq_wait::take_pending(irq) {
                return Ok(0);
            }
            // Single-waiter policy: second caller on same IRQ gets TryAgain.
            if !crate::task::drivers::irq_wait::register_waiter(irq, caller_id, mmio_base) {
                return Err(SyscallError::TryAgain);
            }
            // Park the task — scheduler sweep will wake it when IRQ_PENDING[irq] is set.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                    task.state = super::tcb::TaskState::WaitIrq { irq };
                }
            }
            super::yield_cpu();
            Ok(0)
        }

        // 235: RegisterPcieBar — Platform Cell announces a discovered PCIe BAR.
        // Populates the MMIO allowlist only. BDF ownership belongs to the Driver
        // Cell that claims the device through FindPcieDevice.
        Syscall::RegisterPcieBar { bdf: _, base, len } => {
            if !caller_has_platform(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::resource_registry::register_pcie_bar(base, len);
            // Any RegisterPcieBar call from the Platform Cell marks it as active;
            // Phase 08 uses this to gate out the kernel ECAM scan.
            crate::task::drivers::pcie_ecam::mark_platform_registered();
            Ok(0)
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
        } => {
            if !caller_has_platform(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            use crate::task::drivers::pcie_ecam::{register_device, Registration};
            let registration = register_device(bdf, cls, bar0_base, bar0_size);
            crate::task::drivers::pcie_ecam::mark_platform_registered();
            crate::task::drivers::iommu::try_deferred_init();
            match registration {
                Registration::Recorded => Ok(0),
                Registration::RecordedWithoutDriver => Ok(1),
                Registration::Absent => Err(SyscallError::FileNotFound),
            }
        }

        // 237: ReadLog — drain the user-log (source 0) or audit (source 1) ring
        // into a cell-provided buffer. The caller must have allowlist bit 54
        // (ReadLog) set in its manifest; the audit ring also needs SupervisorCap.
        // Returns the number of bytes actually copied (0 when nothing available).
        Syscall::ReadLog {
            buf_ptr,
            max,
            source,
        } => {
            // Per-call staging cap: the user-log ring holds 8 KiB, so more is
            // zeroed heap that can never be filled.
            let cap = match source {
                0 => 8192,
                1 => {
                    if !caller_has_supervisor(caller_id) {
                        return Err(SyscallError::PermissionDenied);
                    }
                    65536
                }
                _ => return Err(SyscallError::InvalidInput),
            };
            if max == 0 {
                return Ok(0);
            }
            if buf_ptr == 0 {
                return Err(SyscallError::InvalidInput);
            }
            let max = max.min(cap);
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(max)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(max, 0);
            if source == 0 {
                let n = crate::task::read_log_ring(&mut kbuf);
                if n > 0 {
                    write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
                }
                return Ok(n);
            }
            // Audit records leave the ring only once delivered: a faulting
            // buffer must not destroy the evidence it was meant to receive.
            let (start, n) = crate::audit::peek(&mut kbuf);
            if n > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
                crate::audit::consume(start, n);
            }
            Ok(n)
        }

        Syscall::BlkFlush => {
            if !caller_has_block_io(caller_id) {
                log::warn!(
                    "BlkFlush denied: task {} lacks block-I/O capability",
                    caller_id
                );
                return Err(SyscallError::PermissionDenied);
            }

            match crate::task::drivers::block::flush() {
                Ok(()) => Ok(1),
                Err(_) => Ok(0),
            }
        }
        Syscall::Shutdown => {
            authorize_raw_shutdown(caller_id)?;
            #[cfg(all(feature = "test-hooks", target_arch = "aarch64"))]
            crate::qemu_exit(true);

            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            crate::qemu_exit(true);

            #[cfg(all(not(feature = "test-hooks"), target_arch = "riscv64"))]
            unsafe {
                // SAFETY: ecall traps to OpenSBI which powers off QEMU; no return.
                core::arch::asm!(
                    "li a7, 0x53525354", // SBI_EXT_SRST
                    "li a6, 0",          // fid = SYSTEM_RESET
                    "li a0, 0",          // reset_type = Shutdown
                    "li a1, 0",          // reset_reason = NoReason
                    "ecall",
                    options(noreturn)
                );
            }
            #[cfg(all(not(feature = "test-hooks"), target_arch = "aarch64"))]
            unsafe {
                // PSCI SYSTEM_OFF via HVC (QEMU virt machine default)
                core::arch::asm!(
                    "mov x0, #0x0008",
                    "movk x0, #0x8400, lsl #16",
                    "hvc #0",
                    options(noreturn)
                );
            }
            #[cfg(target_arch = "x86_64")]
            loop {
                unsafe {
                    core::arch::asm!("hlt", options(nomem, nostack));
                }
            }
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            loop {
                unsafe {
                    core::arch::asm!("wfi", options(nomem, nostack));
                }
            }
        }
        Syscall::BlkRead { sector, buf_ptr } => {
            if !caller_has_block_io(caller_id) {
                log::warn!(
                    "BlkRead denied: task {} lacks block-I/O capability",
                    caller_id
                );
                return Err(SyscallError::PermissionDenied);
            }
            // Per-cell partition range gate — a runaway FAT offset must never
            // reach kernel-owned LBAs (P2 cell table, P3 snapshot). Returns 0 = failure.
            if !check_block_access(caller_id, sector, 1) {
                return Ok(0);
            }
            validate_user_buf(buf_ptr, 512, MAX_USER_BUF)?;

            // Bounce buffer: VirtioHal::share() treats the buffer's virtual address
            // as its physical address (identity-map assumption). Stack frames ARE
            // identity-mapped; ELF BSS/heap pages are NOT — DMA would land at the
            // wrong physical address without the bounce. Read into an on-stack buffer
            // (always identity-mapped), then copy to the user buffer under SUM=1.
            let mut bounce = [0u8; 512];
            match crate::task::drivers::block::read_sector(sector, &mut bounce) {
                Ok(()) => {
                    write_user_slice(caller_id, buf_ptr, &bounce, MAX_USER_BUF)?;
                    Ok(1)
                }
                Err(_) => Ok(0),
            }
        }
        Syscall::BlkWrite { sector, buf_ptr } => {
            if !caller_has_block_io(caller_id) {
                log::warn!(
                    "BlkWrite denied: task {} lacks block-I/O capability",
                    caller_id
                );
                return Err(SyscallError::PermissionDenied);
            }
            // Per-cell partition range gate — prevents a cell from corrupting
            // the loader's table or the snapshot region. Returns 0 = failure.
            if !check_block_access(caller_id, sector, 1) {
                return Ok(0);
            }
            let user = read_user_slice(caller_id, buf_ptr, 512, MAX_USER_BUF)?;
            let mut bounce = [0u8; 512];
            bounce.copy_from_slice(&user);
            match crate::task::drivers::block::write_sector(sector, &bounce) {
                Ok(()) => Ok(1),
                Err(error) => {
                    log::warn!("[blk] write LBA={} failed: {:?}", sector, error);
                    Ok(0)
                }
            }
        }
        Syscall::HotSwapReady => {
            // The new cell signals that it has finished deserializing state.
            // Set the per-task flag; the supervisor cutover path polls this.
            // No SpawnCap required — only the new cell itself calls this
            // after its restore flow finishes.
            crate::cell::hotswap::set_task_hotswap_ready(caller_id);
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::milestone("hotswap-ready", caller_id);
            Ok(0)
        }

        Syscall::Snapshot => {
            if !caller_has_supervisor(caller_id) {
                log::warn!("[snapshot] denied: caller {caller_id} has no SupervisorCap");
                return Err(SyscallError::PermissionDenied);
            }
            // Cells must be quiesced before calling this (all at yield points).
            // For MVP: the shell is the only active task while the snapshot runs.
            match crate::snapshot::serialize_snapshot() {
                Ok(frame_count) => Ok(frame_count as usize),
                Err(reason) => {
                    log::warn!("[snapshot] unavailable: {reason}");
                    Err(SyscallError::Unknown)
                }
            }
        }

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
        } => {
            #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
            {
                const MAX_DIMENSION: usize = 8192;
                const PAGE_SIZE: usize = 4096;
                // AArch64: only a board with a firmware display path registers a
                // framebuffer; any other board refuses before every other check.
                #[cfg(target_arch = "aarch64")]
                let Some(display) =
                    <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::DISPLAY
                else {
                    let _ = (base, size, packed_dimensions, pitch);
                    return Err(SyscallError::Unknown);
                };
                if !caller_has_mmio_device(caller_id, crate::resource_registry::DEV_DISPLAY) {
                    return Err(SyscallError::PermissionDenied);
                }
                #[cfg(target_arch = "aarch64")]
                if !crate::resource_registry::owns_exact_mmio(
                    caller_id,
                    display.mailbox.base,
                    display.mailbox.size,
                ) {
                    return Err(SyscallError::PermissionDenied);
                }
                if base == 0
                    || size == 0
                    || base & (PAGE_SIZE - 1) != 0
                    || packed_dimensions > u32::MAX as usize
                {
                    return Err(SyscallError::InvalidInput);
                }
                let width = packed_dimensions >> 16;
                let height = packed_dimensions & 0xffff;
                if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                    return Err(SyscallError::InvalidInput);
                }
                let minimum_pitch = width.checked_mul(4).ok_or(SyscallError::InvalidInput)?;
                if pitch < minimum_pitch || pitch & 3 != 0 {
                    return Err(SyscallError::InvalidInput);
                }
                let covered = pitch
                    .checked_mul(height)
                    .ok_or(SyscallError::InvalidInput)?;
                let end = base.checked_add(size).ok_or(SyscallError::InvalidInput)?;
                #[cfg(target_arch = "aarch64")]
                if covered > size || end > display.framebuffer_limit {
                    return Err(SyscallError::InvalidInput);
                }
                #[cfg(not(target_arch = "aarch64"))]
                {
                    if covered > size {
                        return Err(SyscallError::InvalidInput);
                    }
                }
                // The framebuffer is MMIO: it must not overlap *any* managed frame,
                // not merely sit above one end of the allocator's memory.
                //
                // `match` rather than `?`: the early return has to happen outside the
                // `guard` borrow, or the RPi3 build refuses the guard's lifetime
                // (E0597) with the borrowed allocator still in scope.
                let overlaps_managed = {
                    let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
                    match guard.as_ref() {
                        Some(allocator) => allocator.managed_ranges().any(|range| {
                            let range_end = range.start + range.frames * PAGE_SIZE;
                            base < range_end && range.start < end
                        }),
                        None => return Err(SyscallError::Unknown),
                    }
                };
                if overlaps_managed {
                    return Err(SyscallError::InvalidInput);
                }
                let rounded = size
                    .checked_add(PAGE_SIZE - 1)
                    .ok_or(SyscallError::InvalidInput)?
                    & !(PAGE_SIZE - 1);
                let pages = rounded / PAGE_SIZE;
                let owner = types::CellId(caller_id as u64);
                match crate::resource_registry::reserve_display_framebuffer(
                    owner,
                    base,
                    size,
                    width as u16,
                    height as u16,
                    pitch,
                ) {
                    Ok(crate::resource_registry::DisplayFramebufferReservation::ActiveReplay) => {
                        return Ok(0);
                    }
                    Err(crate::resource_registry::DisplayFramebufferReservationError::Conflict) => {
                        return Err(SyscallError::PermissionDenied);
                    }
                    Ok(crate::resource_registry::DisplayFramebufferReservation::Reserved) => {}
                }
                if crate::memory::paging::map_display_framebuffer_user(base, pages).is_err() {
                    crate::resource_registry::cancel_display_framebuffer(
                        owner,
                        base,
                        size,
                        width as u16,
                        height as u16,
                        pitch,
                    );
                    return Err(SyscallError::Unknown);
                }
                if crate::resource_registry::activate_display_framebuffer(
                    owner,
                    base,
                    size,
                    width as u16,
                    height as u16,
                    pitch,
                ) {
                    #[cfg(target_arch = "x86_64")]
                    crate::task::drivers::fb_console::handover();
                    Ok(0)
                } else {
                    // This cannot be recovered safely: pages are firmware-owned
                    // and remain mapped until reboot, so display stays fail-stop.
                    Err(SyscallError::Unknown)
                }
            }
            #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
            {
                let _ = (base, size, packed_dimensions, pitch);
                Err(SyscallError::Unknown)
            }
        }

        Syscall::GrantRegister { size } => grant::grant_register(caller_id, size),
        Syscall::GrantUnregister { reg_id } => grant::grant_unregister(caller_id, reg_id),

        Syscall::WaitForEvent { mask, deadline } => {
            // Lost-wakeup guard: check pending events BEFORE parking.
            let already = super::waker::consume_pending(mask);
            if already != 0 {
                return Ok(already as usize);
            }
            // Park: set WaitEvent state so the timer sweep can wake this task.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                    task.state = super::tcb::TaskState::WaitEvent { mask, deadline };
                }
            }
            // Yield; wake happens in pick_next's global sweep (hart 0).
            super::yield_cpu();
            // After re-schedule: timer sweep wrote the fired mask into trap_frame.regs[10].
            // Return it so ViCell_syscall_dispatch writes the correct value back.
            let fired = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|s| s.tasks.get(&caller_id))
                .map(|t| t.trap_frame.regs[10])
                .unwrap_or(0);
            #[cfg(target_arch = "riscv32")]
            let fired = fired as usize;
            Ok(fired)
        }

        Syscall::WaitCompletion {
            mask,
            deadline,
            out_ptr,
        } => super::completion_wait::wait_completion(caller_id, mask, deadline, out_ptr),

        Syscall::RequestMmio { base, len } => {
            // PlatformCap bypass: the Platform Cell may claim only the kernel-admitted
            // ECAM config-space window (not in PCIE_BARS), never managed RAM — the
            // claim is user-mapped. Overlap check still applies.
            // x86 SAS: the boot identity map covers MMIO kernel-only; a granted
            // window must gain PTE_USER (+PCD, NX) before the ring-3 cell touches
            // it. BAR windows may not be mapped at all yet — this also creates
            // the identity mapping.
            #[cfg(target_arch = "x86_64")]
            fn user_map(base: usize, len: usize) {
                crate::memory::paging::map_mmio_user_x86(base, len);
            }
            // RISC-V/AArch64 make an allowlisted window reachable from EL0 with the
            // *boot* identity map, so a window whose access was revoked earlier —
            // by a capability revoke, or by the self-test that exercises one — stays
            // kernel-only unless the grant re-arms it. This is the non-x86 half of
            // the same rule: a granted window is a reachable window.
            #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
            fn user_map(base: usize, len: usize) {
                let _ = crate::memory::paging::grant_mmio_user(base, len);
            }
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            fn user_map(_base: usize, _len: usize) {}
            // x86: a PCIe BAR is user-mapped without its MSI-X table/PBA pages
            // (ADR-0031 CP1 §4.3), so a cell cannot retarget its device's
            // interrupts by rewriting table entries.
            #[cfg(target_arch = "x86_64")]
            fn user_map_pcie_bar(base: usize, len: usize) {
                for (start, size) in crate::task::drivers::pcie_msi::user_ranges(base, len) {
                    crate::memory::paging::map_mmio_user_x86(start, size);
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            fn user_map_pcie_bar(base: usize, len: usize) {
                user_map(base, len);
            }

            if caller_has_platform(caller_id) {
                return match crate::resource_registry::request_ecam_mmio(caller_id, base, len) {
                    Ok(()) => {
                        user_map(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::PermissionDenied) => Ok(1),
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // PCIe BAR path: cells with PcieDriverCap may claim any BAR
            // discovered during ECAM scan (registered in resource_registry::PCIE_BARS).
            if caller_has_pcie_driver(caller_id) && crate::resource_registry::is_pcie_bar(base, len)
            {
                return match crate::resource_registry::request_mmio(
                    caller_id,
                    base,
                    len,
                    crate::resource_registry::DEV_PCIE,
                ) {
                    Ok(()) => {
                        user_map_pcie_bar(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::PermissionDenied) => Ok(1),
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // USB Host path: cells with UsbDriverCap may claim the DWC2 MMIO aperture.
            if caller_has_usb_driver(caller_id) && crate::resource_registry::is_dwc2_mmio(base, len)
            {
                return match crate::resource_registry::request_dwc2_mmio(caller_id, base, len) {
                    Ok(()) => {
                        user_map(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::PermissionDenied) => Ok(1),
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // GPIO/UART path: gate on manifest-declared device classes.
            let allowed_devices = {
                let sched = super::SCHEDULER.lock();
                sched
                    .as_ref()
                    .map_or(0, |s| s.cell_rights(caller_id).mmio_devices)
            };
            if allowed_devices == 0 {
                return Err(SyscallError::PermissionDenied);
            }
            match crate::resource_registry::request_mmio(caller_id, base, len, allowed_devices) {
                Ok(()) => {
                    user_map(base, len);
                    Ok(0)
                }
                Err(types::ViError::PermissionDenied) => {
                    log::warn!(
                        "[mmio] DENY caller={} base={:#x} len={:#x} allowed_devices={:#04x}",
                        caller_id,
                        base,
                        len,
                        allowed_devices
                    );
                    Ok(1)
                }
                Err(types::ViError::AlreadyExists) => Ok(2),
                Err(_) => Ok(3),
            }
        }

        // 442: ProbeVirtioMmio — the kernel reads a slot's identity registers so
        // a Driver Cell can pick its slot before claiming it; a claim is
        // exclusive and never released, so probing by claiming would lock
        // sibling drivers out of their slots.
        Syscall::ProbeVirtioMmio { base, reserved } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if reserved != (0, 0, 0) {
                return Err(SyscallError::InvalidInput);
            }
            #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
            {
                use crate::task::drivers::virtio_common::{probe_slot, SlotProbe};
                match probe_slot(base) {
                    SlotProbe::Identity(identity) => Ok(identity),
                    SlotProbe::NoTransport => Err(SyscallError::FileNotFound),
                    SlotProbe::NotASlot => Err(SyscallError::InvalidInput),
                }
            }
            // x86_64 reaches VirtIO over PCI: there is no MMIO slot to probe.
            #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
            {
                let _ = base;
                Err(SyscallError::NotSupported)
            }
        }

        Syscall::GetRandom { buf_ptr, len } => {
            if len == 0 {
                return Ok(0);
            }
            // Validate the ABI descriptor before entropy is consumed. `len`
            // remains the caller-declared capacity; only the output span caps
            // at 64 bytes, as required by the frozen syscall contract.
            validate_user_buf(buf_ptr, len, MAX_USER_BUF)?;
            let capped = len.min(64);
            preflight_user_output(caller_id, buf_ptr, capped)?;
            let mut kbuf = [0u8; 64];
            let written = crate::task::drivers::virtio_rng::get_random(&mut kbuf[..capped]);
            let n = if written > 0 {
                written
            } else {
                #[cfg(all(feature = "dev-weak-rng", not(feature = "qemu-dev-https")))]
                {
                    static WEAK_RNG_WARNED: core::sync::atomic::AtomicBool =
                        core::sync::atomic::AtomicBool::new(false);
                    if !WEAK_RNG_WARNED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                        log::warn!(
                            "[kernel] GetRandom: no hardware entropy — serving WEAK xorshift32 \
                             (dev-weak-rng). NOT cryptographically secure; never ship this."
                        );
                    }
                    let seed =
                        super::system_ticks() as u32 ^ (caller_id as u32).wrapping_mul(0x9e37_79b9);
                    let mut state = if seed == 0 { 1 } else { seed };
                    for byte in kbuf[..capped].iter_mut() {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        *byte = state as u8;
                    }
                    capped
                }
                // The QEMU HTTPS image must fail closed even when development
                // defaults also enabled dev-weak-rng through feature unification.
                #[cfg(any(not(feature = "dev-weak-rng"), feature = "qemu-dev-https"))]
                0
            };
            if n > 0 {
                write_getrandom_output(caller_id, buf_ptr, &kbuf[..n])?;
            }
            Ok(n)
        }

        Syscall::BlkReadAsync { sector, grant_id } => {
            if !caller_has_block_io(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !check_block_access(caller_id, sector, 1) {
                return Ok(0);
            }
            // Validate ownership and minimum size (must hold ≥ 512 bytes).
            let buf_paddr = {
                let tbl = grant_table_lock().lock();
                tbl.as_ref()
                    .and_then(|m| m.get(&grant_id))
                    .filter(|g| g.owner == caller_id && g.size >= 512)
                    .map(|g| g.base)
            };
            let buf_paddr = match buf_paddr {
                Some(p) => p,
                None => return Ok(0),
            };
            // Grant pages are identity-mapped (vaddr == paddr), so DMA addresses are correct.
            // SAFETY: buf_paddr is a physically contiguous, identity-mapped grant page; valid
            // for 512 bytes of VirtIO DMA read.

            let buf = unsafe { core::slice::from_raw_parts_mut(buf_paddr as *mut u8, 512) };
            match crate::task::drivers::block::read_sector(sector, buf) {
                Ok(()) => Ok(1), // async_id = 1 means immediately complete (Phase 04 for real async)
                Err(_) => Ok(0),
            }
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
use api::syscall::ViSyscall;

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
fn with_test_sum<T>(operation: impl FnOnce() -> T) -> T {
    let saved_sstatus: usize;
    unsafe {
        // SAFETY: test-only dispatch must reproduce the real trap entry's SUM state.
        core::arch::asm!(
            "csrr {saved_sstatus}, sstatus",
            "csrs sstatus, {sum}",
            saved_sstatus = out(reg) saved_sstatus,
            sum = in(reg) 0x40000_usize,
            options(nostack)
        );
    }
    let result = operation();
    if saved_sstatus & 0x40000 == 0 {
        unsafe {
            // SAFETY: restore exactly the caller hart's prior SUM bit.
            core::arch::asm!(
                "csrc sstatus, {sum}",
                sum = in(reg) 0x40000_usize,
                options(nostack)
            );
        }
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
        let hart_id = super::hart_local::current_hart_id();
        let cid = super::hart_local::ready::current_task_id_for(hart_id);
        if cid > 0 {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&cid) {
                    t.run_ticks = 0;
                    t.rt_overrun_warned = false;
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

    // SAFETY: csrs/csrc sstatus SUM (bit 18) enables S-mode access to user pages
    // for the duration of handle_syscall. Disabled immediately after to prevent
    // inadvertent user-page reads on subsequent kernel faults.
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x40000usize);
    }

    let supports_typed_oom = supports_typed_oom(&syscall);
    let typed_errors = has_typed_errors(&syscall);
    let pcie_function = is_pcie_function_call(&syscall);
    let power_call = matches!(syscall, Syscall::SystemPower { .. });
    let dma_export_call = is_dma_export_call(&syscall);
    let result = dispatch_admitted(caller_id, syscall);

    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
    }

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
        let hart_id = super::hart_local::current_hart_id();
        let cid = super::hart_local::ready::current_task_id_for(hart_id);
        if cid > 0 {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&cid) {
                    t.run_ticks = 0;
                    t.rt_overrun_warned = false;
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

    // SAFETY: csrs/csrc sstatus SUM (bit 18) — same bit position on RV32 and RV64
    // per the RISC-V Privileged Spec §4.1.1. Disabled after handle_syscall.
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x40000usize);
    }

    let supports_typed_oom = supports_typed_oom(&syscall);
    let typed_errors = has_typed_errors(&syscall);
    let result = dispatch_admitted(caller_id, syscall);

    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
    }

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
mod tests;
