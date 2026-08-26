# 2026-08-01 — IPC receiver-owned mailbox

## What happened
Closed the suspended `Recv` buffer-pinning hazard, resolved review blockers, completed the plan,
and committed only the IPC slice from a heavily shared worktree as `0763e8a5`.

## Decisions
- Producers enqueue owned data; only a resumed receiver writes its validated user buffer.
- IRQ-sized payloads stay inline; larger payloads carry receiver CellId ownership for quota refund.
- Death-owned wakes outrank later mailbox traffic; hot-swap copies payloads into replacement ownership.
- Completion-queue migration and `RecvScatter` lifecycle repair remain separately scoped.

## Lessons
- Zero-context partial patches are unsafe when omitted neighboring hunks shift line numbers; use
  contextual patches and compile an isolated staged snapshot before committing.
- Specialist delegation can fail independently of the workflow; retain artifact-backed local fallbacks.

## Next steps
- The concurrent capacity-observability task can run its A2/A3 runtime gates on the preserved tree.
- Revisit completion-queue IPC only with IPC-owned slots and task-level teardown bookkeeping.
