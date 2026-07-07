# Spec 17 — Cell IPC Wire Contract

> **Status**: Ratified 2026-07-07. Normative. New cells and every change to an
> existing IPC path MUST comply. Amendments require an entry in §9.
>
> This spec exists because the single largest source of recurring bugs in Cellos
> is not algorithms — it is **unspecified IPC contracts**. Every service invented
> its own framing, discriminant byte, blocking discipline, and buffer size on a
> shared `[u8; N]` message, and the mismatches produced silent, hard-to-trace
> failures (see §8 case studies). This document makes the contract explicit.

---

## 1. Scope & model

Cellos IPC is **kernel-mediated message passing** between cells (not the direct
vtable call that `specs/01` aspires to — see `system-architecture.md`). The
primitives (`libs/api/src/abi/syscall.rs`, kernel `task.rs`):

- `sys_send(target, &[u8])` — blocking: parks the caller in `Sending{target}`
  until the target is in `Recv` and copies the bytes into the target's recv buffer.
- `sys_try_send(target, &[u8])` — non-blocking: delivers iff the target is in a
  matching `Recv`, else drops (special-cased for the input service — see §6).
- `sys_recv(mask, &mut [u8]) -> sender_tid` — blocks until a message arrives;
  returns the **sender tid**, not a byte count. `mask == 0` = wildcard (any
  sender); `mask == tid` = only that sender.
- `sys_recv_timeout(mask, &mut [u8], ticks)` — as `sys_recv`, returns `Ok(0)` on
  timeout (10 ms/tick).
- `sys_reply` / `current_caller` — the request/reply short-circuit.

There is exactly **one recv buffer per cell**. Everything below exists because
that buffer is untyped and shared across every sender and every protocol.

---

## 2. The recv-mask rule (most important)

> **A request/reply exchange MUST recv masked to the service's tid.**
> `sys_recv(0)` (wildcard) is ONLY for an event loop that legitimately wants
> messages from *any* sender (the `run_app!` loop, the shell's `read_line`).

Rationale: a cell holding **input focus** has key events queued into its
`pending_msgs` by the input path (§6). If such a cell does
`sys_send(service); sys_recv(0)`, the wildcard recv can return a **queued key
event** instead of the service reply. The client decodes garbage (→ a bogus
"operation failed"), and the real reply arrives later and poisons the *next*
exchange. This desynced every VFS conversation for a day (§8.2).

Kernel guarantee (`kernel/src/task/syscall.rs`, Recv & RecvTimeout): the
`pending_msgs` drain **honours the mask** — a masked recv skips non-matching
queued messages and leaves them for the wildcard loop that wants them. Client
code must still pass the right mask.

**Do:**
```rust
let vfs = vfs_endpoint();
sys_send(vfs, &req);
match sys_recv(vfs, &mut reply) { /* only the VFS reply, never a keystroke */ }
```
**Don't:** `sys_recv(0, &mut reply)` in any request/reply helper.

---

## 3. Byte-0 discriminant registry

Every message's first byte selects a protocol namespace. These share one buffer,
so the allocation is **global and must not collide**. Current owners:

| byte 0 | Namespace | Direction | Notes |
|--------|-----------|-----------|-------|
| `0x00`–`0x0F` | **postcard enum variant index** (VfsRequest, NetRequest, ConfigRequest, …) | client → service | Self-delimiting; variant 0 is the first arm of each enum |
| `0x04` | `WIRE_ASCII` — kernel UART relay | kernel → input service | Overlaps the postcard range **but is disambiguated by sender** (kernel sender id `isize::MAX`), not by byte value |
| `0x10` | `INPUT_EVENT_OPCODE` | input service → focused cell | |
| `0x30`–`0x32` | legacy TLS raw ops (connect/send/recv) in the net service | client → net | Predates typed `NetRequest`; kept for `ostd::tls` |
| `0xAC` | `APP_MSG_MAGIC` — App SDK envelope | any → `run_app!`/`app_entry!` cell | byte 1 = event type (`0x00` Message, `0xFF` Shutdown, `0xF0`/`0xF1` hotswap) |

**Hazard:** the NIC Driver-Cell raw ops (`OP_TX=0`, `OP_RX=1`, `OP_GETMAC=2`)
live in the SAME low range as postcard variant indices. They do not collide
today only because the NIC driver and the postcard services are **different
target cells**. A cell that serves BOTH a postcard protocol and a raw op-byte
protocol on its single recv buffer is FORBIDDEN — disambiguate by cell, or by
the `0xAC` envelope, never by hoping the ranges don't meet.

**Rule:** a new protocol MUST either (a) use postcard (`api::ipc::encode`), or
(b) claim an unused byte-0 value here in §3 and §9. Never reuse a value for a
second meaning on the same receiver.

---

## 4. Framing

Raw `sys_recv` hands the receiver its **entire recv buffer** (up to
`IPC_BUF_SIZE` = 4096) with **no length**. The message boundary is not
recoverable from the buffer.

- **Typed messages: use postcard.** `api::ipc::{encode, decode}` /
  `take_from_bytes` — self-delimiting, tolerant of trailing zeros left by a
  previous larger message. This is the default; prefer it for all new IPC.
- **Raw byte protocols MUST carry an explicit length.** The NIC wire protocol
  learned this the hard way (§8.1): Tx is `[op, len_lo, len_hi] ++ frame`, and
  the receiver bounds the frame by `len`, never by "rest of the buffer".
- **Never assume the tail is zero.** The buffer is reused; a short message
  leaves stale bytes from the previous one. postcard handles this; raw parsers
  must respect the length header.

---

## 5. Buffer sizes

- `IPC_BUF_SIZE = 4096` (`libs/api/src/services/ipc.rs`) is the recv-buffer and
  max message size. A cell's recv buffer MUST be `IPC_BUF_SIZE` bytes.
- **A reply must fit the frame *after* its postcard envelope.** VFS caps
  `Data` payloads at 480 bytes, not 512, because a full-frame payload made
  `encode` fail and the client saw an *empty* reply (§8.3). When chunking a
  large payload, size chunks to leave envelope headroom (≤400–480 B is the
  established safe chunk).
- Send/reply scratch buffers smaller than `IPC_BUF_SIZE` are fine, but must be
  ≥ the largest message they encode; a too-small encode buffer returns `Err`,
  which MUST NOT be swallowed (§7).

---

## 6. Blocking discipline & the input queue

- **Service → client replies from a Driver Cell use `sys_try_send`**, not
  blocking `sys_send`. The client waits with `sys_recv_timeout` (≈200 ms). A
  blocking reply to a client that already timed out parks the driver in
  `Sending{client}` forever and desyncs every later request/reply pair (§8.1).
  A dropped reply is safe: the client treats it as a timeout and retries.
- **The input path is the one exception to try-send-drops.** When the input
  service (or the kernel UART relay) sends to a focused cell that is momentarily
  out of `Recv`, the kernel queues the event into the target's `pending_msgs`
  instead of dropping it, so a paste-speed burst is not lost. Bounds:
  - `HOTSWAP_MSG_QUEUE_DEPTH = 64` — messages buffered for a *frozen* cell during
    hot-swap.
  - `INPUT_EVENT_QUEUE_DEPTH = 512` — input events for the *focused* cell. Deeper
    because the shell drains one event per loop iteration and each echo is an SBI
    call per byte on RISC-V (slow on TCG), so backlog accumulates ACROSS commands
    (§8.4). All other `sys_try_send` callers keep strict drop-if-not-ready.
- **Backpressure over drop.** The kernel UART relay, when the input queue is
  full, parks the byte in `PENDING_ASCII` and retries next tick rather than
  dropping it mid-line (`console_drv.rs`).

---

## 7. Fail loud, never silent

Every silent degrade path in Cellos IPC has been a multi-hour debugging session.
Prohibited:

- **Silent-empty-reply** — returning an empty/zero result where an error
  occurred (the >480 B encode-fail; a decode mismatch treated as "no data").
  Surface a typed error variant (`VfsResponse::Err(code)`), not emptiness.
- **Silent-wrong-sender** — accepting `sys_recv`'s result without checking the
  returned sender tid matches the expected service (belt-and-suspenders on top
  of the mask).
- **Silent-drop** — dropping a message without a log or a retry path where the
  caller expects delivery (input relay drop → char loss).
- **Silent fallback to a weaker mechanism** — e.g. predictable-PRNG entropy when
  the real source is absent (now fail-closed behind `dev-weak-rng`,
  `kernel/src/task/syscall.rs`). Degrade paths must log or fail closed.

---

## 8. Case studies (the evidence this spec is built on)

**8.1 virtio-net Driver Cell (2026-07-06).** Four independent bugs, each fatal:
`CellHal::share` assumed cell-heap VAs were DMA-identity (they are not — bounce
via grant pages); the net cell's allowlist lacked `RecvTimeout`; driver replies
used blocking `sys_send` (→ permanent desync); Tx had no length header so the
frame boundary was unrecoverable in the padded buffer. Fixes → §4, §6.

**8.2 VFS "total write regression" (2026-07-07).** VFS writes always worked; the
shell's `sys_recv(0)` consumed a queued input key event as the VFS reply,
printing "vwrite failed" while the write succeeded, and the real reply desynced
the next call (vcat hang). Fix → §2. Unblocked ~10 boot tests.

**8.3 VFS empty reply on ≥512 B (earlier).** A full-frame `Data` payload made the
postcard `encode` fail; the client saw an empty reply. Fix → §5 (cap 480).

**8.4 Input burst loss / duplication (2026-06-29 → 2026-07-07).** Two variants:
timeout re-delivering a stale `current_caller` (character duplication), and the
focused cell's `pending_msgs` overflowing the shared 64-slot bound mid-line
(character loss). Fixes → §6 (`INPUT_EVENT_QUEUE_DEPTH`, timeout clears
`current_caller`).

---

## 9. Compliance checklist & amendments

A new or modified IPC path is compliant when:

- [ ] Request/reply recvs are **masked to the peer tid** (§2); only genuine
      event loops use `sys_recv(0)`.
- [ ] Payloads are **postcard-typed**, or a raw protocol claims a byte-0 value
      registered in §3 and carries an **explicit length** (§4).
- [ ] The recv buffer is `IPC_BUF_SIZE`; replies fit the frame after the
      envelope; chunks leave headroom (§5).
- [ ] Driver replies use `sys_try_send` + client `recv_timeout`; no
      blocking-reply-to-maybe-gone (§6).
- [ ] No silent-empty / silent-drop / silent-fallback path (§7).
- [ ] Prefer `ostd::ipc::service_call` (encapsulates §2/§4/§7) over hand-rolled
      send+recv.

**Byte-0 registry amendments** must add a row to §3 with owner, direction, and
the reason the value is safe against existing owners.
