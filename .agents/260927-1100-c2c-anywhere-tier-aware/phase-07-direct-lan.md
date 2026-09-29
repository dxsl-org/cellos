---
phase: 7
title: "Direct LAN Noise optimization and path change"
status: pending
priority: P2
effort: "optional transport optimization"
dependencies: [6]
tier: thinking
---

# Phase 07: Direct LAN Noise optimization and path change

## Overview

After relay correctness is proven, introduce a direct peer path for configured reachable peers. A direct route is a latency optimization for the **same** authenticated remote NodeId/service contract; a beacon or IP address never changes caller authority. Internet reachability already exists via relay after Phase 06; STUN/ICE/hole-punch/public discovery require separate scope and proof, not this phase.

## Requirements and architecture

- Use configured `PeerTicket` addresses and authenticated Noise KKpsk0 with protocol-role ordered NodeId/cluster prologue (`cells/services/net-broker/src/transport/noise_session.rs:33-72`). Beacons only offer discovery hints to preconfigured peers; no beacon can create export permission or assert a remote per-cell origin.
- Implement a bounded direct listener/accept loop **and** connect lifecycle; neither is wired from broker main today (`cells/services/net-broker/src/main.rs:118-179`). Enforce peer NodeId→live session reuse, socket cap shared with DHCP/ARP/user traffic, four-slot Noise pool no-eviction, deadline-bound exact TCP frame reads/writes, heartbeat-safe yielding handshake and closed-socket cleanup (`connection_manager.rs:78-85`; `transport/tcp_framing.rs:28-60`).
- Selection/failover: connection manager observes peer/session generation and route health, picks direct when authenticated and healthy and relay otherwise. Existing in-flight request stays bound to its submission and retains request ID/epoch across path change; an uncertain previous path is `Indeterminate`, not an instruction to auto-retry a non-idempotent method. No direct→insecure, plaintext or K1-only fallback; protected relay authority remains the relay owner.
- Keep control/safety loops local. Multicast loss is reachability uncertainty, not confirmed service death; neither beacon nor socket eviction may authorize actuation ([Spec 20 draft §2.5](../../docs/specs/20-unified-ipc-contract.md)).

## Related files

- Modify: `cells/services/net-broker/src/{connection_manager,local_runtime,beacon,identity,routing}.rs`, `cells/services/net-broker/src/transport/{connection_pool,noise_session,tcp_framing}.rs` and typed peer-route tests.
- Extend Phase-06 two-node runner with same-subnet direct and partition tests; use explicit selected-path/request IDs in diagnostics.

## Implementation steps

1. Implement bounded accept/connect and NodeId→session mapping before permitting a direct endpoint; reject duplicate peer, wrong cluster, wrong static key, overfull pool and stale route generation without displacing a live session.
2. Add yielding exact-read TCP framing with per-step deadline and watchdog re-arm. Verify partial prefix/payload, socket close, oversized frame and stalled handshake without a broker-wide heartbeat miss.
3. Benchmark direct versus relay with identical typed calls and source-bound p50/p99/p99.9 under 1/2/4 peer pressure; choose direct only when it improves the path without violating deadlines. Reuse relay for unreachable direct peer only with honest `NotSubmitted`/`Indeterminate` classification.
4. Exercise direct→relay→direct transitions during no outstanding requests and during an accepted request; assert unique request IDs, no double non-idempotent execution, bounded memory and authenticated same-node identity.

## Success criteria

- [ ] Same-LAN nodes exchange a real exported request via direct authenticated Noise, while forcing direct failure uses the already proven relay without weakening identity or error classification.
- [ ] Short read, handshake stall, peer restart, capacity pressure and path transition exercise exact sender-visible outcomes with zero socket leak or heartbeat miss.
- [ ] Internet relay oracle stays green and remote/guest/Tier-2 never gains SAS shared-memory authority.

## Assumptions

- **Claim:** Available net-cell socket budget allows at least one direct peer alongside required relay and system sockets. **Confidence:** medium. **Verify:** record live sockets at peak, not merely `MAX_SOCKETS` constant; if full, return `Busy` and keep relay-only path.

## Security considerations

Do not equate transport reachability with trust or downgrade to raw TCP if Noise/relay admission fails. Bind presented envelope source to the authenticated session, never to multicast machine_id alone.

## Risk assessment and rollback

Direct route can increase deadline tails or mask a relay outage via hidden fallback. Disable direct selection only; keep verified relay-only route. A direct request already possibly delivered remains `Indeterminate` until authenticated response; rolling back a connection does not roll back its side effects.

## Deviation log

None.
