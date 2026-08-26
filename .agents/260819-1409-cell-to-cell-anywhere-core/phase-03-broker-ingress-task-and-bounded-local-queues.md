---
title: "Phase 03 - Broker Ingress Task and Bounded Local Queues"
status: pending
priority: P1
effort: 4
depends_on: [01, 02]
owner: "broker-runtime"
---

# Phase 03 - Broker Ingress Task and Bounded Local Queues

## Context Links

- Transport: `research/transport-report.md`
- Dependency graph: `reports/dependency-graph.md`

## Overview

Priority P1. Implement Candidate B runtime shape: a dedicated blocking `sys_recv_attested` ingress task feeds bounded in-cell queues, while network/relay loops keep their own polling cadence.

## Key Insights

- `sys_recv_attested` exists for blocking receive and carries kernel caller identity.
- Current broker uses `sys_try_recv`, which has no attestation flag.
- Avoiding Law 1 means Candidate B must separate local ingress from relay/network polling.

## Requirements

- Functional: local IPC ingress, bounded request queue, bounded reply queue, request/reply correlation, backpressure result, heartbeat discipline.
- Non-functional: no unbounded allocation, no blocking relay receive on ingress task, no guessed caller identity; Phase 04 cannot start until scheduler coexistence is prototyped.

## Entry Gate

- Phase 01 budgets are frozen.
- Phase 02 has pinned public/remote disabled-until-key-lifecycle behavior.
- Prototype scope is approved as throwaway evidence, not product completion.

## Exit Gate Before Phase 04

- Verify `ostd` task scheduler coexistence for ingress task, broker worker, heartbeat, and network polling.
- Verify request/reply correlation under concurrent local and broker traffic.
- Verify bounded queue wakeups and full-queue `Busy`.
- Verify zero watchdog expirations in the prototype window.
- Verify relay/network polling still progresses while ingress blocks in `sys_recv_attested`.

## Architecture

Data flow: local cell -> blocking attested ingress task -> parse request -> bounded broker queue -> broker worker -> local direct call or remote sender -> reply queue -> response to caller.

## Related Code Files

- Future owner phase: `cells/services/net-broker/src/main.rs`
- Future owner phase: new focused broker queue/runtime modules under `cells/services/net-broker/src/`
- Future owner phase: no `libs/api` touch.

## Implementation Steps

1. Define ingress task lifecycle and heartbeat contract.
2. Define bounded queues and overflow mapping to `Busy`.
3. Define caller identity propagation from ingress to worker.
4. Define shutdown and broker restart behavior.
5. Define how relay/network loops wake the worker without consuming ingress capacity.
6. Define broker-owned request ids for replies so concurrent broker traffic cannot satisfy the wrong caller.
7. Build and discard a scheduler coexistence prototype before protocol Phase 04.

## Todo List

- [ ] Size local ingress queue.
- [ ] Size remote reply queue.
- [ ] Define per-caller fairness.
- [ ] Define watchdog-safe blocking points.
- [ ] Define reply correlation and stale reply rejection.
- [ ] Prototype scheduler coexistence with network polling.
- [ ] Record prototype result against Phase 01 budgets.

## Success Criteria

- Local ingress never needs attested `TryRecv`.
- Queue full returns `Busy`, not silent drop.
- Relay polling cannot starve local attested ingress.
- Replies are matched by broker-owned request id, not receive order alone.
- Phase 04 is blocked until scheduler, queue wakeup, heartbeat, and network polling gates pass.

## Risk Assessment

- Risk: extra task/queue adds latency. Likelihood medium, impact medium. Mitigation: compare to captured local IPC baseline; Candidate A requires ingress-specific root cause.
- Risk: queue memory pressure. Likelihood medium, impact high. Mitigation: fixed capacity and static maximum payload.

## Security Considerations

Only kernel-attested caller identity enters authorization. Payload-supplied identity is ignored.

## Rollback

Disable remote broker path and route local calls directly. Remove queued worker path in one phase because no ABI changes exist.

## Next Steps

Proceed to C2C envelope and dedup semantics.
