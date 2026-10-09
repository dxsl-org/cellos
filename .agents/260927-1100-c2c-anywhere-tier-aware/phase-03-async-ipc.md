---
phase: 3
title: "Reliable nonblocking request and completion lifecycle"
status: pending
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

- [ ] `submit` returns without parking when target is busy, or returns immediate `Busy` with no request delivered; later the exact accepted request produces one correlated completion.
- [ ] Full queue, duplicate/late replies, caller/target death, cancellation and deadline produce a terminal outcome with zero silent loss or double execution in the proven window.
- [ ] IPC-masked recv, VFS grant lifetime and TIMER/NET_RX completion behavior remain valid in QEMU; deterministic two-hart wake race test passes.
- [ ] New ABI is absent unless both Law-1 confirmations are recorded; no Cap'n Proto pipelining or wire swap is asserted by this phase.

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

## Deviation log

None.
