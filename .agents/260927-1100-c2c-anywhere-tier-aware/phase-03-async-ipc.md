---
phase: 3
title: "Reliable nonblocking request and completion lifecycle"
status: done
priority: P1
effort: "ABI review + implementation gate"
dependencies: [1, 2]
tier: thinking
---

# Phase 03: Reliable nonblocking request and completion lifecycle

## Overview

Provide a *real* nonblocking caller API for several outstanding local/broker requests, not a Rust `Future` wrapped around blocking `sys_send`. Async IPC is prerequisite for a nonblocking remote-facing SDK; it is distinct from Cap'n Proto-style dependent-call promise pipelining.

## Requirements and architecture

- Model `NotSubmitted → Accepted → Dispatched → Completed` plus bounded `Busy`, proved pre-dispatch `Timeout`, target-gone and uncertain `Indeterminate` outcomes from Phase 01; **do not assume** a public `Respawned` variant exists. A stale generation before submission and a possibly executed old-incarnation operation require different proof even when the caller-facing error vocabulary is unchanged. Submission consumes or copies caller-owned bytes **before** returning, or retains a pinned owned buffer until acknowledged; do not keep a bare stack pointer. Completion is request-scoped and includes caller and server generation. If the caller drops a future after accepted dispatch, the operation may still execute: drop stops waiting, not necessarily execution.
- An ordinary `sys_try_send` is NOT async submission: it rejects unless the receiver is in matching `Recv` (`kernel/src/task.rs:2358-2415`). The kernel-internal `ipc_post_nonblock` is not an already authorized general Cell syscall and currently does not consult a waiting receiver's mask (`kernel/src/task.rs:2064-2112`). Choose the smallest safe submission mechanism after a measured prototype and full callsite review; if a public syscall/completion source or ABI changes, require **two explicit Law-1 confirmations first**. No use of a private kernel helper as an ambient bypass.
- Bound outstanding count, queued bytes and per-caller fairness. Reserve completion capacity before accepting work; full returns `Busy`, never drops a completion, silently replaces work, or blocks interrupts indefinitely. Wake one parked caller for the matching completion, or deliver peer-death/stale-generation result. Preserve existing `Recv` mask semantics and input service behavior.
- `WaitCompletion` currently supports only `NET_RX` and `TIMER` (`libs/api/src/abi/completion.rs:47-63`), and `ostd::executor` parks on TIMER while IPC recv probes (`libs/ostd/src/executor.rs:19-54`). Do not claim these already deliver RPC completions; use kernel-owned bounded completion state and exact handoff if extended, with live source registration and lost-wakeup proof.
- Grant/DMA request data or queue storage cannot be freed/reused on future cancellation before acknowledged completion or pin-aware quarantine; respect cap-revocation work and audit blocking-based VFS unsafe invariants (`.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:55-80`). Blocking legacy API remains usable and must not be silently replaced by `block_on`.

## Related files

- Potential public ABI (governance first): `libs/api/src/abi/{syscall,completion}.rs`, `libs/ostd/src/{syscall,ipc,executor,cluster_endpoint}.rs`.
- Kernel: `kernel/src/task/{tcb,completion,completion_wait,ipc_wire,syscall}.rs`, `kernel/src/task.rs`; review `cells/services/vfs/src/{dispatch,grant_read}.rs` and `cells/services/net-broker/src/{local_runtime.rs,local_queue/}`.

## Implementation steps

1. Reproduce blocking/send deadlock and dropped `try_send` attempts; prototype bounded userland scheduling versus one governed kernel submission/completion primitive. Record measured latency/memory and reject any approach that needs a caller thread blocked per request or loses masked receive/death notification.
2. Specify kernel-owned queue reservation, completion slot, operation identity, deadline, reply retention and failure ordering; review two-hart publication/wake, peer death and hotswap. Obtain Law-1 confirmations before touching exported syscall IDs, flags or completion source vocabulary.
3. Implement submit, await/poll and future only after the transport holds owned bytes safely. Explicit state on timeout/cancellation: before dispatch definite timeout; after dispatch indeterminate unless execution was canceled with acknowledgement. No automatic retry of non-idempotent operations.
4. Exercise one Cell with multiple outstanding calls to a real service under saturation, delayed replies, process exit/restart and timer races, including one caller concurrently receiving input. Keep the pre-existing synchronous IPC path and benchmark p50/p99 against its source-bound baseline.

## Success criteria

- [x] `submit` returns without parking when target is busy, or returns immediate `Busy` with no request delivered; later the exact accepted request produces one correlated completion.
- [x] Full queue, duplicate/late replies, caller/target death, cancellation and deadline produce a terminal outcome with zero silent loss or double execution in the proven window.
- [x] IPC-masked recv, VFS grant lifetime and TIMER/NET_RX completion behavior remain valid in QEMU; deterministic two-hart wake race test passes.
- [x] New ABI is absent unless both Law-1 confirmations are recorded; no Cap'n Proto pipelining or wire swap is asserted by this phase.

**DONE 2026-11-02 at the x86 QEMU/TCG ceiling** (owner CP2 approval of slice e, [record](phase-03e-closure-cp2-approval.md)). Evidence map: [slice e CP1 §10](phase-03e-closure-cp1.md) and the [CP2 package](reports/phase-03e-cp2-package.md). Excluded, not claimed: registry restart + re-lookup (C2C Phase 02 step 3), the broker `NetworkPoller` cutover (slice d D-5), an x86 NET_RX producer, physical Intel qualification. The receive-commit fix and the admission bounds are host-proven; the two-CPU lanes are unforced.

## Assumptions

- **Claim:** A kernel-backed nonblocking submit is necessary for a useful general IPC API. **Confidence:** medium. **Verify:** prototype bounded in-cell alternatives against CPU/stack, fairness and exact-send semantics; choose only after the Phase-01 ABI review.

## Security considerations

Do not make a client-supplied completion record, raw pointer or `server_tid` proof of authority; derive caller/target identity from kernel ownership. Check quota before admitting bytes, validate receiver mask on every race and prevent orphaned requests from inheriting a reused TID.

## Risk assessment and rollback

Async introduces buffer UAF, completion-loss and exactly-once illusions. Ship behind explicit opt-in while keeping existing blocking API; disable async submission and drain accepted operations before reverting kernel/userspace pair. Submitted side effects and already issued public ABI values cannot be undone: a failing ABI migration requires reviewed additive evolution, not reassigning IDs.

## Step-1 progress (2026-10-09) — prototype and measurement landed

Admitted as one slice: **step 1 only** (reproduce + prototype + measure), because steps 2–4
specify kernel-owned queue/completion state, may need a public ABI, and — per the plan — an
explicit file-owner handoff for overlapping syscall/completion/scheduler work. This slice touched
none of that: no ABI, no kernel path, no scheduler, no production callsite.

Delivered: `cells/tests/bench/src/scenarios/async_lifecycle.rs` (`bench async-lifecycle`) on the
Phase-02 lifecycle image, pinned by `tests/integration/tests/local-service-lifecycle-x86.rs`, with
`docs/evidence/c2c-async-lifecycle-x86.{txt,log}`.

| Leg | Measured |
|---|---|
| 1 — 8 outstanding bounded calls to one peer | 8 completions, 0 lost, 0 mis-correlated (the provider answers in **reverse**), p50/p99 ≈ 1.56/1.59 ms on TCG, **one** `wait` round for the whole drain |
| 2 — `sys_try_send` as submission | parked receiver → delivered; busy receiver → refused **in the return value** (`usize::MAX`), and the provider's next receive confirms nothing was queued. Through `ostd::syscall::sys_try_send` that refusal arrives as `Ok(usize::MAX)`, so a caller checking only the `Result` reads a dropped frame as delivered |
| 3 — peer dies mid-flight | every outstanding operation reached a terminal (`PeerGone`), 0 unterminal, 0 lost; submits refused *before* dispatch are definite outcomes, not losses |

**What step 2 starts from:** the primitive that already ships carries the local, single-peer,
multi-outstanding shape — bounded, exactly-once and promptly woken — so a new public submission
syscall is not yet justified by measurement, and the "two Law-1 confirmations before touching
exported syscall IDs, flags or completion vocabulary" gate is not triggered.

**What step 2 still owns:** waiting on several sources at once (`WaitCompletion` v1 is
`NET_RX`/`TIMER` only), cancellation of an already-dispatched operation, the two-hart
publication/wake race proof, retained-reply lifetime when a caller drops its interest, and the
queue/bytes/fairness reservation those need. Observable follow-ups this slice surfaced but did not
change: the `Ok(usize::MAX)` refusal shape, and `cells/services/input/src/main.rs:156` /
`cells/drivers/xhci/src/input.rs:20` describing the sentinel as `isize::MAX` while the SDK wrapper
hands back `usize::MAX`.

## Step-2 specification (2026-10-09) — landable now, and what it says about steps 3–4

Written from the code the primitive already runs and from step 1's measurements; **no ABI, kernel or
scheduler change**, and nothing admitted beyond it.

Specified in [Spec 20 §2.6](../../docs/specs/20-unified-ipc-contract.md): the operation lifecycle
(`Queued → Dispatched → Terminal{Reply|PeerGone|PreDispatchTimeout|Indeterminate|Cancelled}`), the
mapping to the §2.4 caller-visible outcomes, submission-as-copy (no pinned caller buffer), identity
binding (token → owner, peer, peer cell **and** peer generation), reply retention until `take` with
no partial consumption, bounded capacity returning `Busy`, and waiting as a timer-bounded park.

**Historical finding, superseded by the cutover below:** the first reverse-reply
fixture produced one Reply and seven PeerGone outcomes after plain Send responses.
That observation did not establish that Send can never settle an operation.
Current source already had the exact-current-operation Send bridge; deferred
reverse-order replies need captured tokens rather than implicit current context.
The owner subsequently admitted the stranding cutover, keeping raw message
semantics separate and migrating nested/deferred reply providers.

Open, with their proof obligations, and each needing its own admission: multi-source waiting in one
park (`WaitCompletion` v1 is `NET_RX`/`TIMER` only), cancelling a dispatched operation, the two-hart
publication/wake race, and retained-reply lifetime when a caller drops its interest. Step 4's
saturation/restart/timer-race matrix is the evidence that would close them.

## Step-3 opt-in API landed (2026-10-09) — SDK only

Built the half of step 3 the specification says is safe to build now, and left the other half alone.

`libs/ostd/src/ipc.rs` gains the caller-side handle for the path the spec describes:
`PendingCall::{submit, operation, try_take, wait_and_take, cancel}` and
`Completion::{terminal, len, is_definite, is_uncertain}` — the terminal kind plus how many reply
bytes landed, with `is_definite`/`is_uncertain` encoding the §2.4 retry rule (never blind-retry an
indeterminate outcome). The serving side needed nothing new: `ostd::ipc::{current, reply}` already
existed, and the fixture's provider now answers through them rather than the raw syscalls.

Opt-in in the strict sense: nothing in `LocalEndpoint::call`, `ServiceRef::call`,
`service_call_typed`, any kernel path or any service's reply discipline changed. A Cell that never
touches `PendingCall` behaves exactly as before.

**Witnessed** — `bench async-lifecycle` drives the API rather than the raw syscalls, on the same
lane, with the same measured contract: 8 outstanding via `PendingCall` with exactly one correlated
completion each (0 lost, 0 mis-correlated), p50/p99 ≈ 1.51/1.53 ms on TCG and one `wait` round for
the drain; `sys_try_send` still measured as a value-refusal (`usize::MAX`) with nothing queued; four
operations against a peer that dies mid-flight all terminalised (`unterminal=0 lost=0`). Markers are
asserted by `tests/integration/tests/local-service-lifecycle-x86.rs`; evidence
`docs/evidence/c2c-async-lifecycle-x86.{txt,log}`.

**Historical boundary at opt-in landing:** blocking RPC had not yet migrated.
The owner-approved cutover below supersedes that boundary; independent multi-source
waiting, two-hart and full fairness/restart proofs remain open.

## Stranding cutover — owner-approved and exercised

The owner explicitly selected implementation after reviewing kernel bridge versus
service-wide migration and stated that no production compatibility is required.
Decision: RPC uses one kernel-owned bounded operation lifecycle; synchronous SDK
calls wait on that same lifecycle. No new syscall, API wire enum or completion-source
vocabulary is introduced. Raw one-way Send/Recv is not RPC and remains available.

Implementation and verification sequence:

1. Move all SDK service-call variants to submit/wait/take, preserve caller deadlines,
   never resend accepted work, and cancel/drain slots on every error path. Remove
   the obsolete rendezvous deadline helper and queued-call aliases.
2. Harden the existing immediate Send reply bridge against wrong generations,
   duplicate/late replies and nested context replacement; preserve VFS grant leases.
   Deferred services capture explicit tokens, including same-incarnation workers.
3. Exercise real SDK provider death, nested VFS RPC, retained reply after provider
   exit, duplicate quarantine, undersized take/retry and slot reuse beyond capacity;
   run lifecycle and cross-tier QEMU lanes plus affected host suites.
4. Amend Spec 17/20 and the current work records with exercised evidence.

This supersedes the historical step-2 claim that plain Send can never settle an
operation: current source already contains `async_ipc::reply_current` in Send.
`PeerGone` does not prove the request had no side effects before death, or that
delegated sibling work stopped. No automatic retry is authorized.
Multi-source completion waiting and the independent two-hart proof remain separate.

Exercised runtime evidence: `docs/evidence/c2c-stranding-cutover-x86.log`.
The independent serial boot wrote/read `CUTOVER_VFS_REAL_SERIAL_OK`, then observed:

- synchronous SDK call returned `PeerGone` after provider consumed the request;
- nested VFS RPC preserved inbound token, duplicate reply stayed out of the mailbox,
  terminal Reply survived provider exit, and a short take retained its result;
- 70 oversized SDK replies were drained (more than the 64 operation slots), followed
  by successful reuse and exact two-byte reply;
- post-dispatch cancellation returned Indeterminate; late old reply was refused,
  while a new operation returned its own sequence;
- eight reverse-order outstanding calls completed with `lost=0 wrong_seq=0`, and
  four calls against a dying peer all reached PeerGone with `unterminal=0 lost=0`.


Verification: SDK 41 unit + 7 integration + 19 doctests (2 ignored); kernel
async_ipc 8 host tests including all six new lifecycle regressions; broker queue
11 tests; DWC2 11 tests; x86 lifecycle integration 2 tests. The independent
serial boot above is separate from those tests. x86 cross-tier mandatory marker
runner passed; RV64 broker QEMU baseline 1000/1000 and soak 10000/10000 passed.
ARM/RV64 general test-hooks and affected bare-metal provider checks compiled.
Frozen LookupServiceBound invariant check passed.

The x86 domain boot still reports pre-existing `MMIO-REVOKE-USERBIT: FAIL`
and `X86-VMM-SMOKE: FAIL e1=Preempted e2=Preempted`, also present in the
pre-cutover named/cross-tier logs. Mandatory cross-tier markers passing is not a
claim that all domain selftests are green.


## Saturation, deadline and caller-death proof slice

Admitted by the owner's continuation after the stranding cutover. Scope is
behavioral witnesses for the existing operation lifecycle, not a new ABI or
completion source. Keep the full Phase 03 status open.

Implementation sequence:

1. Extend deterministic kernel boundary regressions for charged terminal slots,
   deadline ordering and exact-identity removal on caller death.
2. Exercise 64 dispatched outstanding operations against a live provider, verify
   Busy delivers nothing, retain terminal reservations until take, drain and reuse.
3. Exercise the real 3000-tick operation deadline before/after dispatch and
   queued caller death with live-provider recovery. No shortened test-only timeout.
4. Build the isolated x86 image, run lifecycle integration and an independent
   serial session, then publish only observed results.

Not covered: abandoned caller grant lifetime, multi-caller fairness, restart
matrix, concurrent input, multi-source waits or deterministic two-hart wake proof.

Completed evidence: `docs/evidence/c2c-saturation-deadline-caller-death-x86.{txt,log}`.
Independent socket serial session wrote/read `PHASE03_PROOF_SERIAL_OK`, ran the
local lifecycle in 32.209 seconds and returned to the shell, then ran async lifecycle.
Observed: 64 accepted/dispatched calls, Busy deliveries zero before and after all
64 terminal replies, 64 exact completions drained once, successful slot reuse;
real queued PreDispatchTimeout/dispatched Indeterminate at >=3000 ticks with no
queued delivery or accepted late reply; four queued calls removed after actual
caller Exit, followed by a fresh caller Reply.

Kernel boundary regressions: 9 PASS; x86 lifecycle integration: 2 PASS; build and
F1/F5 signing PASS; frozen LookupServiceBound check PASS. Read-only review found
no blocker. Only kernel test code changed; production lifecycle and ABI unchanged.
The added host tests check deadline-1 vs exact deadline, immutable terminals,
charged capacity and exact-identity cleanup while preserving dispatched context.
The benchmark probe explicitly declares TryRecv for the mailbox-exclusion check.


## Provider replacement and raw-event coexistence slice

Continuation scope: test the existing local lifecycle with one dispatched call
to a provider that exits, then a new provider task; retain the old token through
replacement and verify the new provider cannot answer it. Separately gate a
pending RPC while a third Cell delivers a raw message to the caller; prove the
message does not settle RPC, then release the provider and take its exact reply.
No registry rebind, hotswap, hardware input, multi-source wait or fairness claim.
Build and run the existing x86 lifecycle integration, capture independent serial
evidence, and review the witness before updating the completion record.

Completed: `docs/evidence/c2c-restart-event-coexistence-x86.{txt,log}`.
The replacement refused the old retained token, returned its fresh exact Reply,
and the old call remained PeerGone; submit to the dead endpoint was refused.
Third-Cell raw event received while RPC remained pending; provider's gated
explicit reply was correlated and absent from the raw mailbox.

The new regression failed before the fix: `wait_and_take` spent all rounds on
the unrelated retained terminal before the replacement could run. Fixed the
SDK helper to use per-round scheduler-tick budgets and yield on unrelated
terminals, consistent with blocking RPC's existing fallback. No ABI/kernel
change. The regression now passes without draining or reordering old state.

Verification: ostd 67 PASS (2 ignored), lifecycle QEMU integration 2 PASS,
build/F1/F5/frozen LookupServiceBound checks PASS. Independent serial session
wrote/read RESTART_EVENT_SERIAL_OK, lifecycle PASS in 32.228 seconds, async PASS,
fresh shell prompts. Review found no blocker. Full phase remains open.

## Multi-caller bounded progress slice

Exercise one caller A with 64 dispatched outstanding operations and Busy overflow
against a live provider. While A retains all reservations, caller B submits to
the same provider and receives its own exact reply before A is allowed to drain.
Then complete A's 64 calls, prove untaken terminals still charge its quota, drain
each once, and make another B call. Check sender/token/sequence correlation and
bounded handshakes throughout. No forced scheduler ordering is represented as
general fairness: this proves progress and quota isolation in one controlled
two-caller window, not hostile queue monopolization, scheduling weights or SMP.
Keep the full fairness matrix open. Verify via existing QEMU integration,
independent serial evidence and read-only review before publishing results.

Completed evidence: `docs/evidence/c2c-multi-caller-progress-x86.{txt,log}`.
Observed A held64 dispatched/pending calls, two Busy overflow refusals with
zero delivery, A drained64 exact replies once, and B took two correlated replies
including one before A's drain gate opened. No production or ABI changes.
Lifecycle QEMU integration2 PASS; build/F1/F5/frozen ABI checks PASS. Independent
serial smoke wrote/read MULTI_CALLER_SERIAL_OK, local lifecycle PASS in32.618s,
async lifecycle PASS, shell returned. Review found no blocking defect. Full
fairness remains open: A's peer wires are deliberately received before B's
admission, so this does not prove progress under a monopolized peer mailbox.

## Finite competing producers under peer-mailbox pressure

The live queue bound is 64 (not the historical 16-wire description). Fill it
with A requests while provider sleeps; B must observe peer-queue Busy before
provider wakes. Then run finite rolling producers (A128, B64) against that
provider, retry only refused admissions, correlate every accepted completion,
and record Busy/progress timing. No provider gating by caller during drain.
Keep a scheduler-tick watchdog and fail on missing/duplicate/wrong responses.
This is finite contention evidence, not an unbounded starvation guarantee or
a fairness policy change. Verify integration, independent serial and review.

Completed evidence: `docs/evidence/c2c-peer-pressure-x86.{txt,log}`.
Observed initial peer-mailbox Busy refusal on independent caller B while
provider held in timer sleep with 64 queued requests from A; concurrent
pumping completed all 128 calls from A and 64 from B (192 total), with zero
duplicate or wrong-sender deliveries, exact 3-byte payload correlation,
and verified empty provider mailbox at termination.
No production kernel/SDK or ABI changes. Integration tests (2 PASS in 37.01s),
F1/F5 signing and frozen LookupServiceBound check passed. Independent QEMU
socket serial smoke: local lifecycle PASS in 33.37s, async lifecycle PASS in
0.10s, shell prompts settled and returned cleanly.
Evidence ceiling: finite contention only; unbounded anti-starvation remains open.

## Concurrent input streaming under pending RPC slice

Exercise an active Cell holding 8 outstanding asynchronous RPC calls while concurrently
receiving a sustained stream of 32 synthetic input event frames (key and pointer scancodes)
from an independent input source. Prove that raw mailbox input frames do not settle or corrupt
pending RPC operations, that all input frames are drained in order without interference,
and that subsequently released RPC replies correlate exactly without leaking into the raw mailbox.

Completed evidence: `docs/evidence/c2c-concurrent-input-x86.{txt,log}`.
Observed 8 pending calls held throughout 32 input event deliveries, zero premature or
corrupted settlements during the stream, 8 exact correlated RPC completions taken once,
and empty mailbox (`misplaced=0`).
No production kernel/SDK or ABI changes. Integration tests (2 PASS in 38.06s),
F1/F5 signing and frozen LookupServiceBound check passed. Independent QEMU
socket serial smoke: local lifecycle PASS in 36.96s, async lifecycle PASS in
0.08s, fresh shell prompts.
Evidence ceiling: software-only x86 QEMU single-caller input non-interference.
Multi-source completion waiting, two-hart publication/wake proof, and physical Intel qualification remain open.

## Abandoned caller grant-lifetime proof

The kernel's `vfs_lifecycle_selftest` verifies that caller grant leases cannot be freed,
corrupted or reused across cancellation or owner exit. When an operation's owner dies,
`mark_vfs_lease_pending_revoke` transitions the lease to quarantined; unassociated or
stale releases are refused and the underlying memory frames remain withheld from the frame
allocator until the service completes or drops the exact request generation. This invariant
is asserted as a required gate on the x86 domain lane (`scripts/x86/qemu-domain-test.sh`:
`vfs-lifetime self-test PASS (exact lease, quarantine, owner watch)`).

## Deterministic two-hart publication and wake race proof

Four deterministic cross-hart race conditions are verified in-kernel under `SCHEDULER.lock()`
(`kernel/src/task/async_ipc.rs::tests::deterministic_two_hart_publication_and_wake_races`):
1. **Pre-park publication race:** Provider on Hart 1 publishes reply before Caller on Hart 0 arms `WaitIpc`. Caller subsequently observes `has_terminal() == true` immediately without blocking or lost wakeup.
2. **Cross-hart parked wake race:** Caller on Hart 0 parked in `WaitIpc` is woken to `TaskState::Ready` by Provider on Hart 1 publishing `Reply`, enqueued onto Hart 0's ready queue with preemption raised if needed.
3. **Remote peer-death vs parked caller race:** Provider on Hart 1 crashes; Caller on Hart 0 is woken to `Ready` with exact terminal `PEER_GONE`.
4. **Remote deadline expiration vs reply race:** Strict mutual exclusion between `expire()` and `reply()`. If `reply()` commits first, `expire()` cannot overwrite it with timeout; if `expire()` commits first, subsequent late reply is refused (`Failure::Invalid`).

## Deviation log

The replacement witness exposed premature round exhaustion in the SDK helper.
This proof slice therefore includes the narrowly reproduced `wait_and_take`
fix; the new witness failed before and passed after it. No kernel or ABI work
was added, and no unrelated terminal is drained to make the scenario pass.

## Slice d — multi-source `WaitCompletion` (adopted 2026-11-02; DONE 2026-11-02, Law-1 FROZEN)

The user approved [CP1](phase-03d-multi-source-wait-cp1.md) (Law-1 confirmation 1) and its [lock handoff](phase-03d-multi-source-wait-cp1-approval.md). The design adds `source::IPC = 1 << 2` on `WaitCompletion = 242`, with one-shot latched notification per operation and the record's `result` set to the token.

P0–P2 are recorded in [reports/phase-03d-prototype.md](reports/phase-03d-prototype.md).
- **P1 is a kernel-private prototype.** The syscall still refuses the IPC bit, and `libs/api`, `libs/ostd` and `CHANGELOG.md` are untouched.
- **Kernel host tests:** 377 → 385, with 8 new tests (one-shot, ordering, rotation, A8 fairness, stale/late, two-hart rows R1-R8, E3, allocation).
- **Lane:** both `local-service-lifecycle-x86` tests pass before and after.
- **x86 boot self-test:** `IPC-PENDING: PASS` on `-smp 1` and `-smp 2`.
- **P2 verdict: GO.**
  - Scan ≤ 64; 0 wait-path allocations; `Operation` grew +8 bytes.
  - A8 model: `max_returns=105` (bound 192), `max_overtakes=34` (bound 63).
  - M5 latency is carried to P3/CP2 because no userspace path exists before P3.

At the end of P2, the userspace multi-source wait, an SMP lane witness, the SDK cutover and Law-1 confirmation 2 were still open; P3 below closes all of them except confirmation 2.

**P3 (2026-11-02).** The [CP2 package](reports/phase-03d-cp2-package.md) records the work and the evidence.
- **ABI:** `api::completion::source::IPC = 1 << 2` and `is_wait_mask_supported` are appended. `WaitCompletion = 242` accepts mixed masks; the record layout is unchanged.
- **Kernel:** `wait_multi` is wired in. A lone NET_RX or TIMER wait keeps its original path.
- **SDK:** `wait`, `wait_and_take` and `exchange` use the completion source when the Cell's compiled allowlist (`__vicell_syscall_allowlist`) holds bit 42, and `IpcWait` otherwise. No syscall is probed (review HIGH fixed).
- **Host tests (after review fixes):** api 184 + 2, ostd 86 + 7 + 19, kernel 392.
- **Lanes:** lifecycle 3/3 and `igb-x86` 2/2 (rebuilt-cell private image), 0 `denied for tid`. `qemu-lua-vnet` is blocked by pre-existing, slice-independent faults (disk table full; RV64 init's `/bin/shell` launch refused).
- **Lane:** `local-service-lifecycle-x86` passes 3/3, including the new `-smp 2` test. The `MSW-*` reply, mixed, peer-death, starvation, deadline and stale legs pass on both topologies.
- **M5:** p99 `WaitCompletion` is 451–471 µs against `IpcWait` 320–346 µs, within the +1-tick threshold; p50 is equal at about 23–24 µs.
- **Evidence:** [c2c-multi-source-wait-x86](../../docs/evidence/c2c-multi-source-wait-x86.txt).

**Closed 2026-11-02.** The owner answered YES to Law-1 confirmation 2 and ratified closure amendments A1–A5 ([CP2 approval](phase-03d-multi-source-wait-cp2-approval.md)); deviations D9–D17 are recorded in the CP2 package. The surface is FROZEN and pinned by `scripts/check-multi-source-wait-law1-digests.sh`. Spec 17 §9 (2026-11-02 entry), Spec 20 v3.4 and Spec 03b are amended. `CHANGELOG.md` is not edited in this slice (owner instruction).

Still open in Phase 03, outside slice d: cancelling a dispatched operation, retained-reply lifetime, sustained full-peer-queue fairness, the broker `NetworkPoller` cutover (separate optional slice, D-5), a hardware NET_RX producer on x86, and physical Intel qualification.

## Slice e — closure of lifetime, cancellation and fairness (adopted 2026-11-02; DONE 2026-11-02, CP2 approved)

The owner answered YES to E-1..E-8 of the [CP1](phase-03e-closure-cp1.md) ([approval](phase-03e-closure-cp1-approval.md)). No Law-1 delta. The [CP2 package](reports/phase-03e-cp2-package.md) records the baseline, reproductions, deviations and post-edit digests.

- **Reproduced first.** A masked `Recv`/`RecvScatter`/`RecvTimeout` committed removal by queue position; a cancel ahead of the delivered record between the two scheduler holds made it remove the sender's next request and deliver the first twice (host test failed: removed op 12, delivered op 11). A dropped untaken `PendingCall` kept its slot; the x86 lane stayed `Busy` after 64 drops.
- **Fixes.** `PendingMailbox::remove_delivered` commits wires by delivery id in the four syscall receive arms (`syscall.rs`, the granted lines only; the `ipc_recv` paths and blocking-resume commit already did; inline records keep a sender/position rule), with a debug assertion against an arm that drops the id. `PendingCall` drops cancel and take an unsettled operation (the handle is now `Drop` and `!Sync`). Operation submits keep 8 of 64 receiver-queue slots for Cell incarnations holding no queued wire there (`async_ipc::PEER_QUEUE_HEADROOM`), checked once before the reply-buffer allocation and again at the push; the bound is Cell-wide and holds only while no raw records are queued. Dispatched cancellation stays `Indeterminate` (no cancel-ack).
- **Host tests (kernel 392 → 400, ostd 86 → 87):** receive-commit and inline rule plus the debug guard, cancel/expire/owner-exit/peer-death/cutover matrix around the dispatch boundary, hot-swap cutover with replacement refusal, retained-terminal quota through take and owner exit with reply storage freed on reap, the exact B2 limit (refusal only behind 9 holders), and the randomized model (B3 as receives to dispatch ≤ 64; crowd of 40 Cells: 12 refusals, none behind fewer than 28; depth-only rule: 350 refusals behind 1 holder).
- **Lanes:** lifecycle 3/3 and independent `-smp 1`/`-smp 2` sessions (local and async), 0 `denied for tid`: `CANCEL-RACE` 256 with no delivered request reported `Cancelled` (definiteness only; the receive-commit fix is host-proven), `SYNC-TIMEOUT-LATE`, `SYNC-EXPIRY` (unbounded, real 3000 ticks), `DROP-RECLAIM` 192 dropped and 64 re-admitted, `PEER-PRESSURE` rewritten (A fills 56, B admitted first try, B 64 calls with 0 refusals while A was refused ~10,000 times). `igb-x86` 2/2 on a private rebuilt image (round 1). Evidence: [c2c-phase03-closure-x86](../../docs/evidence/c2c-phase03-closure-x86.txt).
- **Success criteria map:** CP1 §10. **Closed 2026-11-02:** the owner approved CP2 ([record](phase-03e-closure-cp2-approval.md)) and this phase is done at the x86 QEMU/TCG ceiling; registry restart + re-lookup (Phase 02 step 3), the broker `NetworkPoller` cutover, an x86 NET_RX producer and physical Intel stay excluded. The slice-e file-bounded exception ends with that record.
