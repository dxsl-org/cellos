# ADR-0021 — Give applications an actor/supervisor library in userspace, not a supervision ABI

> **Status**: Accepted 2026-09-25.
> **Supersedes**: None. Extends [ADR-0015](0015-dual-mode-hybrid-architecture.md) (tier model)
> and implements B0 of `docs/roadmap/beam-parity-backend-roadmap.md`.

## 1. Context

OTP-style recovery already works in Cellos, but only inside `/bin/init`: a static child table
(`cells/tools/init/src/service_table.rs`), one supervision level, `one_for_one`, per-service
policy `Permanent`/`Transient`/`Temporary`, a time-windowed intensity of ≤5 restarts / ~10 s,
and a "give up on that one service" escalation
(`cells/tools/init/src/supervisor.rs`; Spec 12 §4.3). An application that wants its own
supervision tree has no library to declare one — it must edit `init`, or hand-roll the same
loop again.

The tree already contains the scaffolding for a library but nothing uses it:
`ostd::dispatch::MessageHandler` + `run_service` (`libs/ostd/src/dispatch.rs`),
`ostd::service_entry!` + `CellRuntime::run` (`libs/ostd/src/runtime.rs`),
`AppContext::run_with_lifecycle` (`libs/ostd/src/app.rs`), and `ServiceRef`
(`libs/ostd/src/service.rs`, whose `invalidate()` callers must currently remember to call).

Five existing constraints bound any design; none of them is negotiable in this decision:

| Constraint | Source |
|---|---|
| A cell's mailbox is **bounded** (64 slots, 512 for input events) and a full mailbox produces `Backpressure`, not loss | Spec 12 §4.3, `kernel/src/task/tcb.rs` |
| `sys_send` is a **handoff**: the sender parks until that message is consumed, so there is no fire-and-forget send for cells | Spec 17 §2/§6, `kernel/src/task.rs` |
| **Exactly one code path may recv on a given tid**; request/reply recvs are **masked to the peer tid**, wildcard recv belongs to a genuine event loop | Spec 17 §2, §10.6 |
| Byte-0 is a **global discriminant registry**; a new protocol either uses postcard or claims a row | Spec 17 §3, §9 |
| The frame is fixed at `IPC_BUF_SIZE` (4096 B) and replies must fit **after** the envelope | Spec 17 §5 |

The ABI (`libs/api`) is frozen under Law 1. B0 of the backend roadmap is explicitly a
userspace-only step: it must not need a new syscall, a new opcode, or a new message byte.

## 2. Decision

### 2.1 The library is userspace-only and claims no new wire namespace

`ostd::actor` (actor loop, typed dispatch, call/reply helpers) and `ostd::actor::supervisor`
(child specs, restart policy, strategies, intensity, backoff) ship in-tree in `libs/ostd`.
Nothing in `libs/api` changes, so Law 1 is untouched.

Actor messages ride the **existing** `0xAC` app envelope with event byte `0x00` (Message);
`0xFF` (Shutdown), `0xF2` (CapRevoked) and the hotswap bytes keep their current meaning. No
new byte-0 value is claimed, so Spec 17 §3/§9 need no amendment. A message whose byte 0 is not
an envelope the actor understands is **rejected loudly** — never silently dropped (§7);
a cell that wants to serve a legacy raw protocol keeps its own bespoke loop.

### 2.2 An actor is an event loop, and its recv mask discipline follows Spec 17

An actor parks in a wildcard `sys_recv`/`sys_recv_timeout` on its own tid (the one sanctioned
use of wildcard recv, §2) and classifies what arrives: envelope messages, kernel events, and —
for a supervisor — the resume of a watched child. When an actor has a deadline (a supervisor's
backoff timer, a heartbeat) it uses `sys_recv_timeout` with the nearest deadline rather than
polling.

Outbound request/reply stays **synchronous and masked** to the peer tid through the existing
`ostd::ipc` helper, and is only issued from the actor's own thread: one recv consumer per tid
(§10.6). Concurrent in-actor calls are B1's reactor work, not this decision.

### 2.3 The supervisor mirrors `init`'s semantics and adds a dynamic table, backoff, and strategies

- **Dynamic child table.** Child specs are declared by the application (a slice or a builder),
  not compiled into a kernel/agent table, so a high-churn child class never enters `init`'s
  restart budget.
- **Policy** keeps Spec 12 §4.3's three values and meanings: `Permanent` (always restart),
  `Transient` (restart only on abnormal exit), `Temporary` (never restart).
- **Intensity** is per child (default ≤5 restarts / ~10 s). Escalation **gives up on that child only**,
  logs the reason, and leaves every other child running — the same observable behaviour `init` has
  today. The window is measured in **scheduler ticks** (`GetTime` op 4, the clock the kernel itself
  uses for `RecvTimeout` deadlines). `GetTime` op 0 is the raw architected counter and must not be
  used for a budget or a window: its units differ per architecture, and a window compared against it
  silently never engages — which is what the shipped `init` supervisor does today (recorded as a
  finding in the B0 plan, not fixed here).
- **Backoff** delays a respawn with a capped, deterministic, logged delay instead of
  respawning in a tight loop.
- **Strategies** beyond `one_for_one` are available (`one_for_all`, `rest_for_one`), evaluated
  in declaration order, because a library that owns the child table can offer them without
  touching `init`. Strategy semantics are per-supervisor, not global.
- **Exit reason** comes from the `NotifyOnExit` resume payload (Spec 12 §4.3, no new ABI).

### 2.4 Monitoring stays gated on `SpawnCap`, and addressing stays `u16`

Roadmap §8 left two decisions open before code; both are resolved **conservatively** here,
because opening either one is a Law-1 ABI change and B0 forbids ABI changes:

1. **Monitor scope = `SpawnCap` holders only.** `NotifyOnExit` keeps requiring `SpawnCap`
   (`kernel/src/task/syscall.rs`). A supervisor is therefore a signed, capability-bearing
   cell. Widening monitoring to any cell becomes its own proposal.
2. **Namespace = the existing `u16` service id** (registry) or an explicit tid. No dynamic
   name registry and no `/svc/...` VFS namespace in B0; a later lane may propose one.

### 2.5 `ServiceRef` already self-heals on a dead peer, and B0 keeps it that way

The roadmap's B0 item asked for automatic `invalidate()`-on-restart. Reconnaissance found
that this is **already shipped**: `ServiceRef::call` invalidates its cached TID when the
transport reports `Send`, `Recv`, or `WrongSender`, and the next call re-resolves through the
service registry (`libs/ostd/src/service.rs:120-129`, `:78-91`). What remains is *policy*, not
code: a caller still has to retry the call it lost, because the failed request may already have
been admitted by the peer. This decision therefore records the existing behaviour as the
contract rather than adding a hidden retry — a transparent retry would need a delivery
guarantee the kernel does not offer (Spec 17 §6).

## 3. Consequences

- An application can declare a supervision tree without editing `init` or the kernel; the
  restart/backoff/give-up semantics are the ones the project already proved in `init`.
- Accepted costs, recorded rather than hidden: **no** unbounded mailbox (bounded backpressure
  is the point), **no** fire-and-forget post for cells (the handoff send is the ABI's
  semantics), and **no** intra-cell concurrency — B0 actors are single-threaded event loops
  until B1's reactor lands.
- Strategy evaluation order is the child declaration order; a supervisor with a
  `rest_for_one`/`one_for_all` policy therefore has a defined, documented order rather than an
  incidental one.
- **Placement is part of the contract for an authority-bearing cell.** A supervisor holds
  `SpawnCap`, and `launch_profile::authorize` refuses a non-empty child ceiling on the
  `SpawnFromElf` route — caller-supplied bytes must not borrow a profile that carries authority.
  A disk-only copy therefore cannot be launched from userspace; the supervisor must be staged in
  VIFS1 (the kernel-embedded image), which is also where the hotswap demos live. Children of a
  supervisor are unaffected: they are capability-free, so the supervisor spawns them through VFS
  and `SpawnFromElf` from the ordinary disk cell-store.
- Evidence ceiling: B0 is witnessed at `qemu`. It claims no per-request scale (B2), no
  multi-connection concurrency (B1/B3), and no production trust posture (B7).

## 4. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| Keep supervision inside `init` and let apps extend its static table | Apps cannot declare a tree; per-request churn would spend `init`'s restart budget; `init`'s table is not dynamic |
| Add ABI ops (fire-and-forget post, open monitor, per-actor mailbox) | Law 1 change, and B0's own acceptance needs none of them; the handoff send + `NotifyOnExit` + `sys_wait` already carry the required semantics |
| BEAM-style unbounded mailbox and non-blocking `!` | Removes the backpressure signal that keeps one slow consumer from exhausting the machine (roadmap §7) |
| Kernel-level supervision/monitor server | Moves policy into the TCB for no capability gain; Spec 12 §4.3 already places it in userspace |

## 5. References

| Item | Location |
|---|---|
| Program roadmap (B0 scope, §8 open decisions) | `docs/roadmap/beam-parity-backend-roadmap.md` |
| Implementation plan | `.agents/260925-2214-beam-parity-b0-actor-supervisor/plan.md` |
| Supervisor semantics precedent | `docs/specs/12-reliability.md` §4.3, `cells/tools/init/src/{supervisor,service_table}.rs` |
| IPC rules this design obeys | `docs/specs/17-ipc-wire-contract.md` §2, §3, §5, §6, §7, §9, §10.6 |
| Tier model | `docs/decisions/0015-dual-mode-hybrid-architecture.md` |
