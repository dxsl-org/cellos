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
//! When the ring is full the oldest whole records are evicted, counted in
//! [`dropped_count`], and reported in-band by a `RecordsDropped` record written
//! just ahead of the record that forced the eviction. No blocking, no corruption.
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
static WRITE: crate::sync::Spinlock<()> =
    crate::sync::Spinlock::ranked((), crate::sync::rank::AUDIT_WRITE);

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
    /// A launch was refused before any task existed. Governed-spawn refusals
    /// use [`encode_subject`] with the **spawner** as subject ([`Subject::KERNEL`]
    /// for boot and trusted-init launches), `d0` = refusal code (1 malformed
    /// manifest, 2 owner admission, 3 signature unverifiable, 4 unsigned in
    /// release, 0 privileged manifest outside `/bin`), `d1` = manifest flags.
    /// Domain-admission refusals still use `encode_u32x2(0, audit_code)`.
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
    /// A syscall was refused on an authority check. `d0`: the missing
    /// allowlist bit; `0x100 | bit` when the bit was present but the edge
    /// withholds its observation right; `u32::MAX` for an unknown opcode; or the
    /// raw handle for a pipe-ownership refusal.
    /// Payload: [`encode_subject`] with the caller as subject, `d1` = 0.
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
    /// Runtime revocation: a SpawnCap holder stripped capabilities from a live
    /// Cell incarnation (or was refused for HYPERVISOR). Payload:
    /// [`encode_subject`] with the **caller** as subject, `d0` = target tid,
    /// `d1` = `cap_mask`, where `cap_mask` matches
    /// `abi::syscall::cap_mask` constants (bit0=block_io, bit1=network, bit2=spawn,
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

/// Bytes of the synthetic `RecordsDropped` record: header plus `encode_u32x2`.
const DROP_RECORD_LEN: usize = 10 + 8;

/// Who an authority record is about: the acting task and its Cell
/// incarnation (architecture review H-1). [`Subject::KERNEL`] (all zero) marks
/// a kernel- or boot-initiated action with no calling task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subject {
    pub cell_id: u64,
    pub generation: u64,
    pub tid: u64,
}

impl Subject {
    pub const KERNEL: Subject = Subject {
        cell_id: 0,
        generation: 0,
        tid: 0,
    };

    /// The subject for `tid` read under a scheduler the caller already holds;
    /// a missing task keeps its tid with a zero incarnation.
    pub fn of(sched: &crate::sched::scheduler::Scheduler, tid: usize) -> Subject {
        sched.tasks.get(&tid).map_or(
            Subject {
                tid: tid as u64,
                ..Subject::KERNEL
            },
            |task| Subject {
                cell_id: task.cell_id.0,
                generation: task.cell_generation,
                tid: tid as u64,
            },
        )
    }
}

/// Payload of an authority record (`SyscallDenied`, `CapRevoked`,
/// `CellSpawnDenied`): `[cell u64][generation u64][tid u64][d0 u32][d1 u32]`,
/// all little-endian — the subject, then two event-specific words.
pub fn encode_subject(subject: Subject, d0: u32, d1: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[0..8].copy_from_slice(&subject.cell_id.to_le_bytes());
    out[8..16].copy_from_slice(&subject.generation.to_le_bytes());
    out[16..24].copy_from_slice(&subject.tid.to_le_bytes());
    out[24..28].copy_from_slice(&d0.to_le_bytes());
    out[28..32].copy_from_slice(&d1.to_le_bytes());
    out
}

/// Write a kernel audit event to the ring.
///
/// Disables interrupts for the duration of the write so a same-hart ISR cannot
/// preempt a partial record. When the ring is full the oldest whole records are
/// evicted (and counted in [`dropped_count`]), so the ring always holds the most
/// recent events: without a consumer, drop-on-full would keep only the first
/// 256 KiB after boot and lose everything since.
pub fn log_event(event: AuditEvent, payload: &[u8]) {
    // Capture the outgoing interrupt state and mask interrupts on this hart.
    // Two reasons: a same-hart ISR must not re-enter the write and wait on the
    // lock below, and the record's bytes must be ordered before its publication.
    let saved_interrupts = crate::hal::arch::save_and_disable_interrupts();

    // The guard drops before interrupts return: a same-hart ISR logging in the
    // gap would otherwise spin on a lock its own hart holds.
    {
        let _serialised = WRITE.lock();
        let head = RING.head.load(Ordering::Relaxed);
        let tail = RING.tail.load(Ordering::Relaxed);
        let dropped = RING.dropped.load(Ordering::Relaxed);
        // SAFETY: `WRITE` is held with interrupts masked (see `impl Sync`).
        let buf = unsafe { &mut *RING.buf.get() };

        let (head, tail, evicted) = push_record(buf, head, tail, event as u8, payload, dropped);
        if evicted != 0 {
            RING.tail.store(tail, Ordering::Release);
            RING.dropped.fetch_add(evicted, Ordering::Relaxed);
        }

        // Publish the write — consumer sees it only after this Release store.
        RING.head.store(head, Ordering::Release);
    }

    // SAFETY: `saved_interrupts` was captured on this hart by the call above.
    unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
}

/// Evict for, then write, one record at `head`; returns the new head, the new
/// tail and how many records were evicted. `dropped` is the cumulative eviction
/// count before this call. An eviction is reported by a `RecordsDropped` record
/// (evicted, cumulative) ahead of the event, so a consumer sees the gap where it
/// happened; room for it is reserved up front so it never evicts on its own.
fn push_record(
    buf: &mut [u8; BUF_SIZE],
    mut head: usize,
    tail: usize,
    event: u8,
    payload: &[u8],
    dropped: usize,
) -> (usize, usize, usize) {
    let payload = &payload[..payload.len().min(255)];
    let is_drop_event = event == AuditEvent::RecordsDropped as u8;
    let reserve = if is_drop_event { 0 } else { DROP_RECORD_LEN };
    let (tail, evicted) = evict_for(buf, head, tail, 10 + payload.len() + reserve);
    if evicted != 0 && !is_drop_event {
        let total = dropped.wrapping_add(evicted);
        let drop_payload = encode_u32x2(evicted as u32, total as u32);
        head = write_record_bytes(buf, head, AuditEvent::RecordsDropped as u8, &drop_payload);
    }
    (write_record_bytes(buf, head, event, payload), tail, evicted)
}

/// Write one record (payload at most 255 bytes) at `pos`; returns the position
/// just past it.
fn write_record_bytes(buf: &mut [u8; BUF_SIZE], pos: usize, event: u8, payload: &[u8]) -> usize {
    debug_assert!(payload.len() <= 255);
    let plen = payload.len() as u8;
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
    curr
}

/// Length of the record starting at ring position `at`: the 8-byte timestamp,
/// the event byte, the length byte at offset 9, then the payload.
fn record_len_at(buf: &[u8; BUF_SIZE], at: usize) -> usize {
    10 + buf[at.wrapping_add(9) & MASK] as usize
}

/// Advance `tail` by whole records until `record_len` more bytes fit; returns
/// the new tail and how many records were evicted. `tail` is always a record
/// boundary (only this and [`consume`] move it, by whole records), and a
/// record plus its `RecordsDropped` notice (at most 283 bytes) always fits an
/// empty ring.
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

/// Copy whole records, up to `out.len()` bytes, from the oldest one without
/// removing them. Returns the ring position the copy started at and the bytes
/// copied; hand both to [`consume`] once the copy has reached its reader, so a
/// failed delivery loses no audit record.
pub fn peek(out: &mut [u8]) -> (usize, usize) {
    let saved_interrupts = crate::hal::arch::save_and_disable_interrupts();
    let peeked = {
        let _serialised = WRITE.lock();
        let head = RING.head.load(Ordering::Relaxed);
        let tail = RING.tail.load(Ordering::Relaxed);
        // SAFETY: `WRITE` is held with interrupts masked (see `impl Sync`).
        let buf = unsafe { &*RING.buf.get() };
        (tail, copy_whole_records(buf, head, tail, out))
    };
    // SAFETY: `saved_interrupts` was captured on this hart by the call above.
    unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
    peeked
}

/// Remove the `copied` bytes that a [`peek`] starting at `start` delivered.
/// Serialised with producers because they may advance `tail` when they evict.
pub fn consume(start: usize, copied: usize) {
    let saved_interrupts = crate::hal::arch::save_and_disable_interrupts();
    {
        let _serialised = WRITE.lock();
        let tail = RING.tail.load(Ordering::Relaxed);
        RING.tail
            .store(tail_after_consume(tail, start, copied), Ordering::Release);
    }
    // SAFETY: `saved_interrupts` was captured on this hart by the call above.
    unsafe { crate::hal::arch::restore_sstatus(saved_interrupts) };
}

/// New tail once `[start, start + copied)` is consumed. Producers evict along
/// the same whole-record chain the peek copied, so a tail they moved is either
/// inside that range (finish at its end, a record boundary) or past it (keep).
fn tail_after_consume(tail: usize, start: usize, copied: usize) -> usize {
    if tail.wrapping_sub(start) < copied {
        start.wrapping_add(copied)
    } else {
        tail
    }
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

    const EVENT: u8 = AuditEvent::CellSpawn as u8;
    const DROPPED: u8 = AuditEvent::RecordsDropped as u8;

    /// Overfilling the ring evicts only the oldest whole records: the newest
    /// survive, every eviction is announced by a `RecordsDropped` record just
    /// ahead of the record that caused it, every drained byte parses as whole
    /// records, and a short read stops at a record boundary.
    #[test]
    fn full_ring_evicts_oldest_whole_records_and_drains_whole_records() {
        let mut buf = alloc::boxed::Box::new([0u8; BUF_SIZE]);
        // Start near usize::MAX so positions wrap during the test.
        let (mut head, mut tail) = (usize::MAX - 1000, usize::MAX - 1000);
        let mut evicted = 0;
        let mut notices = 0u32;
        let mut written = 0u32;
        // Varying lengths so record boundaries never line up with the ring size.
        while written < 4000 {
            let len = 1 + (written as usize * 37) % 200;
            let mut payload = alloc::vec![0u8; len];
            payload[..4.min(len)].copy_from_slice(&written.to_le_bytes()[..4.min(len)]);
            let (new_head, new_tail, n) =
                push_record(&mut buf, head, tail, EVENT, &payload, evicted);
            (head, tail) = (new_head, new_tail);
            if n != 0 {
                notices += 1;
            }
            evicted += n;
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
        let mut seen_notices = 0;
        let mut last_total = 0u32;
        let mut ids = alloc::vec::Vec::new();
        while at < n {
            let len = 10 + out[at + 9] as usize;
            assert!(at + len <= n, "partial record");
            let p = &out[at + 10..at + len];
            if out[at + 8] == DROPPED {
                let count = u32::from_le_bytes(p[..4].try_into().unwrap());
                let total = u32::from_le_bytes(p[4..8].try_into().unwrap());
                assert!(count > 0, "a notice reports at least one eviction");
                assert!(total > last_total, "cumulative count only grows");
                assert!(total as usize <= evicted);
                assert!(
                    at + len < n,
                    "a notice precedes the record it made room for"
                );
                assert_eq!(out[at + len + 8], EVENT);
                last_total = total;
                seen_notices += 1;
            } else {
                assert_eq!(out[at + 8], EVENT);
                if p.len() >= 4 {
                    ids.push(u32::from_le_bytes(p[..4].try_into().unwrap()));
                }
            }
            survivors += 1;
            at += len;
        }
        assert!(
            seen_notices > 0,
            "surviving evictions are announced in-band"
        );
        assert_eq!(last_total as usize, evicted, "the newest notice is current");
        assert_eq!(ids.last(), Some(&3999), "the newest record survives");
        assert!(
            ids.windows(2).all(|w| w[1] > w[0]),
            "survivors stay in write order"
        );
        assert_eq!(
            evicted as u32 + survivors,
            4000 + notices,
            "every record is either evicted or still in the ring"
        );
    }

    /// Consuming a peek moves `tail` to the end of what was delivered, unless
    /// producers already evicted past it meanwhile.
    #[test]
    fn consume_finishes_a_peek_without_rewinding_tail() {
        let start = usize::MAX - 5;
        assert_eq!(tail_after_consume(start, start, 20), start.wrapping_add(20));
        let mid = start.wrapping_add(8);
        assert_eq!(tail_after_consume(mid, start, 20), start.wrapping_add(20));
        let past = start.wrapping_add(30);
        assert_eq!(tail_after_consume(past, start, 20), past);
        assert_eq!(tail_after_consume(start, start, 0), start);
    }
}
