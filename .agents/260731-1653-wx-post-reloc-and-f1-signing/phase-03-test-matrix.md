# Phase 03 — Full Test Matrix

## Context Links

- Plan: [plan.md](plan.md) · Depends on [Phase 02](phase-02-route-delivery-pending-msgs.md)
- Research: [research/red-team-findings.md](research/red-team-findings.md) for the specific adversarial cases this matrix must cover
- `tests/integration/` — existing integration test harness

## Overview

- **Priority:** P1 · **Status:** completed · **Risk:** medium
- Prove the delivery-mechanism change correct under both the happy path and the specific
  adversarial cases the red-team review surfaced against the (now-abandoned) queue-based design —
  several of which still apply to the pending_msgs-based fix. Can run in parallel with Phase 04.

## Requirements

**Test matrix (must all pass)**
1. Normal `Recv`/`ipc_send` round trip — message content and sender identity correct.
2. Non-blocking send (`ipc_post_nonblock`) delivers correctly into an idle `Recv`-parked target.
3. Blocking `ipc_send` with no waiting receiver — sender correctly parks in `Sending`, unaffected
   by this change (regression check only).
4. `ipc_reply`-based request/reply round trip (e.g. a VFS call) — the most common real-world Recv
   teardown, must be unaffected by this change.
5. Shell/keyboard input integration test — guest boots, host sends keystrokes over the console
   path, shell cell receives them via `ipc_post_nonblock` → `pending_msgs` → Phase 01's
   drain-on-wake.
6. Input-event burst test at/near `INPUT_EVENT_QUEUE_DEPTH=512` — no truncation, no regression
   from the per-function depth-constant resolution in Phase 02.
7. `RecvScatter` safety regression — model its actual pre-existing park-without-yield state and
   prove delivery routes into `pending_msgs` rather than writing through the retained temporary
   `buf_ptr`. Functional repair of `RecvScatter` itself remains out of scope.
8. `TryRecv`/`RecvTimeout`/`SendGather` — existing behavior unchanged (regression check).
9. Deliberately exhaust a target's `pending_msgs` mailbox (or simulate cell-quota exhaustion) while
   a sender attempts delivery to a `Recv`-parked target — confirm a graceful error, not a hang or
   panic.
10. Task exit while a peer is parked in `Sending` waiting on the exiting task — existing wake path,
    unaffected (regression check only, confirms this change didn't touch `Sending`).

## Architecture

No new production code in this phase — test-only. New tests live alongside existing IPC tests
(unit level) and in `tests/integration/tests/` (integration level).

## Related Code Files

**Modify**
- Existing IPC unit test file(s) — add cases 4, 7, 9 (no existing coverage, since these are the
  cases the red-team specifically flagged as unverified)

**Create**
- One new integration test for case 5 (shell keyboard input) if no equivalent already exists —
  check `tests/integration/tests/` first.
- One new test for case 6 (input burst at depth) if no equivalent already exists.

## Implementation Steps

1. Inventory existing tests covering cases 1-3, 8, 10 — confirm they exist and still pass
   unmodified after Phase 02 (regression, not new-test-writing).
2. Write a new test for case 4 (`ipc_reply` round trip) if none already exercises this path
   end-to-end with the new delivery mechanism.
3. Write the `RecvScatter` safety regression (case 7) around the verified stale-buffer state; do
   not encode the broken syscall behavior as a desired functional contract.
4. Write the mailbox-exhaustion/quota test (case 9).
5. Check `tests/integration/tests/` for an existing console/keyboard-input test to extend for
   case 5; write one only if none exists.
6. Check for an existing input-burst test to extend for case 6; write one only if none exists.
7. Run the full matrix; fix any regressions found by returning to Phase 02 (do not patch symptoms
   in test code).

## Todo List

- [x] Confirm cases 1-3, 8, 10 pass unmodified
- [x] Confirm existing VFS coverage exercises the `ipc_reply` round trip (case 4)
- [x] Model the `RecvScatter` stale retained-buffer state in the producer self-test (case 7)
- [x] Write mailbox-exhaustion/quota tests (case 9)
- [x] Extend shell keyboard input integration coverage (case 5)
- [x] Add near-depth input-burst coverage (case 6)
- [x] Full focused matrix green

## Evidence

- Kernel checks passed for RISC-V, AArch64, and x86_64; the integration test target compiled and
  the release RISC-V kernel built.
- Focused QEMU tests passed: IPC self-test, near-depth UART burst, long-line backspace, FAT16 VFS
  write/read, and keyboard E2E.

## Deviation Log

- The existing FAT16 VFS integration flow supplies the real `ipc_reply` round-trip proof, so no
  duplicate request/reply test was added.
- `RecvScatter` safety is modeled by retaining an invalid `TaskState::Recv` pointer and exercising
  every producer; its separate missing-yield functional defect remains deferred.

## Success Criteria

- All 10 cases pass, with cases 4, 7, 9 being genuinely new coverage.
- No hang or panic under deliberate mailbox/quota exhaustion.

## Risk Assessment

- **Integration tests are slower/flakier than unit tests** — prefer extending an existing analog
  test over inventing a new QEMU boot scenario, per this repo's established pattern of reuse in
  `tests/integration/`.

## Security Considerations

- None beyond what Phase 02 already addresses — this phase verifies, does not introduce new
  surface.

## Next Steps

- On green matrix, this fix is ready to ship per this plan's scope. Migrating Recv's wait
  mechanism onto the completion queue remains a separate, future effort (see plan.md).

## Assumptions

- **Claim:** An existing console/keyboard-input integration test or a close analog already exists
  in `tests/integration/tests/` that can be extended rather than written from scratch.
  **Confidence:** low. **How to verify:** search `tests/integration/tests/` before starting step 5.
- **Claim:** An existing input-burst test already exists near the 512-depth boundary.
  **Confidence:** low. **How to verify:** search for tests referencing
  `INPUT_EVENT_QUEUE_DEPTH` before starting step 6.
