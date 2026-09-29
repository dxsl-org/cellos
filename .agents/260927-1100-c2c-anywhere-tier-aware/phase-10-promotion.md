---
phase: 10
title: "Measured performance and release promotion"
status: blocked
priority: P1
effort: "evidence and release gate"
dependencies: [6]
tier: thinking
---

# Phase 10: Measured performance and release promotion

## Overview

Promote **only capabilities actually proven** on the relevant hardware and security profile. Phase-06 software relay pass is the minimum native C2C Anywhere claim; Phase 03 gates any nonblocking SDK claim, Phase 07 direct LAN, Phase 08 safe shared bulk, and Phase 09 guest service. None of these automatically promotes remote calls to production.

## Requirements and architecture

- Keep separate acceptance cells for (a) existing SAS copied local IPC, (b) native Tier-2 copied IPC, (c) authenticated relay-only remote, (d) direct LAN, (e) Tier-1 ring and true shared bulk, (f) guest bridge and (g) nonblocking call lifecycle. Each advertised claim requires its own completed predecessor and negative tests; record disabled modes honestly.
- Run source-bound tests on a named device/build: local p50/p99 against the project <50 µs target **on supported hardware only**, remote p50/p99/p99.9 at configured deadlines and workloads, concurrency and frame-size sweep, queue depth and capacity exhaustion, worker scheduling/heartbeat under network stalls, memory/pinned pages, socket counts and per-caller fairness. Preexisting single-node QEMU broker benchmark is baseline evidence, not proof of a physical p99 or relay latency.
- Protect safety/control: live-local death detection versus partition suspicion remain distinct, no cross-node automatic physical actuation or failover is implied by a remote RPC. Require an independently reviewed policy if such a product need emerges. Enforce `Public` export and production relay admission only after KMS/Silo protected persistence, authentic time, reviewed pending-key policy, AC-001..AC-012 and independent production/security gates, including the production ADR-0006 block (`docs/project-roadmap.md:133-134`; `../260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`). Do not manufacture a GO by local QEMU passing.
- Define bounded rollback knobs separately: disable remote ingress/export, direct selection, guest export or Tier-1 fastpath without breaking local copied IPC. Drain or revoke live sessions/mappings; a dispatched remote operation can have irreversible effects, so preserve its request ID and `Indeterminate` record for reconciliation.

## Related files

- Integrate with existing focused `scripts/run-c2c-broker-oracle-qemu.sh`, the Phase-06 isolated runner and hardware runners already used in `docs/project-roadmap.md`; publish source/build/environment/evidence pointers there and in `docs/system-architecture.md` after actual proof. Reconcile `docs/specs/20-unified-ipc-contract.md` only once ratified.

## Implementation steps

1. Freeze the exact enabled profile (relay-only, direct, async, bulk, guest), dependent phase checklists and approved production root/relay requirements. Produce a claim-by-claim evidence map: source commit/artifacts, firmware/device, scenario, isolated routes/ACL, adversarial outcomes, measured tails and environment limitations.
2. Run the full local and remote behavioral regression on the claimed profile, then hardware latency/concurrency soak and authenticated relay service trials; compare tails and deadline misses to predeclared budgets, not post hoc averages. Validate max frame and contention without starving critical local work.
3. Perform security and operational review of external identity, rollover, time, relay availability, peer export, privacy-safe logs, stale epochs, dedup windows, resource caps and disable controls. Record exceptions as blocked claims; never waive an external entry gate as a mere performance variance.
4. Update roadmap, system architecture, contract and operator documentation/changelog to describe only exercised capabilities and observed proof. Retire obsolete callers/raw protocol branches and stale `NotSupported` substitutes only after their exact consumers migrate, tests prove parity and governance approves any public ABI change.

## Success criteria

- [ ] Each released capability has a reproducible source-bound proof at its advertised QEMU/device/production rung, negative/error tests, load metrics and documented enable/disable policy; all other capabilities remain visibly disabled.
- [ ] On named supported physical hardware the specified local IPC p99 goal (<50 µs) is measured under a stated load or recorded as unmet, never inferred from QEMU. Remote p99/p99.9, resource caps and watchdog deadlines are measured against an explicit service budget.
- [ ] Production remote operation is allowed only after the independent protected-root, ADR and security gates are explicitly GO; no physical-control safety claim rides on remote reachability.

## Assumptions

- **Claim:** A target physical board and an independently governed production identity/relay environment will be available for the final rung. **Confidence:** low. **Verify:** name device, source revision, authority artifacts and review owners before any production status change; otherwise keep this phase blocked while software claims remain scoped.

## Security considerations

Retained evidence should include hashes, route proof and anonymized correlation without K1, private keys, private payloads or persistent NodeIds. Threat review covers downgrade, cross-tier caller impersonation, replay beyond dedup retention and partial-send ambiguity.

## Risk assessment and rollback

A passing synthetic benchmark can conceal production tail or privilege risks. Stop promotion if hardware/authority or partition tests are missing; retain guarded software capability only at its demonstrated rung. Roll back routes independently and preserve audit entries for possibly executed calls; destructive remote side effects cannot be undone by a binary rollback.

## Deviation log

None.
