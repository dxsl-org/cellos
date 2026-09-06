//! Kernel-owned supervisor allocation registry for private-root snapshots.
//!
//! A range has an owner and lifetime token. Builders may snapshot only shared
//! kernel ranges; selected stacks and private tables stay owner-scoped.
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::{memory::paging::PAGE_SIZE, sync::Spinlock, PhysAddr};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SupervisorRangeKind {
    KernelHeap,
    KernelStack,
    PrivatePageTable,
    StaticText,
    StaticReadOnly,
    StaticWritable,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SupervisorRangeOwner {
    SharedKernel,
    TaskStack,
    AddressSpace(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SupervisorRangeId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SupervisorRange {
    pub(crate) id: SupervisorRangeId,
    pub(crate) start: PhysAddr,
    pub(crate) end: PhysAddr,
    pub(crate) kind: SupervisorRangeKind,
    pub(crate) owner: SupervisorRangeOwner,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static RANGES: Spinlock<Vec<SupervisorRange>> = Spinlock::new(Vec::new());

unsafe extern "C" {
    static __domain_text_start: u8;
    static __domain_text_end: u8;
    static __domain_readonly_start: u8;
    static __domain_readonly_end: u8;
    static __domain_writable_start: u8;
    static __domain_writable_end: u8;
}

/// Enable dynamic registrations after allocation-sensitive boot self-tests.
pub(crate) fn activate() {
    ACTIVE.store(true, Ordering::Release);
}

pub(crate) fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Register the linker-bounded supervisor image after boot self-tests finish.
pub(crate) fn register_static_image() -> Result<(), ()> {
    let ranges = [
        (
            core::ptr::addr_of!(__domain_text_start) as usize,
            core::ptr::addr_of!(__domain_text_end) as usize,
            SupervisorRangeKind::StaticText,
        ),
        (
            core::ptr::addr_of!(__domain_readonly_start) as usize,
            core::ptr::addr_of!(__domain_readonly_end) as usize,
            SupervisorRangeKind::StaticReadOnly,
        ),
        (
            core::ptr::addr_of!(__domain_writable_start) as usize,
            core::ptr::addr_of!(__domain_writable_end) as usize,
            SupervisorRangeKind::StaticWritable,
        ),
    ];
    let mut ids = Vec::with_capacity(ranges.len());
    for (start, end, kind) in ranges {
        match register(start, end, kind, SupervisorRangeOwner::SharedKernel) {
            Ok(id) => ids.push(id),
            Err(()) => {
                for id in ids {
                    let unregistered = unregister(id);
                    debug_assert!(unregistered);
                }
                return Err(());
            }
        }
    }
    Ok(())
}

/// Register one kernel-owned page-aligned range.
pub(crate) fn register(
    start: PhysAddr,
    end: PhysAddr,
    kind: SupervisorRangeKind,
    owner: SupervisorRangeOwner,
) -> Result<SupervisorRangeId, ()> {
    if !is_active()
        || start >= end
        || !start.is_multiple_of(PAGE_SIZE)
        || !end.is_multiple_of(PAGE_SIZE)
    {
        return Err(());
    }
    let mut ranges = RANGES.lock();
    if ranges
        .iter()
        .any(|range| start < range.end && range.start < end)
    {
        return Err(());
    }
    let id = SupervisorRangeId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    ranges.push(SupervisorRange {
        id,
        start,
        end,
        kind,
        owner,
    });
    Ok(id)
}

/// Remove one dynamic registration after every root/hart reference is gone.
pub(crate) fn unregister(id: SupervisorRangeId) -> bool {
    let mut ranges = RANGES.lock();
    let Some(index) = ranges.iter().position(|range| range.id == id) else {
        return false;
    };
    ranges.remove(index);
    true
}

/// Snapshot only ranges shared by every private root.
pub(crate) fn shared_snapshot() -> Vec<SupervisorRange> {
    RANGES
        .lock()
        .iter()
        .copied()
        .filter(|range| range.owner == SupervisorRangeOwner::SharedKernel)
        .collect()
}

#[allow(dead_code)]
pub(crate) fn contains_shared_kind(kind: SupervisorRangeKind) -> bool {
    RANGES
        .lock()
        .iter()
        .any(|range| range.owner == SupervisorRangeOwner::SharedKernel && range.kind == kind)
}
