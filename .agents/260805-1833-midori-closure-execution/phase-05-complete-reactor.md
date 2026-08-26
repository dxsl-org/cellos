---
phase: 5
title: "Close Phase 07 Reactor Honestly"
status: completed
priority: P1
effort: 1d
dependencies: [3, 4]
tier: thinking
---

# Phase 05: Close Phase 07 Reactor Honestly

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Close Phase 07 as a verified partial reactor substrate, not as "reactor complete". The smallest honest closure is to prove the current NET_RX completion queue path and document the deferred IPC/generic-reactor work that would require new ABI and Law 1 confirmation.

Closure executed on 2026-08-06: no product code changes, only status/evidence updates. The recorded status is now "verified partial substrate" with the current NET_RX-only proof and the still-open generic reactor gaps.

## Requirements

- Functional: preserve existing shell `Recv`/`RecvTimeout` plus `pending_msgs` behavior; do not route it through `WaitCompletion`.
- Functional: verify `WaitCompletion` remains NET_RX-only and rejects every other mask.
- Functional: keep `libs/ostd/src/executor.rs` busy-yield status explicit; do not claim real parking while `dummy_raw_waker` remains.
- Functional: do not add peer-death CQ unless Law 1 confirmation #1 and #2 approve the new event bit plus target identity/generation ABI.
- Non-functional: no async VFS, DMA, generic `RecvScatter`, or service-borrowed grant expansion in this closure.

## Architecture

Observed data flow:

1. `cells/services/net/src/main.rs:172-184` calls `sys_wait_completion(NET_RX, timeout_ticks)` after polling IPC and pumping RX.
2. `libs/ostd/src/syscall.rs:1638-1655` writes a 24-byte `ViCompletion` buffer only when syscall 242 returns `1`.
3. `kernel/src/task/completion_wait.rs:67-87` validates the output buffer, reserves a per-cell CQ slot, registers one waiter, and arms NET_RX.
4. `kernel/src/task/waker.rs:89-105` can complete the armed slot or set `NET_RX_PENDING`; current non-test producers are absent (`grep` found only `kernel/src/task/net_rx_selftest.rs:57,90`).
5. `kernel/src/task/completion.rs:364-397` defers wake delivery through the scheduler after append, not from interrupt context.
6. `kernel/src/task/completion_wait.rs:128-155` drains the result or releases the reservation on timeout.

Deferred data flows:

- Generic IPC wait: `Recv`/`RecvTimeout` drain `pending_msgs` before blocking and resume-copy in caller context (`kernel/src/task/syscall.rs:1379-1535`, `1663-1701`). Replacing that with CQ would need a state-machine migration, not just a waker.
- Peer death: `exit_task` wakes `Sending`, `Waiting`, and `NotifyOnExit` watchers only (`kernel/src/task/scheduler.rs:512-568`). CQ peer-death needs a per-dependency registry keyed by target tid plus generation.
- VFS/grants: current unsafe proof is synchronous-only; `kernel/src/fs/fat.rs:467-485` and lease helpers at `kernel/src/task.rs:1531-1585` explicitly require separate pin/lifetime review before live async wiring.

## Assumptions

None — all closure claims above are OBSERVED in this checkout or explicitly PRIOR from the three research reports listed below.

## Related Files

- Modify: `.agents/260805-1833-midori-closure-execution/plan.md`
- Modify: `.agents/260805-1833-midori-closure-execution/phase-05-complete-reactor.md`
- Read-only source status: `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md`
- Evidence: `.agents/260731-1653-wx-post-reloc-and-f1-signing/research/haily-researcher-01-recv-send-state-machine.md`
- Evidence: `.agents/260731-1653-wx-post-reloc-and-f1-signing/research/haily-researcher-02-buffer-pinning-audit.md`
- Evidence: `.agents/260731-1653-wx-post-reloc-and-f1-signing/research/haily-researcher-03-completion-queue-precedent.md`

## Implementation Steps

1. Re-run grep before code: `signal_net_rx`, `WaitCompletion`, `RecvScatter`, `dummy_raw_waker`, `NotifyOnExit`, and `TaskState::Recv`.
2. Add no code unless a failing marker contradicts the verified facts; this phase is a closure/evidence phase by default.
3. Record the current QEMU evidence already attached to the landed Phase 07 reports: queue self-test PASS, NET_RX reservation PASS, IPC pending PASS, and shell prompt reached.
4. If peer-death CQ is still demanded in current scope, stop for Law 1 confirmation #1 and #2 before adding an event bit, target tid/generation ABI, or dependency registry.
5. Update original Phase07 status only to "partial/verified substrate"; do not mark a generic reactor complete.
6. Update Phase08 dependency wording: stack sizing may retain the current boot baseline, but watermark-driven sizing remains blocked on a real parked executor or equivalent post-shim measurement path.

## Success Criteria

- [x] Boot evidence is cited for `completion-queue self-test PASS (reserve, land, bound, defer)` from `kernel/src/main.rs:603`.
- [x] Boot evidence is cited for `net-rx-reservation self-test PASS (fill, remember, release)` from `kernel/src/main.rs:608`.
- [x] Boot evidence is cited for `ipc-pending self-test PASS (deferred delivery, bounds, quota)` from `kernel/src/main.rs:613`.
- [x] QEMU boot evidence is cited for `PASS: shell prompt reached`.
- [x] `grep -RIn "signal_net_rx" kernel libs cells tests` still shows no non-test producer beyond the implementation itself; no caller routes NIC IRQs to it.
- [x] `WaitCompletion` non-NET_RX rejection is cited code at `kernel/src/task/completion_wait.rs:75`.
- [x] `dummy_raw_waker` presence is recorded as "executor still busy-yields" (`libs/ostd/src/executor.rs:13`, `kernel/src/task/scheduler.rs:39`).
- [x] `RecvScatter` remains a separately deferred generic-blocking defect (`kernel/src/task/syscall.rs:1602-1635`).
- [x] Original Midori Phase07 is not marked complete; the status text says verified NET_RX-only substrate and lists deferred generic reactor, peer-death CQ, async VFS/DMA, and executor work.

## Security Considerations

Do not enable cancellable async access to caller/service memory. The current safe boundary is synchronous copy plus owned mailbox delivery; async VFS, DMA, leases, and generic blocking need pin/unpin or driver-ack semantics before they can be exposed.

## Risk Notes

| Risk | Likelihood x Impact | Mitigation | Rollback |
|------|---------------------|------------|----------|
| False "reactor complete" claim | High x High | Status text must say NET_RX-only substrate and list deferred work | Revert plan/status text only |
| Shell input regression | Medium x High | Keep `Recv`/`RecvTimeout`/mailbox paths out of this closure | Revert any Recv/CQ migration |
| Peer-death ABI creep | High x High | Law 1 2/2 checkpoint before event bit/API work | Drop ABI draft before code; cannot undo published ABI without migration |
| Grant UAF via async VFS/DMA | Medium x Critical | No async producer expansion in this phase | Disable producer; keep synchronous path |

## Backwards Compatibility

No ABI or behavior change is approved by this phase. Existing syscall 242 remains NET_RX-only and shares the WaitForEvent authority bit (`libs/api/src/abi/syscall.rs:655-668`); broader completion sources are new design work.

## Deviation Log

### 2026-08-06 — evidence-only closure, no product-code follow-on

- **Decision:** use the already-landed Phase 07 runtime reports as the QEMU evidence source for this closure, then re-run current grep/code checks in-tree. That keeps the closure same-facts/same-commit instead of inventing a new boot proof.
- **Verified:** `.agents/reports/phase-07-completion-queue-260731.md` records `PASS: shell prompt reached` plus `completion-queue self-test PASS (reserve, land, bound, defer)`.
- **Verified:** `.agents/reports/phase-07-net-rx-migration-260731.md` records `net-rx-reservation self-test PASS (fill, remember, release)` and proves the net cell's current wait path with `http-smoke`; `kernel/src/main.rs:613` still carries the `ipc-pending self-test PASS (deferred delivery, bounds, quota)` marker.
- **Verified:** current grep still shows no non-test caller that routes NIC IRQs into `signal_net_rx`; `cells/services/net/src/main.rs:184` waits on `sys_wait_completion(NET_RX, timeout_ticks)`, but `kernel/src/task/waker.rs:89` has no production call site outside self-tests and the waker implementation itself.
- **Verified:** `kernel/src/task/completion_wait.rs:75` still rejects every mask except `NET_RX`; `libs/ostd/src/executor.rs:13` and `kernel/src/task/scheduler.rs:39` still build a `dummy_raw_waker`, so the executor remains busy-yield rather than truly parked.
- **Verified:** `kernel/src/task/syscall.rs:1602-1635` still carries the pre-existing `RecvScatter` generic blocking gap; `cells/services/vfs/src/dispatch.rs:308` still relies on the synchronous `ipc_call` grant-lifetime argument, so async VFS grant producers remain deferred.
- **Decision:** peer-death completion events, target identity/generation tracking, and any generic IPC wait migration stay deferred behind Law 1 confirmation #1 and #2. This closure records the substrate honestly; it does not advance ABI design.
