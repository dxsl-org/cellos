---
title: "Success Gates"
status: pending
created: 2026-08-19
---

# Success Gates

## Phase Gates

- 01: recovery status plus frozen budgets: local IPC p99 regression <=5%, zero watchdog expirations, queue/cache memory budgets, 10k accepted unary soak, measured broker concurrency/saturation target.
- 02: stable NodeId survives reboot, DICE P04 is the single identity lifecycle owner, exports are opt-in, and public/remote stays disabled until key lifecycle is pinned.
- 03: scheduler prototype verifies `ostd` task coexistence, request/reply correlation, bounded wakeups, heartbeat/watchdog, and network polling before Phase 04.
- 04: typed endpoint API, taxonomy, retry mapping, accepted/dispatched/completed dedup states, expiry, and exhaustion pass unit/property tests.
- 05: relay-only two-node oracle passes, including registration proof, duplicate NodeId rejection, sender-visible relay failure mapping, no-evict-in-flight, and dedup expiry/exhaustion.
- 06: LAN-direct oracle passes after relay oracle.
- 07: each failure maps to typed status and observable path/counter; in-flight sessions are never evicted for pool pressure.
- 08: failure injection matrix passes in isolated two-node setup, including stale response, broker restart, half-open TCP, path reorder, and silent-drop attempts.
- 09: Red Team and validation are filled before implementation/ship.

## Product Gate

Cell-to-Cell Anywhere may be called "runtime complete" only after one exported service call succeeds over relay and LAN direct in isolated two-node runs, with retained evidence and no hardware claim unless hardware was actually used.

## Candidate A Gate

All conditions must be true:
- Candidate B fails against frozen Phase 01 latency/watchdog/queue/soak/concurrency targets.
- Failure is reproduced and root-caused specifically to blocking ingress.
- No userspace correction fixes it.
- Candidate A patch is limited to attested `TryRecv` parity.
- User gives two explicit Law-1 confirmations.
