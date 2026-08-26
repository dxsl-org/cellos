---
title: "Assumptions and Open Questions"
status: pending
created: 2026-08-19
---

# Assumptions and Open Questions

## Assumptions

- Cell-to-Cell Anywhere is a flagship product contract: "call an exported Cell service anywhere by stable endpoint identity."
- Local direct IPC remains the fastest path and must not be wrapped through the broker.
- Remote services are explicit exports; no service is remotely reachable by default.
- First-boot X25519 identity is acceptable for V1; K3/DICE is deferred.
- `.agents/260712-1902-dice-attestation-identity/` Phase P04 owns the stable identity lifecycle; this plan consumes it.
- A self-hosted relay is available for private oracles; no public relay is assumed.
- Broker can use a dedicated blocking attested ingress task plus bounded queues.
- V1 payloads are bounded messages, not streams.
- Failure semantics must be typed and visible.

## Open Questions

- Exact node key path and recovery command.
- Export registry file shape and who owns updates.
- Maximum C2C payload size.
- Dedup TTL and memory budget.
- Broker worker/task model constraints under current scheduler.
- Oracle topology and evidence retention path.
- Relay auth method: node allowlist, shared relay secret, or both.
- Exact export registry path/encoding, chosen at Phase 02 entry, with init/supervisor as authority and broker read-only at runtime.
- Exact broker baseline measurement command for concurrency and saturation targets.

## Decisions Already Locked In This Plan

- Candidate B default.
- Candidate A only after reproducible ingress-blocking root cause against frozen budgets, no userspace correction, and Law-1 double confirmation.
- Relay-first correctness before direct LAN optimization.
- QUIC/ICE/hole punch/public discovery/K3/remote VFS/leases deferred.
