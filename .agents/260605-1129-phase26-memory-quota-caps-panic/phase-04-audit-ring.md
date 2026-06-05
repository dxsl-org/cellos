# Phase 04 — Kernel Audit Ring Buffer

**Status**: 📋 PLANNED  
**Priority**: P2  
**Effort**: 2 days  
**Depends on**: Phase 01 (CellId available), Phase 03 (stable after fault isolation)

---

## Context Links

- IPC dispatch entry: `kernel/src/task/syscall.rs:287` (`handle_syscall`)
- File op sites: `kernel/src/task/syscall.rs` (Open, Read, Write, Close handlers)
- Net op sites: `kernel/src/task/syscall.rs:1117-1131` (NetTx, NetRx)
- VFS service: `cells/services/vfs/src/`
- Timer: `hal/arch/riscv/src/common/timer.rs` (`read_mtime()`)

---

## Overview

Zero observability makes debugging impossible. A 256 KB lock-free SPSC ring buffer logs key kernel events — IPC sends/receives, file opens/writes, network transmit/receive, Cell spawn/fault — with a timestamp. A low-priority background Cell drains the ring and appends to `/data/kernel.log`.

**Design constraints:**
- No heap allocation in the ring itself — static `[u8; 256 * 1024]`
- **⚠️ Not SPSC**: the timer ISR can preempt a syscall-context `log_event()` write, creating two concurrent producers. Fix: disable S-mode interrupts (`csrci sstatus, 0x2`) around each `log_event()` call, restore after. This makes the write atomic w.r.t. interrupts on single-hart.
- **Drop-on-full**: if `head - tail >= BUF_SIZE`, skip the write and increment `dropped: AtomicUsize`. No blocking, no corruption.
- Record format: fixed 10-byte header + variable payload — compact, parseable

---

## Record Format

```
[u64 mtime_ticks LE][u8 event_type][u8 payload_len][payload bytes...]
 8 bytes              1 byte         1 byte           0-255 bytes
```

Total minimum: 10 bytes per event.

**Event types:**
```rust
pub enum AuditEvent {
    IpcSend   = 1,  // payload: [sender_id: u32][target_id: u32]
    IpcRecv   = 2,  // payload: [receiver_id: u32][sender_id: u32]
    FileOpen  = 3,  // payload: [cell_id: u32][path_len: u8][path...]
    FileWrite = 4,  // payload: [cell_id: u32][fd: u32][nbytes: u32]
    NetTx     = 5,  // payload: [cell_id: u32][nbytes: u32]
    NetRx     = 6,  // payload: [cell_id: u32][nbytes: u32]
    CellSpawn = 7,  // payload: [cell_id: u32][path_len: u8][path...]
    CellFault = 8,  // payload: [cell_id: u32][scause: u32][sepc: u64]
    CellExit  = 9,  // payload: [cell_id: u32][exit_code: u32]
}
```

---

## Related Code Files

### Create
- `kernel/src/audit.rs` — SPSC ring buffer + `log_event()` API

### Modify
- `kernel/src/main.rs` — add `pub mod audit;`
- `kernel/src/task/syscall.rs` — instrument `Send`, `Recv`, `Open`, `Write`, `NetTx`, `NetRx`, `Exit`
- `kernel/src/task.rs` — instrument `terminate_current_cell_on_fault()`
- `kernel/src/loader.rs` — instrument `spawn_from_path()`
- `cells/apps/` or `cells/services/` — create a low-priority `log-flusher` cell (optional, can be shell builtin)

---

## Implementation Steps

### Step 1 — Create `kernel/src/audit.rs`

```rust
//! Lock-free SPSC ring buffer for kernel audit events.
//!
//! Single producer: the kernel (any call site via `log_event()`).
//! Single consumer: the log-flusher Cell, which calls `drain()`.
//! Safe for QEMU single-hart; on SMP, wrap the producer in a Spinlock.
//!
//! Ring size is a power of two so index wrapping is branchless: `pos & MASK`.

use core::sync::atomic::{AtomicUsize, Ordering};
use hal::common::timer::read_mtime;

const BUF_SIZE: usize = 256 * 1024; // must be power of two
const MASK:     usize = BUF_SIZE - 1;

/// Audit event type byte.
#[repr(u8)]
pub enum AuditEvent {
    IpcSend   = 1,
    IpcRecv   = 2,
    FileOpen  = 3,
    FileWrite = 4,
    NetTx     = 5,
    NetRx     = 6,
    CellSpawn = 7,
    CellFault = 8,
    CellExit  = 9,
}

struct AuditRing {
    buf:     core::cell::UnsafeCell<[u8; BUF_SIZE]>,
    head:    AtomicUsize, // write cursor (kernel writes here)
    tail:    AtomicUsize, // read cursor  (flush task reads here)
    dropped: AtomicUsize, // count of records dropped due to ring-full
}

// SAFETY: single-hart kernel; producer and consumer never run concurrently
// (producer runs in interrupt/syscall context; consumer is a regular Cell task).
unsafe impl Sync for AuditRing {}

static RING: AuditRing = AuditRing {
    // SAFETY: UnsafeCell<[u8; N]> is zero-initialised in the static BSS section.
    buf:     core::cell::UnsafeCell::new([0u8; BUF_SIZE]),
    head:    AtomicUsize::new(0),
    tail:    AtomicUsize::new(0),
    dropped: AtomicUsize::new(0),
};

/// Write a kernel audit event to the ring buffer.
///
/// Payload must fit in 255 bytes.  If the ring is full the oldest data is
/// overwritten (standard ring-buffer overflow behaviour — observability loss
/// is better than blocking the kernel).
pub fn log_event(event: AuditEvent, payload: &[u8]) {
    debug_assert!(payload.len() <= 255, "audit payload too large");
    let record_len = 10 + payload.len(); // header(10) + payload

    // Disable S-mode interrupts to prevent the timer ISR from preempting a
    // partial write — the ISR also calls log_event (CellFault events).
    // SAFETY: single-hart; restoring SIE after the write is always correct.
    let sie_was_set = unsafe {
        let v: usize;
        core::arch::asm!("csrrci {}, sstatus, 0x2", out(reg) v);
        v & 0x2 != 0
    };

    let head = RING.head.load(Ordering::Relaxed);
    let tail = RING.tail.load(Ordering::Acquire);

    // Drop-on-full: skip write to avoid corrupting partially-written records.
    if head.wrapping_sub(tail) + record_len > BUF_SIZE {
        RING.dropped.fetch_add(1, Ordering::Relaxed);
        if sie_was_set { unsafe { core::arch::asm!("csrsi sstatus, 0x2"); } }
        return;
    }

    // Write header + payload into ring, wrapping as needed.
    // SAFETY: single-hart; no concurrent writes to buf.
    let buf = unsafe { &mut *RING.buf.get() };
    let mtime = read_mtime().to_le_bytes();
    let mut pos = head;
    for &b in mtime.iter()
        .chain(core::slice::from_ref(&(event as u8)))
        .chain(core::slice::from_ref(&(payload.len() as u8)))
        .chain(payload)
    {
        buf[pos & MASK] = b;
        pos += 1;
    }

    // Publish the write — consumer sees new data only after this store.
    RING.head.store(head + record_len, Ordering::Release);

    // Restore SIE if it was enabled before we disabled it.
    // SAFETY: restoring to previous state; no invariant violated.
    if sie_was_set { unsafe { core::arch::asm!("csrsi sstatus, 0x2"); } }
}

/// Drain up to `out.len()` bytes from the ring into `out`.
/// Returns the number of bytes written.
/// Called by the log-flusher Cell (consumer).
pub fn drain(out: &mut [u8]) -> usize {
    let head = RING.head.load(Ordering::Acquire);
    let tail = RING.tail.load(Ordering::Relaxed);
    let available = head.wrapping_sub(tail);
    if available == 0 { return 0; }

    let to_copy = available.min(out.len());
    let buf = unsafe { &*RING.buf.get() };
    for (i, byte) in out[..to_copy].iter_mut().enumerate() {
        *byte = buf[(tail + i) & MASK];
    }

    RING.tail.store(tail + to_copy, Ordering::Release);
    to_copy
}
```

### Step 2 — Instrument call sites in `syscall.rs`

Add a one-liner at each key site (non-blocking — just writes to the static ring):

```rust
// In Syscall::Send handler:
crate::audit::log_event(AuditEvent::IpcSend, &encode_u32x2(caller_id as u32, target as u32));

// In Syscall::NetTx handler (after NetworkCap guard from Phase 01):
crate::audit::log_event(AuditEvent::NetTx, &encode_u32x2(caller_id as u32, data_len as u32));
```

### Step 3 — Log-flusher Cell (background task)

A minimal cell that loops: drain ring → VFS write to `/data/kernel.log` → sleep 100 ms.
Runs at `Background` priority (lowest). Can be a `cells/apps/log-flusher/` binary or a shell alias.

---

## Todo List

- [ ] Create `kernel/src/audit.rs` (SPSC ring, `log_event()`, `drain()`)
- [ ] Add `pub mod audit;` to `kernel/src/main.rs`
- [ ] Instrument `Syscall::Send` in `syscall.rs`
- [ ] Instrument `Syscall::Recv` in `syscall.rs`
- [ ] Instrument `Syscall::Open`/`Write` in `syscall.rs`
- [ ] Instrument `Syscall::NetTx`/`NetRx` in `syscall.rs` (after Phase 01 NetworkCap guard)
- [ ] Instrument `spawn_from_path()` in `loader.rs` (CellSpawn event)
- [ ] Instrument `terminate_current_cell_on_fault()` in `task.rs` (CellFault event)
- [ ] Instrument `Syscall::Exit` handler (CellExit event)
- [ ] Create `cells/apps/log-flusher/` minimal background Cell (or shell `logflush` builtin)
- [ ] Integration test: run IPC, check `/data/kernel.log` contains entries

---

## Success Criteria

- [ ] After booting + running shell for 5 seconds, `/data/kernel.log` is non-empty
- [ ] Log entries have correct timestamps (mtime ticks, increasing)
- [ ] IpcSend event appears for every `sys_send()` call (verified by reading log file)
- [ ] CellFault entry appears when a Cell faults (coordinated with Phase 03)
- [ ] Ring overflow (if log-flusher not running) does not crash the kernel
- [ ] `cargo check --workspace` passes

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `UnsafeCell<[u8; 256K]>` in static: BSS zero-init assumed | Low | Confirmed: Rust statics with `UnsafeCell` are zero-initialized; linker places in BSS |
| Ring overflow drops oldest events silently | Medium | Intentional: observability loss > blocking. Consider adding a `dropped_count: AtomicUsize` |
| `drain()` called from Cell context while `log_event()` runs from timer ISR | Medium | SPSC: head is producer-owned, tail consumer-owned; no interleaving possible. Acquire/Release ensures visibility. |
| `read_mtime()` in `log_event()` adds ~3 ns overhead | Low | Acceptable; CSR read is a single instruction |
