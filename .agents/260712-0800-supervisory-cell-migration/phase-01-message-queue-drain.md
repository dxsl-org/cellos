# Phase 01 - Atomic Message-Queue Drain + Cutover Barrier

## Context Links
- Plan: [plan.md](plan.md)
- Current `ResumeCell=414` ABI: `a0 = target_tid` only, bit 49: `libs/api/src/abi/syscall.rs:370`, `libs/api/src/abi/syscall.rs:701`
- Current kernel decode ignores `a1..a3`: `kernel/src/task/syscall.rs:4761`
- Current resume only unfreezes: `kernel/src/task/syscall.rs:3635`
- `PauseService=422` hides lookup and waits pre-pause ingress drain: `kernel/src/task/syscall.rs:3614`, `kernel/src/task.rs:102`
- Current cached-TID rejection while paused: `kernel/src/task.rs:87` (must become phase-sensitive)
- Frozen send queues to old `pending_msgs`: `kernel/src/task.rs:1393`, `kernel/src/task/tcb.rs:328`
- Existing kernel drain copy preserves `sender_tid`: `kernel/src/cell/hotswap.rs:548`, `kernel/src/cell/hotswap.rs:565`
- Supervisor currently registers `new_tid` before old kill: `cells/services/supervisor/src/hotswap.rs:154`

## Overview
- **Priority**: P1. **Status**: complete. **Risk**: HIGH.
- Replace the prior drain-only proposal with a kernel-atomic commit barrier. The bug is a late
  cached-TID race: after the supervisor registers `new_tid` and before it kills `old_tid`, the old
  provider is no longer "paused", so a sender that cached `old_tid` can enqueue to the still-Frozen
  old task after the drain and lose that message.

## Key Insights
- `PauseService` is only the soft quiesce barrier. It proves accepted ingress before pause has
  drained while the old provider still runs, but it is not the cutover point.
- The cutover point must be one kernel linearization point: close old ingress permanently, take the
  old FIFO, publish `service_id -> new_tid`, and wake the replacement if its receive mask matches.
- Do not replay through `ipc_send`. The current kernel drain correctly copies into the replacement
  mailbox and preserves `PendingMsg.sender_tid`; replaying through `ipc_send` can park an already
  resumed original sender.

## Requirements
- Functional: cached-TID admission has exactly three phases:
  1. `Paused + runnable`: reject cached non-supervisor ingress while pre-pause work drains.
  2. `Paused + Frozen + ingress_closed=false`: accept cached non-supervisor sends into the bounded old
     FIFO; this is the only window that proves the barrier drains real client traffic.
  3. `ingress_closed=true`: reject non-supervisor sends to old forever, including before `KillCell`.
- Functional: every message accepted into old `pending_msgs` during phase 2 is delivered to the
  replacement in FIFO order with original `sender_tid`.
- Functional: every non-supervisor send to `old_tid` after the barrier returns backpressure/peer-gone;
  it must not enqueue to old, even before `KillCell(old_tid)` runs.
- Functional: `LookupService(service_id)` returns `new_tid` only after old ingress is closed and the
  old FIFO has been taken.
- Non-functional: preserve syscall number `414` and allowlist bit `49`; no fourth lifecycle primitive.

## Architecture / Data Flow
```
soft quiesce:
  supervisor -> PauseService(service_id, old_tid)
  kernel     -> registry Paused(old_tid); reject cached non-supervisor sends; wait pre-pause drain

frozen admission window:
  supervisor -> FreezeCell(old_tid)
  kernel     -> old TaskState::Frozen, ingress_closed=false
             -> cached non-supervisor sends to old are accepted into old pending_msgs

commit barrier:
  supervisor -> ResumeCell(a0=new_tid, a1=old_tid, a2=service_id, a3=0)
  kernel     -> lock SCHEDULER
             -> validate old Frozen, new live/ready, registry Paused(old_tid)
             -> preflight replacement mailbox capacity
             -> copy old FIFO to new FIFO preserving sender_tid
             -> mark old ingress permanently closed
             -> publish service_id -> new_tid
             -> wake new if copied message mask matches
             -> unlock SCHEDULER

post-commit:
  supervisor -> KillCell(old_tid, 0xAAAA_AAAA)
```

## Exact `ResumeCell=414` ABI
- **Plain abort/resume:** `a0 = target_tid`, `a1 = 0`, `a2 = 0`, `a3 = 0`.
  Resume the frozen target exactly as today. This is the only rollback path before commit.
- **Atomic cutover:** `a0 = target_tid/new_tid`, `a1 = source_tid/old_tid`, `a2 = service_id`,
  `a3 = 0 reserved`.
  `source_tid` is the frozen provider whose FIFO is drained and whose ingress is closed.
  `target_tid` is the replacement that receives the FIFO and becomes the published provider.
- Reject `a3 != 0`, `source_tid == 0`, `target_tid == 0`, `source_tid == target_tid`, missing
  SupervisorCap, missing/frozen-unready target, source not `TaskState::Frozen`, or registry not
  `Paused(source_tid)` for `service_id`.
- Return `0` on success. Return `TryAgain` for capacity/backpressure preflight failure; return
  `InvalidInput`, `FileNotFound`, or `PermissionDenied` for invalid identities/authority.

## Linearization Point
- Implement one helper called only by the `ResumeCell` syscall arm, e.g.
  `commit_hotswap_barrier_locked(source_tid, target_tid, service_id)`.
- Lock order is **SCHEDULER -> service_registry**. This matches cached-TID send admission
  (`kernel/src/task.rs:87`) and avoids introducing registry -> scheduler nesting inside the barrier.
- Admission predicate before the barrier:
  - If `service_registry::is_paused_tid(target_id)` and target is not `TaskState::Frozen`, reject
    non-supervisor sends. This preserves the current pre-pause drain behavior.
  - If target is `TaskState::Frozen` and `hotswap_ingress_closed == false`, allow the existing Frozen
    queue branch to copy non-supervisor messages into old `pending_msgs` even though the registry is
    still `Paused(old_tid)`.
  - If `hotswap_ingress_closed == true`, reject non-supervisor sends before the Frozen queue branch.
- While holding `SCHEDULER`, the barrier helper must:
  1. Validate source and target tasks.
  2. Validate the service registry still records `Paused(source_tid)`.
  3. Preflight that `target.pending_msgs.len() + source.pending_msgs.len() <= HOTSWAP_MSG_QUEUE_DEPTH`.
  4. Copy source messages into target with `queue_pending_msg(target, msg.sender_tid, msg.data, ...)`.
  5. If any copy fails, undo copied target messages and leave source FIFO, source ingress, and registry unchanged.
  6. On success, atomically set source ingress closed, take/empty the source FIFO, publish Active(target_tid),
     and enqueue/wake target according to the original sender mask.
- The "source ingress closed" state must live under SCHEDULER, not only in the service registry. Minimal
  acceptable implementation: add a TCB field such as `hotswap_ingress_closed: bool`, set/reset it as:
  - New task constructor: `false`.
  - `FreezeCell`: reset to `false` before exposing the Frozen FIFO window.
  - Plain abort `ResumeCell(a1=0)`: reset to `false` before requeueing old.
  - Barrier success: set old to `true` at the same linearization point as FIFO transfer and service publish.
  - `KillCell`/exit cleanup: task removal naturally drops the state.

## Related Code Files
- Modify `libs/api/src/abi/syscall.rs`: update `ResumeCell` docs only; keep numeric value `414` and bit 49.
- Modify `libs/ostd/src/syscall.rs`: keep `sys_resume_cell(target_tid)` for aborts; add
  `sys_commit_hotswap(source_tid, target_tid, service_id)` or equivalent wrapper that calls 414 with
  `a0=target_tid,a1=source_tid,a2=service_id,a3=0`.
- Modify `kernel/src/task/tcb.rs`: add scheduler-owned old-ingress-closed state, or an equivalent
  TaskState-level representation.
- Modify `kernel/src/task.rs`: make `paused_target_rejects` phase-sensitive and reject ingress-closed
  old tasks before the Frozen queue branch.
- Modify `kernel/src/cell/service_registry.rs`: add an internal compare-and-commit helper that changes
  `Paused(old_tid)` to `Active(new_tid)` only under the barrier.
- Modify `kernel/src/task/syscall.rs`: decode `ResumeCell { target_tid, source_tid, service_id }` and
  route `source_tid == 0` to the old plain resume; route `source_tid != 0` to the barrier.
- Modify `kernel/src/cell/hotswap.rs`: extract/reuse the direct FIFO-copy logic; do not use `ipc_send`.
- Modify `cells/services/supervisor/src/hotswap.rs`: replace "register new -> kill old" with
  `sys_commit_hotswap(old_tid, new_tid, service_id) -> kill old`.
- Modify `tests/integration/tests/hotswap-smoke.rs` and `cells/tests/bench/src/scenarios/hotswap_supervisor.rs`:
  add the post-cutover cached-TID witness below.

## Implementation Steps
1. Stop for **Law 1 two-confirmation gate** before editing `libs/api/`: confirm twice that extending
   `ResumeCell=414` register semantics is approved while preserving number `414` and bit `49`.
2. Add the scheduler-owned ingress-closed state; reset it on task construction, freeze, and plain resume.
3. Change cached-TID admission:
   - Paused+runnable rejects cached non-supervisor sends until `inbound_ipc_drained(old_tid)` passes.
   - Paused+Frozen+ingress_closed=false accepts cached non-supervisor sends into old `pending_msgs`.
   - ingress_closed=true rejects cached non-supervisor sends forever.
4. Add service-registry compare-and-commit support for `Paused(old_tid) -> Active(new_tid)`.
5. Extend kernel `Syscall::ResumeCell` decode to carry `source_tid` and `service_id`.
6. Implement the barrier helper with the lock order and rollback rules above.
7. Keep the abort path as plain `sys_resume_cell(old_tid)` before the barrier; after the barrier, do not
   rollback to old because ingress is closed and FIFO ownership has moved.
8. Update supervisor commit ordering: call the barrier, then kill old, then clear stash. If barrier fails,
   kill new, plain-resume old, re-register old, and clear stash.
9. Add deterministic QEMU witness:
   - cached sender stores `old_tid` before hotswap.
   - after `LookupService(HOTSWAP_DEMO)` returns `None`, cached sender retries against old until two sends
     to frozen old return `Ok`: first `inc`, then `get`.
   - barrier drains both FIFO entries to the replacement.
   - after `LookupService(HOTSWAP_DEMO)` publishes `new_tid`, cached sender sends to old again and must get `Err`.
   - cached sender then receives the two replies from new in FIFO order: `inc -> ok`, then `get -> v2:6`.
   - QEMU parent asserts final runtime line is `PASS (v1 counter=5 -> v2 counter=6)` and that the old-TID
     post-publish send was rejected. No production sleep/yield hook is required.

## Todo List
- [x] Law 1 confirmation #1 for `ResumeCell=414` register extension
- [x] Law 1 confirmation #2 for `ResumeCell=414` register extension
- [x] Scheduler-owned old-ingress-closed state
- [x] Phase-sensitive cached-TID admission
- [x] Service-registry compare-and-commit helper
- [x] Kernel `ResumeCell` decode + atomic barrier helper
- [x] ostd commit wrapper; old plain resume preserved
- [x] Supervisor commit order changed to barrier -> kill
- [x] QEMU cached-TID post-cutover ordering witness

## Success Criteria
- `ResumeCell` remains syscall number `414` and allowlist bit `49`.
- Plain `sys_resume_cell(old_tid)` still resumes an aborted frozen task.
- `ResumeCell(new_tid, old_tid, service_id, 0)` either commits all of: old ingress closed, FIFO moved,
  service active at `new_tid`; or commits none of them.
- QEMU witness proves two cached sends enter old FIFO while old is Frozen, the barrier delivers both to
  new in FIFO order, and a cached send to old after service publication is rejected.
- Final QEMU parent assertion is `PASS (v1 counter=5 -> v2 counter=6)`.
- No message is dropped on replacement mailbox overflow; barrier returns `TryAgain` before commit.

## Risk Assessment
- **High - partial transfer on allocation/capacity failure.** Mitigation: preflight capacity, copy with an
  undo list, and do not publish `new_tid` or close old ingress until all copies succeed.
- **High - overbroad paused rejection prevents the FIFO proof.** Current `paused_target_rejects` rejects
  cached non-supervisor sends for any paused tid. Mitigation: make the predicate phase-sensitive so
  Paused+runnable still rejects, Paused+Frozen+ingress_open accepts into old FIFO, and ingress_closed rejects.
- **High - lock-order deadlock.** Mitigation: barrier uses SCHEDULER -> service_registry only; no code path
  may call into scheduler while holding the registry lock during commit.
- **High - rollback ambiguity after commit.** Mitigation: define commit as irreversible for old provider
  identity. Before barrier, rollback resets ingress_closed=false, resumes old, and re-registers old. After
  barrier, rollback kills old/new and leaves service absent or escalates; it must not re-open old silently.
- **Medium - supervisor callers confused by changed wrapper.** Mitigation: preserve one-argument
  `sys_resume_cell(target_tid)` and add a separate named commit wrapper.

## Security Considerations
- Preserve original sender identity: target mailbox entries must use `msg.sender_tid`, not supervisor tid.
- The barrier must validate `SupervisorCap` and the paused registry compare before publishing `new_tid`;
  otherwise a compromised supervisor request could redirect an unrelated service.
- Allow old `pending_msgs` only during Paused+Frozen+ingress_open; do not let it accept post-commit messages.

## Assumptions and Evidence
- **Observed:** `PauseService` currently pauses lookup and checks pre-pause drain (`kernel/src/task/syscall.rs:3614`,
  `kernel/src/task.rs:102`).
- **Observed:** current frozen sends enqueue to old `pending_msgs` (`kernel/src/task.rs:1393`) and queue overflow
  maps to backpressure/TryAgain (`kernel/src/task.rs:1398`).
- **Observed:** current paused-TID rejection is broad (`kernel/src/task.rs:87`) and therefore must be narrowed
  by target state and ingress-closed state to create a real Frozen FIFO admission window.
- **Observed:** current supervisor publishes `new_tid` before killing `old_tid` (`cells/services/supervisor/src/hotswap.rs:154`).
- **Observed:** `reports/harness/verification.json` records PASS for fmt/diff checks, API 74+2, RV64 + host integration compilation, disk generation, and `hotswap-smoke` 13/13.
- **Observed:** `reports/harness/review-decision.json` is PASS and `reports/harness/adversarial-validation.json` is PASS.
- **Assumption:** adding one scheduler-owned TCB flag is acceptable because the state is per-task lifetime and
  avoids making service-registry state the sole ingress gate. Verify during implementation by checking all
  `Task` constructors initialize and reset it.

## Next Steps
Phase 02 is now unblocked. It must still treat the QEMU witness as the cutover gate; a state-preserving
swap without the two accepted frozen-old FIFO sends, FIFO replies from new, final counter 6, and
post-publish old-TID rejection is not enough to close this phase.
