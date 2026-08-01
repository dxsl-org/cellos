# Spec 17 — Cell IPC Wire Contract

> **Status**: Ratified 2026-07-07, except §10 which is Draft/reserved-but-unbuilt
> under [ADR 0001](../decisions/0001-readiness-notifications-remain-draft-until-implemented.md).
> New cells and every change to a Ratified IPC path MUST comply. Amendments require
> an entry in §9.
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
- `sys_recv_attested(mask, &mut [u8]) -> sender_tid` — as `sys_recv`, plus a
  kernel-written caller identity in the tail of the buffer (§11).
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
| `0x00`–`0x16` | **postcard enum variant index** (VfsRequest, NetRequest, ConfigRequest, …) | client → service | Self-delimiting; variant 0 is the first arm of each enum. Range widened from `0x0F` on 2026-07-31 by the `VfsRequest` directory-capability variants (14–22) — see the note below |
| `0x04` | `WIRE_ASCII` — kernel UART relay | kernel → input service | Overlaps the postcard range **but is disambiguated by sender** (kernel sender id `isize::MAX`), not by byte value |
| `0x10` | `INPUT_EVENT_OPCODE` | input service → focused cell | |
| `0x11` | **Reserved:** proposed `NET_READY` readiness edge (§10, Draft) | net service → interest-owner tid | No implementation exists. Held against reuse under ADR 0001; the proposed collision rules remain design constraints, not runtime claims |
| `0x12` | **Reserved:** proposed `REACTOR_WAKE` (§10.5, Draft) | same-cell thread → reactor tid | No implementation exists. Held against reuse under ADR 0001; no same-cell pending-message fallback is claimed yet |
| `0x30`–`0x32` | legacy TLS raw ops (connect/send/recv) in the net service | client → net | Predates typed `NetRequest`; kept for `ostd::tls`. **Client→net only — the net service never emits these toward a client** (§10.2) |
| `0xAC` | `APP_MSG_MAGIC` — App SDK envelope | any → `run_app!`/`app_entry!` cell | byte 1 = event type (`0x00` Message, `0xFF` Shutdown, `0xF0`/`0xF1` hotswap) |

**Hazard:** the NIC Driver-Cell raw ops (`OP_TX=0`, `OP_RX=1`, `OP_GETMAC=2`)
live in the SAME low range as postcard variant indices. They do not collide
today only because the NIC driver and the postcard services are **different
target cells**. A cell that serves BOTH a postcard protocol and a raw op-byte
protocol on its single recv buffer is FORBIDDEN — disambiguate by cell, or by
the `0xAC` envelope, never by hoping the ranges don't meet.

**Postcard variant growth past `0x0F` (2026-07-31).** `VfsRequest` now runs to
variant 22 (`0x16`), so its byte-0 values numerically overlap `0x10`
`INPUT_EVENT_OPCODE`, `0x11` `NET_READY` and `0x12` `REACTOR_WAKE`. Safe on the
same grounds those three are safe against each other — **by receiver, not by
value**:

- `0x10` goes to the *focused* cell. The VFS service never calls `SetFocus`, so
  it is never a focus target.
- `0x11` goes to a net-interest owner. The VFS manifest declares `network =
  false` and its syscall allowlist carries no net op, so it can never register
  an interest.
- `0x12` goes to a reactor tid inside the sending cell. The VFS main loop is a
  plain `sys_recv_attested` loop, not a reactor.

**Therefore:** growing `VfsRequest` past variant 22 (byte-0 `0x17`+), or making
the VFS service a focus target / net-interest owner / reactor host, MUST re-check
this table first. Same standing obligation the `NetRequest` 17/18 rows carry.

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
- **A receiver that opts into caller attestation (§11) gives up the last 32
  bytes** of its recv buffer to the kernel. Max usable payload for it is
  `IPC_BUF_SIZE - CALLER_IDENTITY_LEN`.
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
- [ ] A service that **authorizes** requests reads the caller from the kernel
      attestation (§11), never from the sender tid or the payload, and denies when
      the attestation is absent.

**Byte-0 registry amendments** must add a row to §3 with owner, direction, and
the reason the value is safe against existing owners.

**Amendment log:**
- 2026-08-01 — **D8 ruling:** §10 returns to Draft/reserved-but-unbuilt because
  its mechanisms are absent and Spec 21 forbids unbuilt work in a Ratified section.
  `0x11`/`0x12` remain reserved. The 2026-07-23 Law-1 confirmation #1 is historical;
  confirmation #2 is still required immediately before implementation. See ADR 0001.
- 2026-07-23 — **Ratified:** §3 rows `0x11 NET_READY` + `0x12 REACTOR_WAKE`;
  new §10 "Readiness notifications" (G4 P2.5). Design + rationale:
  `.agents/260722-0917-g4-full-std-tier1/design-p25-readiness-protocol-handle-abi.md`.
  Includes user confirm #1 (of the Law-1 2×) for appending `NetRequest`
  variants 17/18; confirm #2 happens at implementation time.
- 2026-07-30 — **Ratified (Law-1 2× confirmed):** new §11 "Caller attestation".
  Adds `api::caller_identity` (`CallerIdentity`, `CALLER_IDENTITY_LEN`,
  `RECV_ATTEST_CALLER`) and a flag on the previously-unused fourth `Recv`
  argument. No message enum changes, no discriminant changes, no framing change
  for any existing receiver.
- 2026-07-31 — **Ratified (Law-1 2× confirmed):** `VfsRequest` gains nine
  directory-capability variants (14–22) and `VfsResponse` gains `DirHandle`, all
  **appended at the end**; discriminants 0–13 and the existing response variants
  are unchanged, so an unmigrated cell's messages decode exactly as before. The
  §3 postcard range widens to `0x16` with the receiver-disambiguation note there.
  A handle-plus-component request is strictly smaller on the wire than the
  absolute path it replaces, so no message that fitted the frame stops fitting.
  Model and rationale: `docs/specs/09c-vfs-directory-capabilities-adr.md`.

---

## 10. Readiness notifications (G4 P2.5) — Draft / reserved-but-unbuilt

> How a cell learns "socket X is now readable/writable" without a kernel epoll.
> Consumed by the G4 async reactor (`polling`/`mio` backends); implemented by the
> net cell's readiness engine (G4 P2.6). Kernel stays multiplexing-free.
>
> This section reserves a reviewed design and byte values only. It does not describe
> behavior available in the current implementation. See ADR 0001.

### 10.1 `NET_READY` frame

Fixed 6-byte raw frame, sender = net service tid, target = the **interest-owner
tid** (10.3):

```
[0]      0x11  NET_READY
[1]      events bitmask: READABLE 0x01 · WRITABLE 0x02 · ERROR 0x04 · HUP 0x08
[2..6]   cap_id  u32 LE  (the provider-local socket handle from NetRequest)
```

Fixed length satisfies §4's explicit-boundary rule. Readiness is a **signal,
never data**: the payload carries no bytes; the receiver re-fetches via
`TcpRecv`/`UdpRecv` and must treat the frame as unforgeable only per 10.2.

### 10.2 Collision-safety invariants (normative)

1. `NetResponse` MUST NOT exceed 16 variants — its postcard byte-0 stays ≤
   `0x0F`, disjoint from `0x11`/`0x12` forever.
2. The net service never sends raw ops `0x30`–`0x32` toward a client; a reactor
   receiving them logs and drops (§7).
3. Attacker-controlled remote bytes exist only inside `NetResponse::Data`
   payloads at offset ≥ 2 (postcard variant tag + length varint) — they can
   never occupy byte-0, so `(sender_tid, byte0)` classification is unforgeable.
4. `NetRequest` variants 17/18 postcard-encode to byte-0 `0x11`/`0x12` — the
   same values as the two frames above. Safe **by direction only** (requests
   flow client→net; the frames flow net→client / same-cell→reactor). Therefore:
   the net service tid must never be a reactor's interest owner, and any
   `NetRequest` growth past variant 18 (byte-0 `0x13`+) MUST re-check this
   table before claiming the next index.

### 10.3 Interest registration

`NetRequest::NotifyRegister { cap_id: u32, interest: u8 }` (variant 17) and
`NotifyDeregister { cap_id: u32 }` (variant 18) — **append-only**; discriminants
0–16 are frozen. Interest bits: `READABLE 0x01 · WRITABLE 0x02` (ERROR/HUP are
always-on, unmaskable). Rules:

- Owner = the tid that sent `NotifyRegister`; edges go ONLY to that tid.
  Re-register replaces the interest mask. `TcpClose`/UDP close imply deregister.
- **Registration edge:** registering (or re-registering) immediately emits an
  edge for every interest bit currently level-true — closes the
  register-after-data race (mio semantics).
- Non-owner `NotifyRegister`/ops on a cap → typed `Err(NOT_OWNER)` (fail-loud §7).

### 10.4 Edge semantics + delivery

- **Edge-triggered:** one edge per false→true transition per interest bit.
  The client MUST drain until `WouldBlock`; an undrained level-true socket
  produces no further edges.
- Multiple transitions coalesce: the net cell keeps at most **one pending edge
  slot per (owner, cap)**, OR-merging event bits.
- Fan-out uses `sys_try_send` (§6 — never block on a client). A failed try_send
  keeps the edge in its pending slot and retries next loop iteration until
  delivered or deregistered — a dropped edge is **deferred, never lost**.
  Bounded by construction (≤ 1 slot per interest).

### 10.5 `REACTOR_WAKE` + the kernel same-cell fallback

`notify()` (waking a reactor parked in recv, from another thread of the same
cell or from itself before parking) sends the 1-byte frame `[0x12]` via
`sys_try_send(reactor_tid, ..)`. The kernel's `pending_msgs` fallback (§6,
previously input-service-only) is extended to queue a try_send **iff** the
sender's cell == target's cell **and** byte-0 == `0x12` **and** no same-cell
`0x12` is already queued (wakes are idempotent — coalesced to one slot). All
other same-cell try_sends keep strict drop-if-not-ready.

Because every recv path drains `pending_msgs` mask-honoring on entry, a wake
queued while the reactor was not yet parked is returned at its next recv
entry — no new syscall, and the wake is never *lost*. **SMP caveat:** the
drain and the park take the scheduler lock separately, so a wake landing in
the gap between them is seen only at the *next* recv entry — i.e. delivery
may defer up to one timeout period. Consequently a reactor MUST park with
`sys_recv_timeout` (bounded tick), NEVER bare `sys_recv` — with a bare recv
the deferred wake would be a true lost wakeup.

### 10.6 One recv consumer per tid + demux

**Exactly one code path may recv on a given tid.** A reactor owns wildcard
`sys_recv_timeout(0, ..)` on its own tid and classifies every message:

| Match `(sender, byte0)` | Route |
|---|---|
| net tid, `0x11` | fire waker for the handle; unknown/closed handle → log+drop |
| net tid, ≤ `0x0F` | postcard `NetResponse` → the reactor's in-flight request slot |
| same cell, `0x12` | wake — return from wait (spurious-safe) |
| `0xAC` / `0x10` | forward to the app queue (never dropped) |
| anything else | log + drop (§7) — never decode-guess |

`AppContext::run`/`run_with_lifecycle` on the same tid as a reactor is
**forbidden** (two wildcard consumers = §8.2 poisoning); ostd enforces via a
per-task recv-owner claim that panics at startup on double-claim. Blocking
request/reply on *other* threads keeps the §2 masked-recv rule unchanged —
delivery is tid-targeted, so a sibling's masked recv and the reactor's wildcard
recv cannot steal each other's messages.

### 10.7 Handle namespace (frozen)

The `u32` socket handle is provider-local, allocated monotonically from 1,
**never reused** within a provider's lifetime, `Err` on exhaustion — stale-edge
handling is therefore drop-unknown-handle at the reactor. The std-layer
`AsCellHandle` ABI over this value (class-tagged `u32`) is frozen in
`.agents/260722-0917-g4-full-std-tier1/design-p25-readiness-protocol-handle-abi.md` §D7.

---

## 11. Caller attestation — Ratified 2026-07-30

> How a service learns **which cell** is calling it. Required reading before
> putting an authorization check in any service.

### 11.1 The problem

`sys_recv` returns a **tid**, and a tid is not a cell:

- A cell spawned by the loader gets `cell_id == CellId(tid)`, so for it the two
  coincide — by accident of the spawn path, not by contract.
- A **thread** gets its own tid while inheriting its parent's `cell_id`. A service
  that built `CellId(sender)` therefore invented a cell that does not exist, and
  charged that thread's quota to a ledger row nothing owned.

Deriving identity from the request is worse. The nearest thing to a cell's name in
the system is the last component of the `path_hint` its spawner passed to
`SpawnFromPath` / `SpawnFromElf`, and a spawner chooses that string freely: a cell
spawned as `path_hint = "/bin/vfs"` is named `vfs`. Cell signatures cover the ELF
bytes, not the path, so nothing rejects the lie. **Any ACL keyed on a cell name is
forgeable.** `CapSet` is safe there (the ceiling bounds it, and a lied-about hint
can only lose privilege); a service-side ACL is not.

### 11.2 The mechanism

A receiver opts in by passing `RECV_ATTEST_CALLER` as the **fourth argument of
`ViSyscall::Recv`** — a register that was unused and that every pre-existing
caller passes as 0. `ostd::syscall::sys_recv_attested` is the wrapper.

The kernel then writes a `CallerIdentity` (`cell_id`, `generation`, `sender_tid`;
32 bytes, LE, tagged) into the **last `CALLER_IDENTITY_LEN` bytes of that
receiver's own recv buffer**, and does so **after** copying the payload. Read it
with `CallerIdentity::from_recv_buf(&buf)`.

Why the tail and not a message field: byte 0 is the postcard discriminant, so
widening a request enum or prefixing the frame moves every following byte — the
failure mode §8.1 was built on. `decode` (`take_from_bytes`) consumes exactly one
message and ignores the rest, so a fixed-offset trailer past the payload is
invisible to every existing parser.

### 11.3 Normative rules

- **Absent identity means deny.** `from_recv_buf` returning `None` — no trailer, a
  garbage tail, or `cell_id == 0` — is "unknown caller". Refuse the request. Do
  not fall back to the sender tid, and do not treat it as a caller that merely
  owns nothing: owning nothing still reads everything unowned.
- **Opting in reserves the tail** (§5). Do not expect payload bytes there.
- **The kernel writes the trailer last**, so a sender that pads its message across
  the whole buffer cannot pre-place a forged one.
- **Identity is per cell, not per task.** A thread reports its parent cell, which
  is what makes one attestation serve both authorization and accounting.
- `generation` is a monotonic cell epoch, inherited by a cell's threads. A service
  that holds state against a `CellId` — open handles, pending reads — MUST compare
  the generation too, so a successor cell under a recycled id does not inherit its
  predecessor's state. `generation == 0` means "not attested on this path"; refuse
  to record durable state for it.
- `TryRecv`, `RecvTimeout`, and `RecvScatter` do **not** attest. `RecvTimeout`
  already uses the fourth argument for its deadline, and `RecvScatter` has no
  single buffer tail to write. A service that authorizes must receive with `Recv`.

### 11.4 Direct (non-`ecall`) service calls

`fast_ipc` bypasses the trap entirely, so there is no sender tid and nothing in
the arguments is trustworthy — every one of them is chosen by the cell being
authorized. `kernel::fast_ipc::call_vfs` therefore resolves the identity itself,
from live scheduler state for the task running on the hart, and passes it to the
handler. `TrustedHandle<T>` is not a control; its own contract says it is
advisory.

A fast-path handler MUST authorize exactly as its `ecall` counterpart does. VFS's
fast path serves `GetFile`, which replies with a raw `DataPtr` — in a single
address space that is permanent, unrevocable read authority — so an ungated fast
path would reduce the gate on the message path to decoration.
