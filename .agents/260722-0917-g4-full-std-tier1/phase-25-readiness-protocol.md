# Phase 02.5 — Readiness-notification protocol (spec 17 extension)

## Context Links
- Plan: [plan.md](plan.md) · Extends: `docs/specs/17-ipc-wire-contract.md`
- Consumed by: [phase-03](phase-03-async-backends.md) · Related: [phase-02](phase-02-os-std.md) net handle model

## Overview
- **Priority:** P2 (design-critical). **Status:** **design RATIFIED 2026-07-23** — contract +
  handle ABI frozen; only the `reactor-spike` validation remains (post-G3 code). Deliverables:
  [design-p25](design-p25-readiness-protocol-handle-abi.md) (decisions D1-D9 + frozen
  `AsCellHandle` ABI) + spec 17 §10 ratified amendment (`docs/specs/17-ipc-wire-contract.md`).
- Define, as a normative extension to spec 17, how a cell learns "socket X is now readable/writable"
  without an epoll syscall. This is the **highest design-risk item** in G4 — spec before any P3 code.
- **Red-team additions:** this phase also owns the **reactor recv-channel rules** (M1 — three facets) and
  **freezes the `AsCellHandle` trait + handle namespace/lifetime/reuse rule** (M6 — moved here from P4 so
  P3's mio fork and P4 both consume a frozen definition and neither can redefine it).

## Key Insights (from research, verified)
- **Readiness, not completion.** mio's Windows/AFD backend manufactures edge-readiness on top of a
  completion primitive; both `polling` and `mio` present **readiness** at the public API and never pass
  data. Cellos backend must do the same: the readiness IPC message is a **signal**, the payload is
  discarded at the reactor layer, and the caller's later `recv()` on the socket handle re-fetches bytes.
- **Cellos IPC already provides the two halves of `epoll_wait`:** blocking `sys_recv_timeout(mask,buf,
  ticks)` (returns sender tid, or 0 on timeout) + non-blocking drain `sys_try_recv`. Loop try_recv until
  `Ok(0)` = drain-all-ready in one `wait()`.
- **The mask is BINARY** (0 = any sender, N = exactly sender N) — **not an OR-able bitmask**. So a reactor
  cannot "recv only these 3 sources." It MUST use **wildcard recv (mask=0) + software demux by sender_id**
  against a registered token map. This forecloses one-recv-per-socket designs.
- **No self-wake/`notify()` primitive exists** (grep confirmed: only `sys_notify_on_exit`). A reactor
  `notify()` (wake the blocked reactor thread when a new future/interest is registered from another
  thread) needs one of: **(a) self-send an IPC wakeup message to own tid** (if kernel permits send-to-self
  and it interrupts a blocked recv), or **(b) a small `sys_wake_recv(tid)` kernel primitive** (IPC/
  scheduler mechanism — Boundary-Law-legal), or (c) bounded-timeout busy-poll (worst; adds latency).
  **This is the central decision this phase must settle before P3.**
- **[M1a — self-send has a lost-wakeup window, verified.]** `ipc_try_send` delivers only if the target is
  in `TaskState::Recv`; otherwise the bounded `pending_msgs` fallback fires **only for
  `caller==input_tid`** — every other sender is **dropped** (`kernel/src/task.rs:1304-1375`). So a
  `notify()` self-send arriving while the reactor is **not yet parked** in Recv is **lost** → tokio hangs.
  **Fix: a wake that survives the not-yet-parked window** — a per-task **pending-wake flag** consumed by
  `recv_timeout` on entry (return immediately if set), OR extend the `pending_msgs` fallback to self-sends.
- **[M1b — one shared channel, attacker bytes vs edges.]** The reactor uses wildcard recv (mask is binary
  0/N), so an `R::Data` reply carrying **attacker-controlled remote bytes** shares `sender=net_tid` with
  readiness edges. Byte-0 software demux is the **only** separator. **Require a length-prefixed,
  postcard-typed envelope whose type tag is outside any attacker payload**; forbid mixing raw byte-0 ops
  `0x30-0x32` on the reactor channel; **prove the readiness discriminant cannot collide with a Data
  frame's first byte.**
- **[M1c — single-consumer ownership, verified.]** ostd `run_with_lifecycle` loops on wildcard `sys_recv`
  (`libs/ostd/src/app.rs:162,187`) while a tokio reactor also blocks on wildcard recv → **two consumers
  steal each other's messages** (the spec-17 §8.2 poisoning class). **Fix: normative rule — exactly one
  recv consumer per cell.** Either the reactor owns recv and forwards app IPC to an ostd-compat queue, or
  ostd's loop owns recv and forwards readiness; **forbid `run_with_lifecycle` + a reactor in one cell**
  (compile-time or boot-time assert).
- **[M6 — freeze `AsCellHandle` here.]** The socket handle (`u32`) is the "fd" that P3's `polling`/`mio`
  forks key on and P4's `std::os::cellos::io` formalizes. If P4 changed the namespace/generation rule
  after P3 froze, the mio fork would need rework. **Define `AsCellHandle`/`FromCellHandle`/`IntoCellHandle`
  + the handle namespace, lifetime, generation, and reuse rule in this frozen contract.** P3 and P4 both
  consume it; P4 adds only extension methods, never redefines.

## Requirements (the protocol MUST specify)
1. **Readiness message format** — a byte-0 discriminant (claim a value in spec 17 §3, e.g. `0x11`
   `NET_READY`), carrying `{ socket_handle: u32, events: u8 (READABLE|WRITABLE|ERROR|HUP) }`. Postcard or
   fixed raw+length (spec 17 §4). Sender = net cell tid.
2. **Interest registration** — how a cell tells the net cell "notify me on readable/writable for handle H"
   (a `NetRequest::Register{handle, interest}` typed request). Deregister + reregister semantics.
3. **Edge vs level** — pick **edge-triggered with mandatory drain** (mio convention: caller reads until
   `WouldBlock`). Net cell sends one readiness edge per level transition; cell must drain the socket.
4. **Wakeup primitive** — decide (a)/(b)/(c) above; if (b), spec the syscall (nr, args, Boundary-Law note).
5. **Demux contract** — reactor does wildcard `recv_timeout`, classifies each message: readiness (→ fire
   token wakers), reply-to-a-request (→ route to waiting future), or app IPC (→ app queue). Must not
   poison request/reply exchanges (spec 17 §2 hazard — the whole reason spec 17 exists).
6. **Coalescing + fairness** — multiple edges for one handle coalesce; no starvation across handles.
7. **Socket-handle namespace** — the `u32` handle from P2 is the "fd"; define its lifetime, uniqueness,
   and reuse rules (a closed handle's late readiness message must be dropped, not misrouted).
8. **[M1a] Wakeup survives the not-yet-parked window** — a per-task pending-wake flag consumed by
   `recv_timeout` on entry, or `pending_msgs` extended to self-sends; specify which and its semantics.
9. **[M1b] Typed reactor envelope** — length-prefixed postcard type tag outside attacker payload;
   readiness discriminant proven non-colliding with any `R::Data` first byte; raw `0x30-0x32` forbidden
   on the reactor channel.
10. **[M1c] Exactly one recv consumer per cell** — reactor-owns-recv (forwards app IPC to ostd-compat
    queue) or ostd-owns-recv (forwards readiness); `run_with_lifecycle` + reactor in one cell is forbidden
    (compile-time or boot-time assert).
11. **[M6] Frozen `AsCellHandle` ABI** — `As/From/IntoCellHandle` + handle namespace/lifetime/generation/
    reuse; this is the "handle ABI frozen" gate that P3 and P4 both consume.

## Architecture / data flow
```
app future .await ──▶ reactor.wait(): sys_recv_timeout(mask=0, buf, ticks)
   ├─ msg from net tid, byte0=0x11 NET_READY {handle, events} ─▶ fire waker for token(handle)
   ├─ msg = reply to an in-flight NetRequest        ─▶ deliver to the requesting future
   └─ msg = ordinary app IPC                          ─▶ enqueue for app
reactor.notify() (from another thread)  ──▶ (a) sys_send(self_tid, WAKE) | (b) sys_wake_recv(self_tid)
net cell: socket becomes readable ──▶ try_send(app_tid, NET_READY{handle, READABLE})   [§6 discipline]
```

## Related Code Files
- **Create:** `docs/specs/17-ipc-wire-contract.md` amendment (§3 byte-0 row `0x11 NET_READY`; new §10
  "Readiness notifications"; §9 amendment log entry). A prototype `cells/apps/reactor-spike/` (optional,
  ≤200 LOC) validating wildcard-recv + demux + wakeup latency **before** P3 commits.
- **Reference:** net cell `NetRequest`; `libs/ostd/src/ipc.rs` (AsyncRecv), `executor.rs`;
  `libs/ostd/src/app.rs:162,187` (the competing wildcard-recv loop — M1c); `kernel/src/task.rs:1304-1375`
  (try_send / pending_msgs fallback — M1a).
- **Possible (Law/boundary):** `sys_wake_recv` syscall OR a per-task pending-wake flag if self-send can't
  cover the not-parked window (M1a) — kernel `task.rs`/`syscall.rs`.
- **Freeze here (M6):** the `AsCellHandle` trait shape (implemented in P4's `std::os::cellos::io`).

## Implementation Steps
1. Write the protocol spec (all 7 requirements). Circulate for review — this is the gate.
2. Build `reactor-spike`: register interest, block on wildcard recv, receive N readiness edges from a
   stub net cell, demux by handle, measure wake latency; test the chosen `notify()` option.
3. Ratify; feed the frozen contract into P3.

## Todo List (design items drafted 2026-07-23 — see design-p25 + spec 17 §10 draft)
- [x] Byte-0 `0x11 NET_READY` claimed in spec 17 §3 (`0x12 REACTOR_WAKE` too; registry checked — 0x10 input, 0x30-0x32 client→net only)
- [x] Interest register/deregister/reregister request types specified (D8 — `NotifyRegister`=17/`NotifyDeregister`=18, append-only)
- [x] Edge-triggered + drain-until-WouldBlock contract documented (D5 — incl. registration edge for the register-after-data race)
- [x] `notify()` wakeup decision made: **(a′) same-cell `pending_msgs` fallback for byte-0 `0x12`, coalesced — no new syscall** (D1; Boundary-Law: extends existing IPC delivery mechanism)
- [x] Demux contract that preserves spec 17 §2 (D9 — `(sender, byte0)` classifier, 5 rows, fail-loud default)
- [x] Socket-handle lifetime/reuse + stale-message drop rule (D7 — monotonic no-reuse-ever; reactor drops unknown handles)
- [x] (M1a) wakeup survives not-yet-parked window — all 3 recv paths drain `pending_msgs` on entry (`syscall.rs:1016,1194,1266`); SMP caveat: wake may defer ≤ 1 timeout tick → reactor MUST use `recv_timeout`, never bare `recv` (review finding, folded into D1 + spec §10.5)
- [x] (M1b) readiness discriminant proven non-colliding: `NetResponse` byte-0 ≤ 0x0F (normative ≤16-variant cap), attacker bytes at offset ≥ 2; raw fixed 6-byte frame chosen over postcard (O(1) classify)
- [x] (M1c) exactly-one-recv-consumer **per tid** + forbid run_with_lifecycle+reactor (ostd RECV_OWNER claim, startup panic)
- [x] **(M6) handle ABI frozen** — `class:8|id:24` u32, trait quad `As/From/IntoCellHandle` + `OwnedCellHandle` (D7); P3 & P4 consume, never redefine
- [x] User ratification 2026-07-23 (all 4 items: D1 kernel item, byte-0 claims, D7 freeze, D8 Law-1 confirm #1)
- [ ] reactor-spike measures wake latency (post-G3, code — the only remaining item)

## Success Criteria
- Ratified spec 17 §10 amendment (design deliverable — reviewable now).
- `reactor-spike` in QEMU: registers 3 handles, receives interleaved readiness edges, demuxes correctly,
  a cross-thread `notify()` wakes the blocked reactor within one tick. Oracle: `REACTOR-SPIKE: PASS`.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| **`notify()` forces a new syscall** — tension with "kernel additions ≈ 0" | H×M | Try self-send-to-own-tid first (option a); only add `sys_wake_recv` if self-send can't interrupt a blocked recv; frame as IPC mechanism (Boundary-Law §whitelist), not policy |
| Demux misroutes a reply as readiness → spec 17 §8.2 class regression | M×H | Reply messages stay masked/typed; readiness has a distinct byte-0; reactor checks sender tid AND byte-0 |
| Binary mask ⇒ wildcard recv is the ONLY multiplex; a rogue sender can flood the reactor | M×M | Only the net cell + self are expected senders; unexpected sender → log+drop (fail-loud); bound queue |
| Edge-triggered + missed drain = hang (socket readable but no new edge) | M×H | Mandate drain-until-WouldBlock (mio convention); net cell re-arms on level transition; test the race |
| Stale readiness after close misroutes to a reused handle | M×M | Monotonic handle generation or drop-on-unregister; spec the reuse rule explicitly |
| **[M1a] Lost wakeup** (self-send dropped when reactor not yet parked) | H×H | Per-task pending-wake flag consumed on `recv_timeout` entry, or extend pending_msgs to self-sends; test the not-parked race |
| **[M1b] Attacker Data bytes masquerade as a readiness edge** on the shared channel | M×H | Typed length-prefixed envelope; readiness tag proven non-colliding; forbid raw 0x30-0x32 on reactor channel |
| **[M1c] Two recv consumers** (ostd loop + reactor) steal messages (spec-17 §8.2) | H×H | Exactly-one-consumer rule; forbid run_with_lifecycle+reactor via assert; one owns recv and forwards |
| **[M6] Handle namespace drifts** between P3 mio fork and P4 os::cellos | M×H | Freeze `AsCellHandle` ABI here; P4 adds only ext methods; "handle ABI frozen" gate blocks P3 |

## Security Considerations
- Readiness messages are unprivileged signals; they carry no capability. A malicious sender can at worst
  cause spurious wakeups (DoS-lite) — bounded by fail-loud drop of unexpected senders.
- If `sys_wake_recv` is added, it must only wake a tid the caller is entitled to signal (self, or same
  cell) — do not let arbitrary cross-cell recv-wakes become a side channel.

## Next Steps
- Frozen contract (protocol + reactor recv rules + `AsCellHandle` ABI) gates **P2.6** (implements it in
  the net cell) and **P3** (consumes it). No P3 code until `notify()`/demux/consumer/handle are ratified.
