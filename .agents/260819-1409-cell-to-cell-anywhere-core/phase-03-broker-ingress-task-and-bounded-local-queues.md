---
title: "Phase 03 - Broker Ingress Task and Bounded Local Queues"
status: complete
priority: P1
effort: 4
depends_on: [01]
owner: "broker-runtime"
---

# Phase 03 - Broker Ingress Task and Bounded Local Queues

## Context Links

- Transport: `research/transport-report.md`
- Dependency graph: `reports/dependency-graph.md`

## Overview

Priority P1. Candidate B is implemented: the main broker task blocks in
`sys_recv_attested`, while dedicated worker, reply-pump, and network-poller
roles operate bounded broker-owned state.

## Key Insights

- Kernel-attested caller identity, not payload identity, binds each request.
- Fixed request/reply/in-flight capacities make overload observable as `Busy`.
- Broker-owned monotonic request ids and stale history prevent receive-order or
  delayed-reply confusion.
- The network poller retains independent cadence while local ingress blocks.

## Requirements

- Functional: local IPC ingress, bounded request queue, bounded reply queue, request/reply correlation, backpressure result, heartbeat discipline.
- Non-functional: no unbounded allocation, no blocking relay receive on ingress task, no guessed caller identity; Phase 04 cannot start until scheduler coexistence is prototyped.

## Entry Gate

- Phase 01 budgets are frozen and measured.
- Phase 02 pins remote disabled unless protected KMS identity and later gates pass.
- Evidence remains a single-guest local-runtime oracle, not remote completion.

## Exit Gate Before Phase 04

- [x] Verify scheduler coexistence for ingress, worker, reply pump, heartbeat,
  and network polling.
- [x] Verify request/reply correlation under concurrent local traffic.
- [x] Verify bounded queue wakeups and full-queue `Busy`.
- [x] Verify zero kernel heartbeat/watchdog termination markers in the oracle.
- [x] Verify network polling progresses while ingress and soak traffic run.

## Architecture

Data flow: local cell -> blocking attested ingress -> fixed request queue -> fair broker worker -> fixed reply queue -> bounded `sys_try_send` pump. A separate role polls network state without holding the broker lock across network IPC.

## Related Code Files

- `cells/services/net-broker/src/local_runtime.rs`
- `cells/services/net-broker/src/local_ingress.rs`
- `cells/services/net-broker/src/local_queue.rs`
- `cells/services/net-broker/src/local_queue/state/`
- `cells/services/net-broker/src/reply_pump.rs`
- `cells/services/net-broker/src/runtime_roles.rs`
- `cells/services/net-broker/src/local_runtime/restart_oracle.rs`
- `cells/tests/bench/src/scenarios/c2c_broker_oracle*`
- `tests/integration/tests/c2c-broker-oracle.rs`
- `scripts/run-c2c-broker-oracle-qemu.sh`

## Implementation Steps

1. Bound request, reply, in-flight, stale-history, and per-caller state.
2. Bind ingress to kernel-attested sender TID, Cell id, and generation.
3. Assign broker-owned monotonic request ids and reject stale completion.
4. Separate worker, reply-pump, and network-poller roles.
5. Retain Busy replies with bounded attempts and explicit terminal accounting.
6. Exercise concurrency, saturation, soak, scheduler progress, and supervised
   broker restart in isolated RV64 QEMU.

## Todo List

- [x] Size local ingress queue.
- [x] Size local reply queue.
- [x] Define and unit-test per-caller fairness.
- [x] Define watchdog-safe blocking points.
- [x] Define reply correlation and stale reply rejection.
- [x] Prove scheduler coexistence with network polling.
- [x] Record the result against Phase 01 budgets.

## Success Criteria

- Local ingress never needs attested `TryRecv`.
- Queue full returns `Busy`, not silent drop.
- The broker network poller cannot starve local attested ingress.
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

Phase 04 remains blocked on the protected remote identity/provider gates. The
completed local ingress runtime may continue serving local-only broker work.
Remote envelope, relay, two-node, and direct-LAN claims remain open.

## Verification Evidence

- 63/63 focused host library tests pass.
- The isolated restart-enabled RV64 oracle passes 1/1 with 1,000 measured
  calibration calls; successful 1/2/4/8/16-client sweeps; 10,000/10,000 soak
  calls with zero silent drops, positive network progress, and no heartbeat or
  watchdog termination marker; bounded overflow at queue peak 16; and
  supervised restart with clean three-role drain, stale old-TID failure, fresh
  broker state, and successful retry on a replacement TID.
- Evidence ceiling: single-guest local QEMU only.

## Scope Decision

- **2026-08-29:** The Phase 03 coexistence gate is explicitly local-only. It
  covers the implemented single-guest network/beacon poller running beside
  attested ingress, worker, and reply-pump roles. It does not cover an
  unimplemented relay loop. Relay-specific starvation and remote-session
  progress remain Phase 05/08 gates and cannot be inferred from this phase.
