---
phase: 4
title: "Unified bounded operation lifecycle and multi-waiter completion"
status: complete
priority: P1
tier: thinking
dependencies: [2, 3]
---

# P04 — One operation contract

## Deliverable and requirements

AC-10..AC-12, aggregate-budget AC-06 and worker-death AC-09. Unify RPC/wait/event ownership semantics without weakening current exact-operation terminals. Exact completion/waitset ABI changes must pass P01 gates; existing opcodes are not silently reinterpreted.

## Related files / ownership

- `kernel/src/task/{async_ipc.rs,completion.rs,completion_wait.rs,ipc_wire.rs,tcb.rs,scheduler.rs}`, syscall dispatch paths.
- `libs/api/src/abi/{syscall.rs,completion.rs}`, `libs/ostd/src/{ipc.rs,service.rs,cluster_endpoint.rs,executor.rs,dispatch.rs}`.
- `cells/services/net-broker/src/local_runtime.rs` and deferred VFS/KMS/NET/NIC providers from P01 caller census.
- `cells/tests/bench/src/scenarios/local_service_lifecycle.rs`, `tests/integration/tests/local-service-lifecycle-x86.rs`, x86 lifecycle runner.

## Implementation steps

1. Define one internal Cell-charged reservation/accounting contract for accepted operation terminal storage; retain owner-thread cleanup and exact caller/provider incarnation. Don't retain borrowed user request/reply pointers.
2. Implement a bounded multi-waiter registration set keyed by live task and filter/operation identity, replacing the single waiter assumption. Non-consuming wait and atomic consuming take are distinct; register/recheck-before-park plus publish-before-wake avoids lost wake. IRQ completion appends without scheduler lock/allocator and defers wakes.
   Wait/take filters require the owner thread or explicit same-Cell consumption-handle handoff; handing off consumption does not move the operation's owner lifetime. Serialize owner-exit cleanup with consuming take so one releases and the other sees invalid/consumed, never a double free or stolen result.
3. Support wait over RPC and required NET_RX/TIMER/raw-event sources without polling loops or consuming someone else's terminal. No unlimited event accumulation; overflow/refusal policy per source is explicit and charged.
4. Keep per-thread current receive context and explicit retained tokens for nested/deferred replies. Any same-incarnation worker given the token can finish it; duplicate/late/foreign-generation reply fails. Cross-Cell reply delegation is not implicit and requires separately approved authority if a consumer needs it.
5. Enforce pre-dispatch cancellation/removal versus post-dispatch Indeterminate. Worker exit removes its queued operations and safely drops terminal reservations; dispatched effects remain uncertain. Root death cleans the whole shared arena only after readers/writers quiesce.
6. Make SDK sync calls thin waits over this operation path; migrate all clients/providers and executor paths affected by changed semantics. Remove stale mailbox-specific retry/correlation conventions, not the raw event protocol still in use.
7. Preserve reply/error buffer semantics (undersized take retains the terminal until deliberately consumed), terminal charging until drain, deadline monotonicity and no retarget to replacement. Handle nested calls under bounded capacity without retaining locks across waits.
8. Bind broker accounting to actual reply outcome instead of treating failed replies as delivered; preserve ADR-0009 NotSubmitted/Submitted distinction for future remote integration.

## Verification / success criteria

- [ ] Model deterministic reply/cancel/death/take and two-waiter races; one terminal, one consuming take, no foreign wake, no leaked reservation.
- [ ] Run x86 lifecycle suite and independent serial smoke: sync/async PeerGone, nested reply, duplicate quarantine, oversized reply cleanup, 64-slot saturation, pre/post-dispatch deadline and replacement rejection remain correct.
- [ ] New two-CPU multi-waiter runtime scenario waits on RPC + timer + event while another worker exits; each exact result reaches its eligible owner, no blocked thread remains after finite deadline.
- [ ] Finite competing producers complete with exact correlations, no Busy request delivered, bounded accounting and documented progress assumptions.
- [ ] Hot completion/IRQ append does not allocate; reserve refusal leaves no accepted work. All affected SDK/provider consumers compile and run under the same contract.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Security and rollback

A completion identity is not resource authority. Keep exact thread/Cell lifetimes across table reuse; never let dead worker callbacks write caller memory. On failure use the prior known-safe whole image after cold boot; outstanding results/effects require reconciliation, not replay. Public ABI freeze/removal follows P01 two checkpoints.

## Assumptions and deviation log

Current queue has one waiter and IPC storage is task-local. New multi-waiter behavior is implementation work, not a current capability inferred from its ring structure. Deviation log: none.
