---
phase: 5
title: "Authenticated broker-to-broker request and reply"
status: blocked
priority: P1
effort: "implementation gate"
dependencies: [1, 2, 4]
tier: thinking
---

# Phase 05: Authenticated broker-to-broker request and reply

## Overview

Connect the existing bounded local ingress, envelope/dedup and Noise modules to a real protected relay session and a live local exported Cell. This phase implements the remote request/reply flow behind a disabled-by-default route; Phase 06 provides the isolated two-node evidence needed to enable a development route.

## Requirements and architecture

`LocalEndpoint` remains direct. A bounded `RemoteEndpoint` call first sends a typed local request to net-broker. The broker binds kernel-attested caller `(cell_id,generation,TID)` to request ID and destination epoch, owns one queued request and a deadline, encrypts the canonical V1 envelope into a Noise record and hands only that record to the protected authority. Peer broker receives an authenticated record, checks the Noise session's peer NodeId / cluster / local NodeId / broker incarnation against **all** envelope IDs, looks up the current export and its scope/allowlist, then validates target server epoch **before** dedup and local delivery. Only the registered live local service may execute; response follows inverse Noise/relay path to the exact live caller generation.

- Remote principal is authenticated **node**, not a peer-supplied Cell path or tier; `ClusterId` routes, never authenticates. Reject unauthorized export before local IPC even if dedup sees the frame. Private export is limited to admitted configured peers; `Public` scope needs separate explicit policy/governance, not blanket Internet access.
- The current `RemoteExports` parser has only five boot-config fields and deliberately remains `NoSecureIdentity`; it provides neither a peer allowlist nor live destination binding (`export_registry.rs:82-89,144-189`). At admission intersect **separately provisioned** peer policy, authenticated Noise NodeId, typed method/retry contract and live registered destination before `ReceiveGate`; `scope=remote` alone is never sufficient.
- Caller-facing `RemoteEndpoint::new` currently checks only nonzero metadata (`libs/ostd/src/cluster_endpoint.rs:106-124`). Replace construction at enablement with broker-issued, session/epoch-bound metadata; nonzero bytes do not confer authority. Authenticate endpoint observations using live peer session and export registry, invalidate on restart/rekey/disconnect.
- Preserve existing V1 112-byte header and 3,712-byte payload cap, 16-entry/30-second dedup and 16 replay floors unless Phase 01 ratifies a change (`cells/services/net-broker/src/c2c_envelope.rs:7-19`, `docs/system-architecture.md:1635-1651`). Admission returns explicit `Busy`; never evict in-flight state or silently replay an expired non-idempotent request. Current source replay floors order `src_boot_epoch` numerically; beacon boot epoch comes from `sys_get_time_ms()` and is **not** a protected cross-reboot monotonic source. Require authority-bound nonrollback incarnation evidence or a separately reviewed replay-model revision before remote ingress. Charge remote-origin quota to bounded peer ingress, not an unbounded recipient allocation.
- Track owner state `NotSubmitted/Submitted` plus authenticated request ID and ADR-0009 transport-local correlation. A definite pre-accept/destination-absence proof gives `Unreachable`; a possibly delivered request with unknown outcome gives `Indeterminate`. A deadline is `Timeout` only on proof no dispatch was possible, not merely because authority accepted but a reply was lost. Stale endpoint/restart after possible submission remains `Indeterminate` in the first API: the existing `RemoteCallError` has no `Respawned` variant and `NoService` requires an authenticated definite absence. No hidden retries, downgrade, raw TCP identity or local fallback.
- `RelativeDeadline` is only a duration in V1, not a shared-clock timestamp (`c2c_envelope.rs:108-112`). Keep the origin admission deadline across local queue/authority/relay, transmit at most its remaining budget and apply a separate bounded destination-arrival budget. Do not restart the caller's deadline on transit or infer global expiry from the receiver's monotonic clock; origin expiry after possible submission without a reply is `Indeterminate`.
- A synchronous **deadline-bounded** call can be the first externally testable endpoint; Phase 03's async SDK is a separate enhancement and must use exactly this request lifecycle once available. Do not delay core relay correctness on new public async ABI, and do not claim nonblocking callers before Phase 03 is exercised over this path.

## Related files

- Modify: `libs/ostd/src/cluster_endpoint.rs`, `cells/services/net-broker/src/{main,local_runtime,connection_manager,transport,export_registry,c2c_receive,c2c_dedup,identity}.rs`, typed `cells/services/net-broker/src/local_runtime/request_dispatch.rs`.
- Reuse: `cells/services/net-broker/src/{c2c_envelope,c2c_deadline,server_epoch,noise_identity}.rs`, `libs/types/src/c2c.rs`, `libs/ostd/src/ipc.rs`; owner of relay TLS stays the external Phase-04 protected authority.

## Implementation steps

1. Design a bounded broker-local typed `RemoteCall` ingress with per-caller queue/in-flight limits under [Spec 20 Draft v3](../../docs/specs/20-unified-ipc-contract.md). The existing `service::NET_BROKER = 8` benchmark receiver has an unrestricted eight-byte sequence, so a new byte-0 tag **cannot** share that live parser. Prefer one selected parser per image profile during guarded development (legacy local oracle **or** typed RPC, never auto-detect/fallback); require proof no legacy caller is packaged with the typed image, and cut over benchmark-only consumers and tests before general enablement. Coexistence in one image instead needs a separately registered receiver/service ID and Law-1 approval. Reply only to the original kernel-attested live caller; generation/sequence mismatch is never success.
2. Implement exact-read/length-bounded TCP/Noise framing and connection lifecycle in the selected relay transport; current direct framing reads only once per length-prefixed payload (`cells/services/net-broker/src/transport/tcp_framing.rs:28-60`) and `find_session` always returns `None` (`connection_manager.rs:78-85`). Authenticate before parser/dedup. Keep connection pool <=4 and no-evict semantics until measured capacity governance changes.
3. Implement one ingress gate taking *session-authenticated* peer, local NodeId/cluster and registered live export; intersect an independently provisioned peer allowlist, matching method/retry class and live destination generation before applying `ReceiveGate`/dedup and bounded sender-masked service IPC. The current parsed registry is always remote-disabled and has no peer-allowlist field; do not treat loading `scope=remote` as enablement or give `scope=public` implicit admission. Never synthesize a local attested per-cell identity from remote bytes.
4. Map response, broker death, remote service restart, timeout, correlation error, relay disconnect and reconnect to the Phase-01 outcome matrix. Count rejected/expired/busy work with bounded-cardinality metrics and no plaintext/key/complete NodeId logs.
5. Host-test two interacting broker state machines with malformed/unauthorized frames and incomplete transport records; compile RV64 route **still disabled for general callers** pending the Phase-06 two-node oracle. If Phase 03 is complete, additionally integrate its awaitable request handle without changing remote classification.

## Success criteria

- [ ] Two broker state machines complete an authenticated exported request/reply and reject unauthenticated source, wrong destination/cluster, config-only `scope=remote` without allowlist/live target, mismatched retry contract, stale epoch, replay and over-capacity without local dispatch.
- [ ] Short TCP reads/writes, peer restart, lost reply after dispatch, late response, and two concurrent correlated requests produce the specified definite/indeterminate result; no duplicate non-idempotent local execution in the retention window.
- [ ] `LocalEndpoint` still bypasses broker. The legacy single-guest local oracle remains green in its separate regression image while the guarded typed-RPC image passes its own full receive-path negatives; before general promotion retire the legacy-only parser/callers and replace that regression coverage. Remote stays disabled in general images before Phase 06.

## Assumptions

- **Claim:** The existing net-cell socket budget and broker worker cadence can sustain a deadline-bound two-node relay session without a kernel ABI change. **Confidence:** medium. **Verify:** worst-case handshake/receive-timeout model and isolated Phase-06 oracle; fail closed if heartbeat is missed.

## Security considerations

Protection boundary is two hops: relay mTLS authenticates the external route, E2E Noise authenticates the actual peer NodeId. A relay error is not application completion; ingress must validate export authorization before delivering even a well-formed encrypted envelope.

## Risk assessment and rollback

High risks: remote spoof via envelope field, double-execution after unknown completion, broker-wide stall on blocking socket. Run remote behind exact enabled-profile gate; revert by refusing new remote submissions, draining/marking submitted work `Indeterminate` and closing sessions, without touching local copied IPC. Already executed remote side effects are irreversible; never claim rollback makes them disappear.

## Deviation log

None.
