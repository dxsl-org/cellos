# 2026-08-06 — Recv and peer-death guardrails

## What happened
Phase 02 froze shell input, peer-death, RecvScatter, and VFS grant-copy boundaries.
Commit `d4cc2aa3` contains the implementation, runtime guards, and living-doc updates.

## Decisions
- Use a dedicated boot selftest instead of extending the already-full IPC pending test, keeping stale-pointer and ready-queue assertions local.
- Return dead-peer errors through both `reply_value` and trap `a0`, because the resumed Send handler reads `reply_value` before returning to userspace.
- Clear `reply_value` before every new blocked send so one dead-peer error cannot poison a later successful send.
- Use an app-bench heartbeat peer to prove a real blocked sender resumes with an error, plus a separate ForceExit notification drain.
- Keep RecvScatter mailbox-only; do not repair or migrate it to the completion queue in this phase.

## Lessons
- A Ready-state assertion alone can false-pass if the scheduler forgets to requeue the task.
- A trap-register write alone does not prove caller-visible syscall output when the handler resumes inside the kernel.
- Existing documentation reused “Phase 02” for another closure; reactor-stack guardrails need a distinct label.

## Next steps
- Enter Phase 03 Law 1 gate; no Phase 04 or Phase 05 edits without two explicit confirmations.
- Keep generic reactor, async VFS/DMA, parked executor, RecvScatter repair, and stack resizing deferred.
