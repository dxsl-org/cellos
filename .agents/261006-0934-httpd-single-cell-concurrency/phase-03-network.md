---
phase: 3
title: "Bound net service work and qualify socket lifecycle"
status: pending
priority: P1
tier: thinking
dependencies: [1, 2]
---
# Phase 03 — Net capacity and progress

## Requirements / architecture
Changing 18 to a larger MAX_SOCKETS is insufficient. All net-loop paths must yield bounded progress to established TCP clients, including DNS, legacy TLS and NIC operations. Keep one SocketSet owner; no concurrent mutation by helper threads.

## Related files
cells/services/net/src/{socket_table.rs,service-runtime.rs,handlers.rs,interface.rs,dns.rs,tls_handler.rs,handlers/tcp.rs}; net tests; docs/network-api.md; relevant manifests/build features discovered via references. Kernel async dependency owns any transport primitive; do not duplicate it in net.

## Implementation steps
1. Implement agreed profile budgets across cap table, SocketSet storage, listener reserve, pending replies, net heap and other consumers. Make accept promotion transactional: if replacement listener/allocation fails, no leaked cap or lost connection ownership. Separate accepted-cap budget from DHCP/DNS management storage.
2. Remove per-recv avoidable allocation using bounded service-owned scratch/output storage; cap payload by serialized envelope size, not raw IPC buffer length. Bound batches per turn.
3. Integrate readiness after packet/interface progress. Schedule packet, IPC and timer work fairly so neither connection flood nor completion flood starves another class.
4. Convert DNS wait loops and legacy TLS connect/handshake into bounded progress with retained reply state; preserve existing caller protocol. Integrate asynchronous/owned driver request/reply so one stalled NIC response does not hold the entire service loop for one second. Never solve this by removing existing DNS/TLS features.
5. Replace httpd's yield-200-then-remove with explicit graceful-close/drain lifecycle: submit FIN after queued output, retain socket until flushed/terminal or deadline, abort explicitly on deadline. A cap being released must not discard accepted TX data silently.
6. Reclaim owner-generation sockets/waits on cell death/restart through attested existing death mechanism (extend shared primitive only through governance if absent). Cancel pending operations before reuse, and reclaim all listener/accepted/TLS map state. Test root cell death separately from worker-TID exit.
7. Overload must be observable as bounded refusal, not OOM panic. Reserve error-response space when feasible, but do not claim HTTP 503 for connections never admitted.

## Success criteria
- [ ] 256 accepted connections plus listener and other service reserves fit measured profile; allocation refusal is recoverable.
- [ ] Slow DNS/TLS/driver operation does not monopolize TCP dispatch; disconnected NIC yields explicit bounded error, not fictitious successful delivery.
- [ ] Full/partial TX reaches client byte-exact before normal close; reset and deadline reclaim resources.
- [ ] Accept failure, cap exhaustion, cell death and restart return resource accounting to baseline; foreign generation cannot operate old sockets.

## Assumptions
Exact owner-death watch suitable for net is not yet verified; design it in Phase 01 under the shared ABI gate if absent. Other clients' real socket demand must be measured before fixing reserve count.

## Security
Maintain attested CellId/generation authorization, per-owner caps and bounded input queues. No change to raw TLS authority or cryptographic semantics, only execution scheduling.

## Risk assessment / rollback
Network changes affect every consumer. Roll back matched net image/config after draining; do not revert table sizes while more live sockets exist than old capacity. Already sent packets cannot be undone; document abort semantics.

## Deviation log
None.
