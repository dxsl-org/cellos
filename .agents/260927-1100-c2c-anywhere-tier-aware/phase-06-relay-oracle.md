---
phase: 6
title: "Isolated relay-only two-node oracle"
status: blocked
priority: P1
effort: "integration gate"
dependencies: [5]
tier: thinking
---

# Phase 06: Isolated relay-only two-node oracle

## Overview

Prove that two actual Cellos broker runtimes exchange an exported request and response via a self-hosted authenticated relay **with direct node-to-node paths forbidden**. This is the first gate for a guarded software/QEMU remote-call enablement, not a production qualification or physical-safety permit.

## Requirements and architecture

- One self-hosted relay and two Cellos nodes run with independent KMS identity/profiles, service registries and host/network namespaces. Namespace ACLs deny node A↔node B directly; each node reaches only the relay. Preserve route/ACL snapshots and *failed* direct-connect probes as evidence, not merely broker log assertions. Do not use a public relay, host mock peer, plaintext fallback or local broker response as success.
- The exact request ID/path must show source local Cell → node A broker → authority-owned TLS → relay sees NodeIds + opaque Noise ciphertext → node B authenticated broker → one registered export → reply via relay → original live caller. Relay server has no payload/key access. Exercise a non-idempotent method with a receiver-side execution counter and durable witness so duplicate delivery is observable.
- The two-node images select **only** the typed-RPC broker receive parser for `service::NET_BROKER`; the legacy `tests/bench` Echo/Snapshot/Hold oracle callers and parser must not be packaged into those images. Retain the separate single-guest legacy CI profile until an equivalent typed local regression replaces it at cutover. Record build-feature and packaged-Cell inventories alongside the network ACL proof; sending a legacy frame into the typed profile must fail closed, not auto-detect or route locally.
- Deterministic scenarios: valid reply; wrong certificate/peer/external relay endpoint; duplicate live NodeId; missing target (definite pre-write), write/drain uncertainty, drop before/after application delivery, expired deadline before dispatch, lost reply after dispatch, duplicate-inflight `Busy`, retained replay, expired non-idempotent `Indeterminate`, 16-entry in-flight saturation, stale boot/server epoch, **source reboot with an uptime counter below its prior epoch** (must not be mistaken for a valid new protected incarnation), restart and reconnect, stale correlation, bounded frame, max payload and cap overflow. Account for completed-entry expiry versus in-flight never-evict (`cells/services/net-broker/src/c2c_dedup.rs`).
- Deadline/export negatives must include an authority-queued frame delivered **after the origin's local deadline** (the destination's fresh local budget does not extend the origin's call), and a boot config declaring `scope=remote` without a matching independently provisioned peer allowlist or live target (no destination dispatch). Contrast a never-submitted local queue timeout with an accepted-but-delayed unresolved send: the latter resolves `Indeterminate` without a reply, and a later authenticated response must not complete an already resolved operation. Record both the origin outcome and receiver execution count; a request that already reached the destination may still execute.
- Retain a privacy-safe, source/build-bound test bundle using the already defined `cellos.authenticated-evidence/v1` manifest, external run/nonce binding, secret scan and opaque per-run NodeId aliases (`../260819-1409-cell-to-cell-anywhere-core/phase-05-relay-first-remote-correctness-oracle.md:150-183`). QEMU evidence remains software evidence even after passing.

## Related files

- Add or adapt an isolated two-node QEMU integration runner under `scripts/` and behavioral integration oracle under `tests/integration/`, following existing `scripts/run-c2c-broker-oracle-qemu.sh` source-bound build and retained evidence conventions.
- Exercise: `cells/services/net-broker/src/{main,local_runtime,transport,connection_manager}.rs`, `tools/relay-server/`, protected authority and `libs/ostd/src/cluster_endpoint.rs`.

## Implementation steps

1. Assemble two signed *development* Cellos images with distinct NodeIds and the approved protected DEV_REFERENCE relay identity/profile. Verify KMS/Silo AC-012 and Phase-05 ingress/egress gate first. Use network namespaces/firewall rules to force exactly one permitted relay path; capture route and negative-connect evidence before calls.
2. Call an actually exported typed service from A to B, obtain reply and receiver-side execution count; assert no local `NotSupported`/Echo/Snapshot/Hold oracle path can satisfy the fixture. Check relay never observes a plaintext C2C payload.
3. Inject each named failure deterministically while preserving per-request ownership and exact correlation. Run simultaneous requests and sustained saturation to catch queue and socket starvation; record broker worker/network heartbeat gaps, queue depth/bytes, p50/p99/p99.9 and tail deadline misses.
4. Validate and retain the complete secret-scanned source/artifact/network/evidence bundle. Only when all isolation, crypto, delivery and rollback negatives pass may a deliberately configured software profile enable **private remote exports**; default image stays remote-disabled. Hold `Public` and real-hardware/production promotion separately.

## Success criteria

- [ ] Two distinct Cellos images and self-hosted relay complete a request/response under enforced no-direct ACL; receiver executes once; exact request ID and selected relay path reconcile end to end.
- [ ] Each malformed identity, export, deadline, stale epoch, replay, ambiguity, disconnect and congestion case yields the Phase-01 typed outcome without silent loss, local fallback, in-flight eviction or duplicate non-idempotent dispatch in the documented window.
- [ ] Evidence records routes, ACLs, rejected direct probe, source/artifact hashes and raw summaries without K1, private key, full NodeId or plaintext; no hardware/production claim.

## Assumptions

- **Claim:** The approved protected authority lane can provide two independent DEV_REFERENCE identities in the isolated topology once external entry gates pass. **Confidence:** low. **Verify:** exact AC-001..AC-012 artifact and non-executing provisioning authority; never substitute a software K1-only identity while calling it equivalent.

## Security considerations

Do not mistake relay acceptance or TCP write for application execution. Every injected remote origin remains node-granular; local Tier-1 authority is never delegated from a relay certificate. The oracle must test that forcing direct traffic cannot bypass the relay and that missing authentic time leaves the route off.

## Risk assessment and rollback

Bad isolation can give a false positive through a direct LAN route; treat any incomplete ACL capture or direct probe as failed evidence, not an inconclusive pass. Roll back by closing private remote admission and authority sessions; preserve local IPC and retained artifacts. Submitted operations may already have changed remote state and must be reconciled explicitly (`Indeterminate`), never undone by re-running blindly.

## Deviation log

None.
