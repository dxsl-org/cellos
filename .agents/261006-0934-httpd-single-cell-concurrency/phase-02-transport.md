---
phase: 2
title: "Integrate real asynchronous IPC and TCP readiness"
status: pending
priority: P1
tier: thinking
dependencies: [1]
---
# Phase 02 — Nonblocking transport prerequisite

## Requirements / architecture
Use the shared C2C Phase 03 bounded submission/completion mechanism, not another kernel queue. Submission returns accepted or Busy without parking on a busy service; accepted work reserves owned bytes and eventual completion capacity. Waiting on idle readiness is allowed only when no runnable HTTP work exists.

## Related files
- Shared prerequisite owns kernel task IPC/completion/syscall and libs/ostd ipc/executor changes if required.
- Net-specific typed additions: libs/api/src/services/ipc.rs, cells/services/net/src/handlers/tcp.rs, socket_table.rs, service-runtime.rs.
- HTTP transport adapter: cells/services/httpd/src/net_ipc.rs; narrow new modules only when useful.
- Existing consumers remain on stable existing APIs; migrate every httpd callsite, no dual sync/async httpd paths.

## Implementation steps
1. Reconcile shared prerequisite status against current code. Complete/reuse its exact admission, peer death, late reply, request identity and owned-buffer lifecycle before asserting this phase passes. Its remote discovery/relay goals are not prerequisites here.
2. Add governed typed nonblocking TCP probe/results plus owner-scoped batched readiness/wait contract. Ready interests: accept, read, write, EOF/error. A writable interest is armed only while outbound bytes exist. Level-triggered retained readiness with generation validation is preferred; do not fire-and-forget TrySend notifications.
3. Bounded waitset: 256 socket interests may map to one/bounded number of retained batch operations; do not allocate one of the current 32 completion slots per idle socket. Paginate ready batches fairly without losing undispatched readiness. Reserve capacity before accepting new operations.
4. Register/recheck/park atomically relative to service state changes; validate no lost wakeup for events before registration, during registration, or before sleep. Completion to wrong cell incarnation is refused.
5. Adapter drives submit/poll/drain for network, VFS and AI without synchronous service_call_typed on the reactor path. One consumer demultiplexes by operation ID and peer generation, not just sender TID. Keep operation buffers until terminal acknowledgement; timeout does not free in-flight grants or imply side-effect cancellation.
6. Explicit close/cancel removes interests before cap reuse. Quarantine late replies by operation identity; no retry of non-idempotent work after uncertain execution. Handle service restart as terminal failure for old incarnation.
7. Preserve existing masked Recv/peer-death behavior and legacy wire meanings. Extend ABI append-only, obtain second Law 1 checkpoint after exact delta and evidence, before promotion.

## Success criteria
- [ ] Busy net/VFS/AI peer never parks httpd reactor during submission; unrelated ready connection advances.
- [ ] Saturation, timeout, late completion, receiver exit and generation reuse neither misdeliver responses nor lose accepted completions.
- [ ] 256 interests work with explicitly bounded queue memory; idle reactor sleeps without O(N) IPC busy scans.
- [ ] Real runtime smoke shows concurrent outstanding requests and wakeup races; existing synchronous consumers still work.

## Assumptions
Shared nonblocking mechanism is pending at planning time. Exact wire IDs and host feature gates must be established by Phase 01, not invented here. API trait declarations and TIMER-only executor are not implementation evidence.

## Security
Attested ownership only; no caller-supplied TID as authority; no pointers outliving storage; cancel/drop cannot cause grant/DMA UAF. Reserve completion storage before dispatch.

## Risk assessment / rollback
Largest dependency risk. Do not substitute a blocking polling loop if prerequisite fails. Drain/cancel accepted operations before coordinated kernel/service rollback. Issued side effects and public ABI allocations are not reversible; retain stable ABI IDs.

## Deviation log
None.
