# Slice VL — VFS per-launch request-log retirement

Status: **closed 2026-11-04 by the owner; the Intel WIP lane is free again.** Implemented and verified the same day. Method: `hc-debug` (root cause before the fix).

## Root cause (CONFIRMED)

VFS keeps one `RequestLog` per `(creator, Operation)` (`cells/services/vfs/src/prepared_cwd.rs`). The log fences the creator's request IDs and VFS's own kernel-mutation IDs in that Operation. `reclaim` dropped a finished reservation but never its log. `reconcile_matching` removed logs only when their creator had died. Every foreground launch uses a new child Operation, so a live shell gained one log per launch until `MAX_CREATOR_LOGS = 64`, after which `ReserveCwd` answered `QuotaExceeded`. The probe from slice TL logged `total=64` on each refused launch.

## Retirement rule

A log can be dropped once the kernel has removed its Operation. No request in that scope can be admitted again: ReserveCwd resolves the prepared record (removed with the Operation), Commit/Resolve need a reservation, Acknowledge/Query need the log, and Operation references are never reused. VFS cannot see the removal directly, so it probes for it:
- **Candidates.** Logs with no reservation naming the scope, no retained outcome, no unresolved kernel mutation, and every kernel mutation acknowledged. At least one kernel mutation is required, since that creates VFS's ledger in the Operation.
- **Probe.** For each candidate, VFS re-acknowledges the ID it already acknowledged (`AcknowledgeRequests`, not ledgered). A live ledger answers Ok and nothing changes. The kernel drops the ledger only in `remove_operation`, after which the answer is `UnknownRequest`. Any other answer keeps the log.
- **When it runs.** On every reconcile (the creator's next filesystem request or the idle cadence), and once more when a creator is at the cap.

## Symptom (from [slice TL](slice-thread-exit-leak.md), Deviation Log item 2)

- One shell can start only 64 programs per boot. Launch 65 fails with `shell: foreground preparation failed: QuotaExceeded`, whether or not the program uses threads.
- A temporary probe confirmed the source: `cells/services/vfs/src/prepared_cwd.rs` logged `vfs request logs full: total=64`. Each foreground launch reserves the child's cwd (`VfsOp::ReserveCwd`) under a new child operation. That creates a new `(creator, operation)` request log in VFS. The log is removed only when the creator itself has died (`reconcile_matching`, the `!has_met(creator)` branch). The cap is `MAX_CREATOR_LOGS = 64`.

## Scope

- The VFS prepared-cwd request-log lifetime (`cells/services/vfs/src/prepared_cwd.rs` and its tests), plus SDK/terminal callers only if the cause requires them. Exact file set recorded in the Deviation Log before editing.
- Not touched: `libs/api/**`, `libs/types/**` (a Law-1 change would need its own CP), and the kernel `cell_session` protocol.

## Acceptance

1. Root cause and the retirement rule documented: when a per-operation log may be dropped without breaking replay/idempotency for that operation.
2. A launch loop from one shell is no longer capped at 64. `MLIBC_POSIX_RUNS=100` passes in one boot on TCG, plus a KVM run.
3. Regression: VFS host tests (with a test for the retirement rule), x86 domain lane, kernel host tests.

## Results

| Check | Result |
|---|---|
| Witness TCG, `MLIBC_POSIX_RUNS=100` in one boot (101 launches from one shell) | PASS, 100/100 `7/7 pass`, 100 of 100 domains released, no refused launch |
| Witness KVM (`qemu64,+pdpe1gb`), 100 runs | PASS, 100 of 100 domains released |
| VFS host tests (`service-vfs`) | 139/139, including the new selection test `only_a_settled_unreferenced_log_is_offered_for_operation_removal` |
| x86 domain lane | PASS |
| Kernel host tests | not re-run: no kernel change in this slice (421/421 at slice TL) |

## Deviation Log

1. **File set.** `cells/services/vfs/src/prepared_cwd.rs`: module declaration, the cap fallback in `log`, the dead-creator branch reusing `drop_log`, the call in `reconcile_matching`, and the test. New `cells/services/vfs/src/prepared_cwd/closed_logs.rs`. No SDK, terminal, kernel or `libs/api` change.
2. **Relied-on kernel behaviour, not a frozen contract.** The probe depends on existing code in `kernel/src/task/cell_session.rs`: `AcknowledgeRequests` is not ledgered, `through <= acknowledged` answers Ok, and request ledgers are removed only in `remove_operation`. If a future kernel dropped a ledger while its Operation lived, VFS could drop a live scope's log. A retransmitted first request in that scope could then be admitted again. Any kernel change to ledger lifetime must keep this invariant or tell VFS.
