---
title: "Phase 02 - Stable Node Identity and Exported Endpoint Registry"
status: pending
priority: P1
effort: 4
depends_on: [01]
owner: "identity-and-registry"
---

# Phase 02 - Stable Node Identity and Exported Endpoint Registry

## Context Links

- Research: `research/research-audit.md`
- Semantics: `research/semantics-report.md`
- Assumptions: `reports/assumptions.md`

## Overview

Priority P1. Replace per-run broker identity with stable first-boot X25519 node identity and require explicit service exports before remote calls.

## Key Insights

- Current broker generates per-run X25519 and derives `CellNetId`, which breaks stable Anywhere addressing.
- G1 `CellNetId` is already X25519 public key shaped; K3/DICE is deferred.
- Remote service access must be opt-in.
- This plan consumes and integrates `.agents/260712-1902-dice-attestation-identity/` Phase P04 stable identity; it does not define a second key lifecycle.

## Requirements

- Functional: first-boot node key, pinned key path and permissions, clone-image rekey protocol, node id recovery story, export registry, endpoint versioning, retry class per export, and public/remote disabled until key lifecycle is pinned.
- Non-functional: no Law 1; VFS-backed config must be bounded; key never logged.

## Entry Decisions

- Choose exact node-key path and permissions from the DICE P04 stable identity slice.
- Choose exact export registry path and encoding.
- Confirm one owner for key lifecycle: DICE P04 produces the stable identity slice; this plan integrates it into Cell-to-Cell Anywhere.
- Keep public and remote export modes disabled until key lifecycle, clone handling, and lost-key recovery are pinned.

## Architecture

Data flow: DICE P04 stable identity slice -> broker loads pinned node key -> derives `CellNetId` -> read-only broker loads init/supervisor-provisioned export registry -> advertises exported endpoints only -> remote peers route by `(node_id, service_id, export_id)`.

## Related Code Files

- Future owner phase: `cells/services/net-broker/src/identity.rs`
- Future owner phase: `cells/services/net-broker/src/transport.rs`
- Future owner phase: `cells/services/net-broker/src/routing.rs`
- Future owner phase: config files under `/etc/cellos/` only, not repo data.
- External plan owner consumed here: `.agents/260712-1902-dice-attestation-identity/phase-04-k2-per-node-identity.md`

## Implementation Steps

1. Define key source and first-boot behavior.
2. Define export registry format with explicit `service_id`, `export_id`, version, and retry class.
3. Define migration from no key to first-boot key.
4. Define lost-key and cloned-image behavior.
5. Add route records keyed by node id plus export id.
6. Define export registry authority: init/supervisor provisions; broker reads only at runtime; atomic replace and version validation; permissions fail closed.

## Todo List

- [ ] Specify node-key path and permissions.
- [ ] Specify export registry path.
- [ ] Specify duplicate-node detection behavior.
- [ ] Specify operational recovery for lost key.
- [ ] Specify clone-image rekey command and audit log.
- [ ] Pin one key lifecycle owner through DICE P04.
- [ ] Pin export registry authority and atomic replacement contract.

## Success Criteria

- Reboot keeps the same `CellNetId`.
- Cloned images are detected or rejected before joining the cluster.
- No service is remotely callable unless exported.
- Phase 03 cannot start until key path, permissions, lost-key recovery, and clone rekey behavior are pinned.
- Public/remote stays disabled until key lifecycle and export registry authority are pinned.

## Risk Assessment

- Risk: cloned image shares node key. Likelihood medium, impact high. Mitigation: boot epoch plus duplicate-node alarm and documented rekey flow.
- Risk: VFS key load failure bricks remote. Likelihood medium, impact medium. Mitigation: local mode still works; broker reports remote disabled.

## Security Considerations

Private key stays broker-local. Export registry is allowlist-only, init/supervisor-provisioned, read-only to broker at runtime, atomically replaced, version-validated, and fail-closed on permission or parse error. Cluster id is not authorization.

## Rollback

Disable remote exports and fall back to local-only broker behavior. Existing local services remain unaffected.

## Next Steps

Proceed to broker ingress task and bounded queues.
