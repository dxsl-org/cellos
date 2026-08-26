# Phase 01 — Resume-Side pending_msgs Drain-on-Wake (Infrastructure Only)

## Context Links

- Plan: [plan.md](plan.md) · Research: [scout-report.md](scout-report.md), [research/red-team-findings.md](research/red-team-findings.md), [research/haily-researcher-01-recv-send-state-machine.md](research/haily-researcher-01-recv-send-state-machine.md)
- `kernel/src/task/syscall.rs:1290-1373` — `Recv` syscall handler (drain-before-park exists here; drain-after-wake does not, and is what this phase adds)
- `kernel/src/task/syscall.rs:1537-1597` — `RecvTimeout` handler (same gap)
- `kernel/src/task/syscall.rs:1610-1636` — `TryRecv` handler (never parks, not in scope for this phase's new drain-on-wake code, but must keep working unchanged)
- `kernel/src/task/syscall.rs:1470-1525` — `RecvScatter` (uses a kernel-heap temp buffer as `buf_ptr` per red-team finding — investigate before assuming it shares this phase's new path cleanly)
- `kernel/src/task/tcb.rs` — `pending_msgs`, `PendingMsg` struct, depth constants
- `kernel/src/task.rs:1529` — `ipc_reply`; verified independent of `ipc_send` and does not write into a `Recv` buffer

## Overview

- **Priority:** P1 (blocks Phase 02) · **Status:** completed · **Risk:** low
- Add the missing resume-side step: when a `Recv`/`RecvTimeout` syscall's `yield_cpu()` returns
  (the task has been woken), drain `pending_msgs` and, if a message is present, copy it into the
  caller's `buf_ptr`/`buf_len` and return the sender's tid — exactly the behavior direct-copy
  delivery provides today, just performed by the resumed receiver instead of the sender. **No
  producer changes in this phase** — nothing pushes to `pending_msgs` for a `Recv`-parked target
  yet, so this phase is testable only via a synthetic/targeted test that places a message in
  `pending_msgs` for an already-parked task and confirms the resume path picks it up correctly.
  Real end-to-end behavior change lands in Phase 02.

## Key Insights

1. This is the exact gap the red-team review found fatal in the original design: "the
   `pending_msgs` drain runs only before `ipc_recv`; after `yield_cpu()` the handler reads
   `t.current_caller` and returns it with no drain and no copy." That finding is still true and
   is what this phase fixes — it just no longer needs to interact with any completion queue.
2. The existing drain-before-park logic (`syscall.rs:1334-1369` etc.) is the template: same
   mailbox, same `PendingMsg` shape (`sender_tid`, owned `Vec<u8>`, `enqueued_tick`), same
   copy-into-`buf_ptr` semantics. The new drain-on-wake code should read as a near-duplicate of
   the existing drain-before-park code, not a new abstraction — mirror it directly (YAGNI: don't
   build a shared helper unless the duplication is large enough to be worth it; two ~10-line
   blocks are not).
3. Distinguish a real delivery from a timeout/spurious wake: after `yield_cpu()` returns, check
   `pending_msgs` first — if non-empty, drain and deliver (this phase's new behavior); if empty,
   fall through to today's existing logic (checking `regs[10]`/timeout state, returning
   accordingly). Order matters: check the mailbox before concluding "this was a timeout."
4. **`RecvScatter` is verified as a separate pre-existing bug.** It calls `ipc_recv` once, but on
   `Ok(0)` it does not call `yield_cpu`; it immediately scatters the temporary kernel buffer and
   returns while `TaskState::Recv` retains that temporary buffer pointer. This plan must remove the
   foreign-context producer writes that make the stale pointer dangerous, but must not pretend the
   syscall becomes functionally correct. Phase 04 records the separate blocking/lifecycle defect.
5. **`ipc_reply` is independent and safe for this scope.** It wakes `current_caller` and stores a
   reply value; it neither calls `ipc_send` nor writes into a foreign `Recv` buffer. Phase 02 needs
   no `ipc_reply` producer change, while Phase 03 must retain a request/reply regression check.

## Requirements

**Functional**
- `Recv` syscall handler: after `yield_cpu()` returns, drain `pending_msgs`; if a message is
  present, copy into `buf_ptr`/`buf_len` (truncating to `min(msg.len(), buf_len)` matching today's
  direct-copy truncation behavior), return sender's tid; else fall through to existing
  timeout/spurious-wake handling unchanged.
- `RecvTimeout` handler: same addition, still respecting the existing deadline-patch logic
  (`syscall.rs:1573-1583`) for the case where nothing arrives before the deadline.
- `TryRecv` unaffected (never parks; no wake to drain-on).
- `RecvScatter` receives no drain-on-wake addition because it has no wake/resume cycle today. Its
  pre-existing blocking/lifetime defect is documented in Phase 04; Phase 02 must still ensure no
  producer writes through the stale `TaskState::Recv.buf_ptr`.

**Non-functional**
- No change to any existing test's behavior — this phase adds unreachable-in-practice code
  (nothing pushes to a parked `Recv` target's mailbox yet) until Phase 02 lands, verified by a
  dedicated new test rather than by absence of regressions alone.

## Architecture

```
// Recv syscall handler, after yield_cpu() returns (new code, mirrors existing drain-before-park):

if let Some(msg) = drain_pending_msg(&mut task) {          // same PendingMsg shape as existing drain
    let copy_len = core::cmp::min(msg.data.len(), buf_len);
    unsafe { core::ptr::copy_nonoverlapping(msg.data.as_ptr(), buf_ptr as *mut u8, copy_len); }
    return Ok(msg.sender_tid);
}
// else: fall through to existing post-yield logic (timeout check, current_caller read, etc.)
```

`drain_pending_msg` is the same function (or a direct copy of the same few lines) already used by
the drain-before-park step — do not invent a second implementation of mailbox draining.

## Related Code Files

**Modify**
- `kernel/src/task/syscall.rs:1290-1373` — `Recv` handler
- `kernel/src/task/syscall.rs:1537-1597` — `RecvTimeout` handler
- `kernel/src/task/syscall.rs:1470-1525` — `RecvScatter`, pending investigation outcome

**Create**
- None — reuse the existing drain helper/logic; do not add a new module for this

## Implementation Steps

1. Record the verified `ipc_reply` finding: it is independent of `ipc_send`, performs no foreign
   buffer copy, and needs regression coverage rather than a producer edit.
2. Record the verified `RecvScatter` finding: it does not yield after parking and is deferred as a
   separate syscall-lifecycle fix; do not add drain-on-wake code to it in this phase.
3. Add the drain-on-wake block to the `Recv` handler, immediately after `yield_cpu()` returns and
   before existing timeout/current_caller logic.
4. Add the equivalent block to `RecvTimeout`, preserving the existing deadline-patch behavior for
   the no-message case.
5. Leave `RecvScatter` behavior unchanged and feed the verified defect into Phase 04's guardrail.
6. Write a targeted unit test: manually place a `PendingMsg` in a parked `Recv` task's mailbox
   (bypassing the normal producer path, since none exists yet), trigger its wake via the existing
   scheduler wake mechanism, and confirm the syscall returns the correct sender tid and buffer
   contents.

## Todo List

- [x] Confirm `ipc_reply` is independent of `ipc_send` and has no foreign `Recv` buffer write
- [x] Confirm `RecvScatter` does not yield and retains a temporary-buffer pointer when it parks
- [x] Add drain-on-wake to `Recv` handler
- [x] Add drain-on-wake to `RecvTimeout` handler
- [x] Keep `RecvScatter` out of the wake-drain change and pass its verified defect to Phase 04
- [x] Write synthetic drain-on-wake unit test
- [x] Confirm zero regression in existing Recv/RecvTimeout/TryRecv tests

## Evidence

- `kernel/src/task/syscall.rs` selects death-owned wakes before matching queued messages and drains
  the selected owned payload only after the receiver resumes.
- `ipc_pending_delivery_selftest_passes` passed in QEMU after the final release build.

## Deviation Log

- The focused boot self-test was added in `kernel/src/task/ipc_pending_selftest.rs` rather than an
  existing unit-test module because the no_std scheduler path requires initialized runtime state.

## Success Criteria

- New synthetic test passes: a message manually placed in a parked `Recv` task's mailbox is
  correctly delivered on wake.
- All existing IPC tests pass unchanged (this phase adds no live producer yet).
- `RecvScatter` remains behaviorally unchanged, no producer can write through its retained stale
  buffer pointer after Phase 02, and its separate syscall-lifecycle defect is recorded in Phase 04.

## Risk Assessment

- **Low**, since no producer changes yet — the only way this phase's new code executes today is
  via the synthetic test. The real risk surface opens in Phase 02.
- **RecvScatter pre-existing defect** — do not expand this fix into a syscall redesign. Document
  the missing yield and stale temporary-buffer lifecycle in Phase 04, and verify Phase 02 removes
  the foreign write that currently turns that defect into a memory-safety hazard.

## Security Considerations

- None new — this phase adds dead-until-Phase-02 code paths using an existing, already-audited
  mailbox mechanism.

## Next Steps

- Phase 02 makes this phase's new code path live by changing the three delivery sites to actually
  push into `pending_msgs` for a `Recv`-parked target.

## Assumptions

- **Observed:** `ipc_reply` is independent of `ipc_send` and does not copy into `Recv.buf_ptr`.
- **Observed:** `RecvScatter` calls `ipc_recv` without a subsequent `yield_cpu`; fixing that
  syscall's blocking/lifecycle behavior is explicitly outside this plan.
- **Claim:** The existing drain-before-park logic can be reused verbatim (same buffer-copy
  truncation semantics) for the drain-on-wake case. **Confidence:** high — both are copying the
  same `PendingMsg` shape into the same kind of destination buffer.
