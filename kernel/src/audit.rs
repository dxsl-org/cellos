//! Kernel audit ring buffer — 256 KB in-memory event log.
//!
//! Records key kernel events (IPC, file, net, spawn, fault, exit) with a
//! monotonic timestamp for post-mortem analysis.  A background Cell (or shell
//! builtin) drains the ring and appends to `/data/kernel.log`.
//!
//! # Concurrency
//! Two things can make a write concurrent with another producer: the timer ISR
//! preempting a syscall-context `log_event()` call on the same hart, and a
//! second hart logging at the same time.  S-mode interrupts are disabled for
//! the duration (which also makes the lock below unreachable from an ISR, so it
//! cannot self-deadlock), and `WRITE` serialises the hart-to-hart case: without
//! it two harts read the same `head`, write into the same byte range, and store
//! the same advanced `head`, so one record is silently overwritten.
//!
//! # Overflow
//! When the ring is full, new writes are dropped and `DROPPED` is incremented.
//! No blocking, no corruption.
//!
//! # Record format
//! ```
//! [u64 mtime_ticks LE][u8 event_type][u8 payload_len][payload…]
//! ```
//! Total minimum: 10 bytes per event.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

const BUF_SIZE: usize = 256 * 1024; // must be a power of two
const MASK: usize = BUF_SIZE - 1;

/// Serialises producers. Taken with S-mode interrupts already disabled, so a
/// same-hart ISR can never wait on it and a holder never blocks an interrupt.
static WRITE: crate::sync::Spinlock<()> = crate::sync::Spinlock::new(());

/// Kernel audit event type byte.
#[repr(u8)]
#[allow(dead_code)] // reason: variants logged by different kernel subsystems
pub enum AuditEvent {
    IpcSend = 1,
    IpcRecv = 2,
    FileOpen = 3,
    FileWrite = 4,
    NetTx = 5,
    NetRx = 6,
    CellSpawn = 7,
    CellFault = 8,
    CellExit = 9,
    CellSpawnDenied = 10,
    /// An RT-priority cell's `RecvTimeout` deadline elapsed before its awaited
    /// message arrived — a missed control-loop cycle. Payload: `encode_u32x2(cell_id,
    /// cumulative_miss_count)`. Observability only (no enforcement): makes RT misses
    /// visible for post-mortem / tuning once real-hardware bench data is available.
    RtDeadlineMiss = 11,
    /// An RT-priority cell crossed the CPU-monopoly *warning* threshold (a fraction
    /// of the watchdog budget) without yielding — an early signal BEFORE the hard
    /// watchdog kill. Payload: `encode_u32x2(cell_id, run_ticks)`. One-shot per episode.
    RtCpuOverrun = 12,
    /// A cell that opted into liveness heartbeating missed its deadline — a silent hang
    /// (deadlock / stuck loop) the CPU watchdog cannot detect. The kernel terminates it
    /// for supervised restart. Payload: `encode_u32x2(cell_id, tid)`.
    CellHung = 13,
    /// A cell invoked a syscall not present in its `__ViCell_syscalls` allowlist.
    /// Payload: `encode_u32x2(caller_tid, allowlist_bit)`.
    SyscallDenied = 14,
    /// A cell's ELF image was measured at spawn (integrity measurement, IMA-style).
    /// Payload: `encode_u32x2(tid, sha256_prefix_le_u32)`. The full digest and the
    /// rolling aggregate live in [`crate::measurement_log`].
    CellMeasure = 15,
    /// The signed operator policy was loaded + verified at boot (P5).
    /// Payload: `encode_u32x2(entry_count, 0)`.
    PolicyLoaded = 16,
    /// The operator policy failed signature verification or parsing — fail-closed.
    /// Payload: `encode_u32x2(reason_code, 0)`.
    PolicyInvalid = 17,
    /// No operator policy present in VIFS1. Payload: `encode_u32x2(0, 0)`.
    PolicyAbsent = 18,
    /// Operator policy narrowed a cell's spawn-time caps. Payload:
    /// `encode_u32x2(tid, dropped_flags)` (dropped_flags: bit0 block_io, bit1
    /// network, bit2 spawn, bit3 hypervisor, bit4 pcie_driver, bit5 platform,
    /// bit6 supervisor, bit7 mmio_devices changed, bit8 block_regions changed).
    CapNarrowedByPolicy = 19,
    /// Runtime revocation: a SpawnCap holder stripped capabilities from a live cell.
    /// Payload: `encode_u32x2(target_tid, cap_mask)` where `cap_mask` matches
    /// `api::syscall::cap_mask` constants (bit0=block_io, bit1=network, bit2=spawn,
    /// bit3=hypervisor, bits8-15=mmio_devices, bits16-23=block_regions).
    CapRevoked = 20,
    /// Cell binary signature verified at spawn (binary integrity gate).
    /// Payload: `encode_u32x2(tid, 0)`.
    CellSignatureVerified = 21,
    /// Cell binary signature absent or invalid — spawn denied.
    /// Payload: `encode_u32x2(tid, 0)`. Path is logged separately via `log::warn!`.
    CellSignatureFailed = 22,
    /// A cell was granted privileged path authority that survived policy narrowing.
    /// Payload: `encode_u32x2(tid, mask)` (bit0 pcie_driver, bit1 platform,
    /// bit2 supervisor). These are the caps that can DMA anywhere or orchestrate
    /// other cells, so the grant — not only its removal — is auditable.
    PrivilegedCapGranted = 23,
    /// The signed maintenance bypass was exercised: policy narrowing was skipped
    /// for this spawn. Requires BOTH the `maintenance-mode` build feature and the
    /// `MAINTENANCE_PERMITTED` flag in the signed policy. Payload:
    /// `encode_u32x2(tid, 0)`; the path is logged separately via `log::warn!`.
    PolicyMaintenanceBypass = 24,
    /// A cell hit `MAX_THREADS_PER_CELL` and its `spawn_thread` was refused.
    /// Payload: `encode_u32x2(cell_id, live_task_count)`. Refusing is the correct
    /// outcome, but a cell hitting the cap repeatedly is either leaking threads or
    /// probing for the allocator-fragmentation DoS this cap closes.
    ///
    /// Numbered 25 rather than 23: this variant and the two above were authored on
    /// parallel branches that each claimed 23. The discriminant is the byte written
    /// into the ring, so a collision would leave two unrelated events
    /// indistinguishable to anything that decodes the log.
    ThreadCapReached = 25,
    /// A present, verified policy had NO entry for a path that mints privileged
    /// (P-TRUST) authority, so that authority was stripped instead of kept.
    /// Payload: `encode_u32x2(tid, mask)` (bit0 pcie_driver, bit1 platform,
    /// bit2 supervisor) — the bits the request carried into the strip; `mask == 0`
    /// means the spawner held none of them and only the coverage gap remains.
    ///
    /// This is a bake-time mistake surfacing at runtime: every such path is
    /// supposed to be listed in the signed policy, and `scripts/sign-policy.py`
    /// refuses to build a blob that omits one. Path via `log::warn!`, as for the
    /// other policy events.
    PolicyNoEntryStripped = 26,
}

struct AuditRing {
    buf: UnsafeCell<[u8; BUF_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicUsize,
}

// SAFETY: single-hart kernel; interrupt-disable in log_event prevents
// timer ISR from preempting a partial write.
unsafe impl Sync for AuditRing {}

static RING: AuditRing = AuditRing {
    // SAFETY: UnsafeCell<[u8; N]> in a static is always zero-initialised.
    buf: UnsafeCell::new([0u8; BUF_SIZE]),
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
    dropped: AtomicUsize::new(0),
};

/// Write a kernel audit event to the ring.
///
/// Disables S-mode interrupts for the duration of the write so the timer ISR
/// cannot preempt a partial record.  Drops silently on ring-full.
pub fn log_event(event: AuditEvent, payload: &[u8]) {
    let plen = payload.len().min(255) as u8;
    let record_len = 10 + plen as usize;

    // Capture the outgoing interrupt state and mask interrupts on this hart.
    // Two reasons: a same-hart ISR must not re-enter the write and wait on the
    // lock below, and the record's bytes must be ordered before its publication.
    let saved_interrupts = crate::hal::arch::save_and_disable_interrupts();

    let _serialised = WRITE.lock();
    let head = RING.head.load(Ordering::Relaxed);
    let tail = RING.tail.load(Ordering::Acquire);

    // Drop-on-full: never overwrite consumer bytes.
    if head.wrapping_sub(tail) + record_len > BUF_SIZE {
        RING.dropped.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `saved_interrupts` was captured on this hart by the call above.
        unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
        return;
    }

    #[cfg(target_arch = "riscv64")]
    let mtime = hal::common::timer::read_mtime().to_le_bytes();
    #[cfg(not(target_arch = "riscv64"))]
    let mtime = 0u64.to_le_bytes();
    let buf = unsafe { &mut *RING.buf.get() };
    let mut pos = head;
    for &b in mtime
        .iter()
        .chain(core::slice::from_ref(&(event as u8)))
        .chain(core::slice::from_ref(&plen))
        .chain(&payload[..plen as usize])
    {
        buf[pos & MASK] = b;
        pos = pos.wrapping_add(1);
    }

    // Publish the write — consumer sees it only after this Release store.
    RING.head
        .store(head.wrapping_add(record_len), Ordering::Release);

    // SAFETY: `saved_interrupts` was captured on this hart by the call above.
    unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
}

/// Drain up to `out.len()` bytes from the ring.  Returns bytes copied.
///
/// Called by the log-flusher Cell (consumer side).
pub fn drain(out: &mut [u8]) -> usize {
    let head = RING.head.load(Ordering::Acquire);
    let tail = RING.tail.load(Ordering::Relaxed);
    let available = head.wrapping_sub(tail);
    if available == 0 {
        return 0;
    }

    let to_copy = available.min(out.len());
    let buf = unsafe { &*RING.buf.get() };
    for (i, byte) in out[..to_copy].iter_mut().enumerate() {
        *byte = buf[(tail.wrapping_add(i)) & MASK];
    }
    RING.tail
        .store(tail.wrapping_add(to_copy), Ordering::Release);
    to_copy
}

/// Number of records dropped due to ring-full since boot.
pub fn dropped_count() -> usize {
    RING.dropped.load(Ordering::Relaxed)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn snapshot() -> (usize, usize, usize) {
    (
        RING.head.load(Ordering::Acquire),
        RING.tail.load(Ordering::Acquire),
        RING.dropped.load(Ordering::Acquire),
    )
}

// ── Payload helpers ────────────────────────────────────────────────────────

/// Encode two `u32` values into a fixed 8-byte payload.
pub fn encode_u32x2(a: u32, b: u32) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&a.to_le_bytes());
    out[4..].copy_from_slice(&b.to_le_bytes());
    out
}
