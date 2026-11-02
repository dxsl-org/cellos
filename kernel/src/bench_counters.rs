//! Benchmark counters (`bench-counters` feature only; measurement, never shipped).
//!
//! Global relaxed counters that let the `bench` cell attribute a scenario's
//! cost: how many syscalls it made, how many CR3 writes the x86 domain syscall
//! path added, how many root programmings and context switches the scheduler
//! did, how many IPC payload bytes crossed the user boundary, and how many heap
//! allocations backed it all. The cell reads a [`snapshot`] through raw opcode
//! [`OPCODE`] before and after a scenario and prints the difference.
//!
//! Every count is `Relaxed`: the snapshot is a set of independent totals, not
//! a consistent cut, and a scenario delta is only meaningful over a quiet run.

use core::sync::atomic::{AtomicU64, Ordering};

/// Raw syscall number. Outside `ViSyscall` like 500–503, so no ABI enum change.
pub(crate) const OPCODE: usize = 504;

/// Record version, slot 0 of [`snapshot`].
pub(crate) const VERSION: u64 = 1;

/// Number of `u64` slots in the record.
pub(crate) const SLOTS: usize = 8;

/// Size in bytes of the little-endian record copied to the caller.
pub(crate) const RECORD_BYTES: usize = SLOTS * core::mem::size_of::<u64>();

static SYSCALLS: AtomicU64 = AtomicU64::new(0);
static DOMAIN_SYSCALL_CR3: AtomicU64 = AtomicU64::new(0);
static ROOT_PROGRAMMINGS: AtomicU64 = AtomicU64::new(0);
static CONTEXT_SWITCHES: AtomicU64 = AtomicU64::new(0);
static IPC_BYTES_COPIED: AtomicU64 = AtomicU64::new(0);
static HEAP_ALLOCS: AtomicU64 = AtomicU64::new(0);
static HEAP_ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

/// One trap into the syscall dispatcher.
#[inline]
pub(crate) fn syscall() {
    SYSCALLS.fetch_add(1, Ordering::Relaxed);
}

/// One x86 SYSCALL taken from a non-kernel root. The entry stub then writes
/// the kernel CR3 and the exit stub writes the user CR3 back: two CR3 writes
/// per count, neither of which `root_programmings` sees.
#[cfg(target_arch = "x86_64")]
#[inline]
pub(crate) fn domain_syscall_cr3() {
    DOMAIN_SYSCALL_CR3.fetch_add(1, Ordering::Relaxed);
}

/// One root (CR3/SATP/TTBR0) programming planned by a context switch.
#[inline]
pub(crate) fn root_programming() {
    ROOT_PROGRAMMINGS.fetch_add(1, Ordering::Relaxed);
}

/// One switch onto a task context.
#[inline]
pub(crate) fn context_switch() {
    CONTEXT_SWITCHES.fetch_add(1, Ordering::Relaxed);
}

/// IPC payload bytes copied across the user boundary.
#[inline]
pub(crate) fn ipc_bytes(len: usize) {
    IPC_BYTES_COPIED.fetch_add(len as u64, Ordering::Relaxed);
}

/// One successful kernel heap allocation of `size` payload bytes.
#[inline]
pub(crate) fn heap_alloc(size: usize) {
    HEAP_ALLOCS.fetch_add(1, Ordering::Relaxed);
    HEAP_ALLOC_BYTES.fetch_add(size as u64, Ordering::Relaxed);
}

/// The versioned record: `[version, syscalls, domain_syscall_cr3,
/// root_programmings, context_switches, ipc_bytes_copied, heap_allocs,
/// heap_alloc_bytes]`.
pub(crate) fn snapshot() -> [u64; SLOTS] {
    [
        VERSION,
        SYSCALLS.load(Ordering::Relaxed),
        DOMAIN_SYSCALL_CR3.load(Ordering::Relaxed),
        ROOT_PROGRAMMINGS.load(Ordering::Relaxed),
        CONTEXT_SWITCHES.load(Ordering::Relaxed),
        IPC_BYTES_COPIED.load(Ordering::Relaxed),
        HEAP_ALLOCS.load(Ordering::Relaxed),
        HEAP_ALLOC_BYTES.load(Ordering::Relaxed),
    ]
}

// Retirement timing (Phase 03 P08). The Exit syscall may not log or lock while
// the exiting Cell is still charged, so it only stores its tid and time here;
// the later DMA teardown, which may log, pairs them. One slot: a concurrent
// exit overwrites it and the pair then reports the exit time as unknown.
const NO_TID: u64 = u64::MAX;
static LAST_EXIT_TID: AtomicU64 = AtomicU64::new(NO_TID);
static LAST_EXIT_NS: AtomicU64 = AtomicU64::new(0);

/// The calling task entered the Exit syscall. Atomics only: no log, lock or
/// allocation on the victim-attributed path.
pub(crate) fn note_exit(tid: usize) {
    let Some(ns) = crate::task::telemetry::monotonic_ns() else {
        return;
    };
    LAST_EXIT_TID.store(NO_TID, Ordering::Release);
    LAST_EXIT_NS.store(ns, Ordering::Release);
    LAST_EXIT_TID.store(tid as u64, Ordering::Release);
}

/// Exit time of `tid` if the slot still holds its unbroken record.
fn exit_ns(tid: usize) -> Option<u64> {
    let before = LAST_EXIT_TID.load(Ordering::Acquire);
    let ns = LAST_EXIT_NS.load(Ordering::Acquire);
    let after = LAST_EXIT_TID.load(Ordering::Acquire);
    (before == tid as u64 && after == before).then_some(ns)
}

/// Serial milestone `[bench-ts] <event> tid=<tid> ns=<now> exit_ns=<ns|unknown>`
/// on the `GetTime(0)` clock. Phase 03 events: `dma-retired`, `block-ready`
/// (P08 driver retirement and restart) and `state-stashed`,
/// `replacement-sealed`, `state-restored`, `hotswap-ready` (P09 replacement).
/// `exit_ns` is the tid's own Exit-syscall time when still recorded, so a
/// retirement span is computed from one guest clock; live tasks print unknown.
pub(crate) fn milestone(event: &str, tid: usize) {
    let Some(now) = crate::task::telemetry::monotonic_ns() else {
        return;
    };
    match exit_ns(tid) {
        Some(exit) => log::warn!(
            "[bench-ts] {} tid={} ns={} exit_ns={}",
            event,
            tid,
            now,
            exit
        ),
        None => log::warn!(
            "[bench-ts] {} tid={} ns={} exit_ns=unknown",
            event,
            tid,
            now
        ),
    }
}

/// [`snapshot`] encoded as the little-endian bytes the caller receives.
pub(crate) fn snapshot_bytes() -> [u8; RECORD_BYTES] {
    let mut out = [0u8; RECORD_BYTES];
    for (chunk, value) in out.chunks_exact_mut(8).zip(snapshot()) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_counters_record_layout_and_version() {
        assert_eq!(OPCODE, 504);
        assert_eq!(RECORD_BYTES, 64);
        let before = snapshot();
        assert_eq!(before[0], VERSION);
        assert_eq!(VERSION, 1);

        syscall();
        root_programming();
        context_switch();
        ipc_bytes(64);
        heap_alloc(24);
        let after = snapshot();
        assert_eq!(after[0], VERSION);
        // Other tests run concurrently and may add to the same totals, so
        // each slot moved by at least this test's own increments.
        assert!(after[1] > before[1]);
        assert!(after[3] > before[3]);
        assert!(after[4] > before[4]);
        assert!(after[5] >= before[5] + 64);
        assert!(after[6] > before[6]);
        assert!(after[7] >= before[7] + 24);

        let bytes = snapshot_bytes();
        assert_eq!(u64::from_le_bytes(bytes[..8].try_into().unwrap()), VERSION);
    }
}
