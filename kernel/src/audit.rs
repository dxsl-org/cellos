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
    /// bit6 supervisor, bit7 mmio_devices changed, bit8 block_regions changed,
    /// bit9 net_resolve, bit10 net_connect, bit11 net_listen, bit12 net_l2,
    /// bit13 observe_tasks, bit14 observe_memory, bit15 vfs_mutate,
    /// bit16 config_write, bit17 kms_ssh_sign, bit18 kms_ssh_admin).
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
    /// bit2 supervisor, bit3 serial_port). These are the caps that can DMA
    /// anywhere, orchestrate other cells, or reach hardware the kernel owns, so
    /// the grant — not only its removal — is auditable.
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
    /// A cell left policy holding coarse network rights, which the kernel exports
    /// to service-net as attested `CALLER_FLAG_NET_*` trailer flags (Phase 04
    /// S2-N). Payload: `encode_u32x2(tid, mask)` (bit0 net_resolve, bit1
    /// net_connect, bit2 net_listen, bit3 net_l2). Recorded for the grant, not
    /// only a narrowing, because `net_l2` puts raw frames on the NIC.
    NetRightsGranted = 27,
    /// Synthetic event recorded when ring buffer overflows and older records are evicted.
    /// Payload: `encode_u32x2(evicted_count, cumulative_dropped)`.
    RecordsDropped = 28,
}

struct AuditRing {
    buf: UnsafeCell<[u8; BUF_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    /// Whole records evicted to make room for newer ones.
    dropped: AtomicUsize,
}

// SAFETY: every access to `buf`, and every move of `tail`, happens under
// `WRITE` with local interrupts masked: producers (any hart, any context) and
// the consumer in `drain` are serialised, and a same-hart ISR cannot interleave.
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
/// Disables interrupts for the duration of the write so a same-hart ISR cannot
/// preempt a partial record. When the ring is full the oldest whole records are
/// evicted (and counted in [`dropped_count`]), so the ring always holds the most
/// recent events: without a consumer, drop-on-full would keep only the first
/// 256 KiB after boot and lose everything since.
pub fn log_event(event: AuditEvent, payload: &[u8]) {
    let plen = payload.len().min(255) as u8;
    let record_len = 10 + plen as usize;

    // Capture the outgoing interrupt state and mask interrupts on this hart.
    let saved_interrupts = crate::hal::arch::save_and_disable_interrupts();

    let _serialised = WRITE.lock();
    let mut head = RING.head.load(Ordering::Relaxed);
    let tail = RING.tail.load(Ordering::Relaxed);
    let buf = unsafe { &mut *RING.buf.get() };

    let is_drop_event = matches!(event, AuditEvent::RecordsDropped);
    let needed = if !is_drop_event {
        record_len + 18
    } else {
        record_len
    };

    // Evict whole records from the tail until the new one fits.
    let (new_tail, evicted) = evict_for(buf, head, tail, needed);
    if evicted != 0 {
        RING.tail.store(new_tail, Ordering::Release);
        let total = RING.dropped.fetch_add(evicted, Ordering::Relaxed) + evicted;
        if !is_drop_event {
            let drop_payload = encode_u32x2(evicted as u32, total as u32);
            let drop_len =
                write_record_bytes(buf, head, AuditEvent::RecordsDropped as u8, &drop_payload);
            head = head.wrapping_add(drop_len);
        }
    }

    let written = write_record_bytes(buf, head, event as u8, &payload[..plen as usize]);
    RING.head
        .store(head.wrapping_add(written), Ordering::Release);

    unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
}

fn write_record_bytes(buf: &mut [u8; BUF_SIZE], pos: usize, event: u8, payload: &[u8]) -> usize {
    let plen = payload.len().min(255) as u8;
    #[cfg(target_arch = "riscv64")]
    let mtime = hal::common::timer::read_mtime().to_le_bytes();
    #[cfg(not(target_arch = "riscv64"))]
    let mtime = 0u64.to_le_bytes();
    let mut curr = pos;
    for &b in mtime
        .iter()
        .chain(core::slice::from_ref(&event))
        .chain(core::slice::from_ref(&plen))
        .chain(payload)
    {
        buf[curr & MASK] = b;
        curr = curr.wrapping_add(1);
    }
    10 + plen as usize
}

/// Length of the record starting at ring position `at`: the 8-byte timestamp,
/// the event byte, the length byte at offset 9, then the payload.
fn record_len_at(buf: &[u8; BUF_SIZE], at: usize) -> usize {
    10 + buf[at.wrapping_add(9) & MASK] as usize
}

/// Advance `tail` by whole records until `record_len` more bytes fit; returns
/// the new tail and how many records were evicted. `tail` is always a record
/// boundary (only this and `copy_whole_records` move it, by whole records), and
/// a record (at most 265 bytes) always fits an empty ring.
fn evict_for(
    buf: &[u8; BUF_SIZE],
    head: usize,
    mut tail: usize,
    record_len: usize,
) -> (usize, usize) {
    let mut evicted = 0;
    while head.wrapping_sub(tail) + record_len > BUF_SIZE {
        tail = tail.wrapping_add(record_len_at(buf, tail));
        evicted += 1;
    }
    (tail, evicted)
}

/// Copy whole records from `tail` towards `head` into `out`; a record that does
/// not fit in the remaining space stops the copy. Returns bytes copied.
fn copy_whole_records(buf: &[u8; BUF_SIZE], head: usize, tail: usize, out: &mut [u8]) -> usize {
    let mut copied = 0;
    while tail.wrapping_add(copied) != head {
        let len = record_len_at(buf, tail.wrapping_add(copied));
        if copied + len > out.len() {
            break;
        }
        for i in 0..len {
            out[copied + i] = buf[tail.wrapping_add(copied + i) & MASK];
        }
        copied += len;
    }
    copied
}

/// Drain whole records, up to `out.len()` bytes, from the ring. Returns bytes
/// copied; a record that does not fit in the remaining space is left for the
/// next call.
///
/// For the future log-flusher consumer; it has no caller today. Serialised with
/// producers because they may advance `tail` when they evict.
pub fn drain(out: &mut [u8]) -> usize {
    let saved_interrupts = crate::hal::arch::save_and_disable_interrupts();
    let copied = {
        let _serialised = WRITE.lock();
        let head = RING.head.load(Ordering::Relaxed);
        let tail = RING.tail.load(Ordering::Relaxed);
        // SAFETY: `WRITE` is held with interrupts masked (see `impl Sync`).
        let buf = unsafe { &*RING.buf.get() };
        let copied = copy_whole_records(buf, head, tail, out);
        RING.tail
            .store(tail.wrapping_add(copied), Ordering::Release);
        copied
    };
    // SAFETY: `saved_interrupts` was captured on this hart by the call above.
    unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
    copied
}

/// Number of records evicted to make room for newer ones since boot.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The producer path of `log_event` over a local ring (the real one masks
    /// interrupts, which a host test cannot do): evict, write, advance head.
    fn put(
        buf: &mut [u8; BUF_SIZE],
        head: &mut usize,
        tail: &mut usize,
        event: u8,
        payload: &[u8],
    ) -> usize {
        let record_len = 10 + payload.len();
        let (new_tail, evicted) = evict_for(buf, *head, *tail, record_len);
        *tail = new_tail;
        let mut pos = *head;
        for &b in [0u8; 8]
            .iter()
            .chain(&[event, payload.len() as u8])
            .chain(payload)
        {
            buf[pos & MASK] = b;
            pos = pos.wrapping_add(1);
        }
        *head = pos;
        evicted
    }

    /// Overfilling the ring evicts only the oldest whole records: the newest
    /// survive, every drained byte parses as whole records, and a short read
    /// stops at a record boundary instead of splitting one.
    #[test]
    fn full_ring_evicts_oldest_whole_records_and_drains_whole_records() {
        let mut buf = alloc::boxed::Box::new([0u8; BUF_SIZE]);
        // Start near usize::MAX so positions wrap during the test.
        let (mut head, mut tail) = (usize::MAX - 1000, usize::MAX - 1000);
        let mut evicted = 0;
        let mut written = 0u32;
        // Varying lengths so record boundaries never line up with the ring size.
        while written < 4000 {
            let len = 1 + (written as usize * 37) % 200;
            let mut payload = alloc::vec![0u8; len];
            payload[..4.min(len)].copy_from_slice(&written.to_le_bytes()[..4.min(len)]);
            evicted += put(&mut buf, &mut head, &mut tail, 7, &payload);
            written += 1;
            assert!(head.wrapping_sub(tail) <= BUF_SIZE);
        }
        assert!(
            evicted > 0,
            "4000 records of up to 210 bytes overflow 256 KiB"
        );

        let mut out = alloc::vec![0u8; BUF_SIZE];
        let first_len = record_len_at(&buf, tail);
        assert_eq!(
            copy_whole_records(&buf, head, tail, &mut out[..first_len + 3]),
            first_len,
            "short read stops at a boundary"
        );
        assert_eq!(
            copy_whole_records(&buf, head, tail, &mut out[..first_len - 1]),
            0,
            "a record larger than the space is left"
        );

        let n = copy_whole_records(&buf, head, tail, &mut out);
        assert_eq!(
            n,
            head.wrapping_sub(tail),
            "an ample buffer drains everything"
        );
        let mut at = 0;
        let mut survivors = 0u32;
        let mut ids = alloc::vec::Vec::new();
        while at < n {
            let len = 10 + out[at + 9] as usize;
            assert!(at + len <= n, "partial record");
            assert_eq!(out[at + 8], 7);
            let p = &out[at + 10..at + len];
            if p.len() >= 4 {
                ids.push(u32::from_le_bytes(p[..4].try_into().unwrap()));
            }
            survivors += 1;
            at += len;
        }
        assert_eq!(ids.last(), Some(&3999), "the newest record survives");
        assert!(
            ids.windows(2).all(|w| w[1] > w[0]),
            "survivors stay in write order"
        );
        assert_eq!(
            evicted as u32 + survivors,
            4000,
            "every record is either evicted or still in the ring"
        );
    }
}
