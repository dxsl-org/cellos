---
phase: 5
title: "QEMU Relay Software Evidence"
status: pending
priority: P1
effort: "not estimated"
dependencies: [2, 4]
tier: medium
---

# Phase 5: QEMU Relay Software Evidence

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links
- `tools/relay-server/`
- `.agents/260825-sdk-delivery/phase-02-relay.md`
- `research/protected-root-report.json`

## Overview
Prove the complete software path on AArch64 QEMU with the development Silo provider and existing mandatory-mTLS relay. This closes software integration only, not hardware qualification.

## Key Insights
QEMU can prove policy mediation, CSR enrollment, TLS behavior, Noise opacity, cancellation, and failure semantics. It cannot prove immutable identity, non-exportability against privileged software, OTP, rollback resistance, TRNG quality, side-channel resistance, or physical provisioning.

## Requirements
- Build two fresh Cellos/QEMU nodes with distinct development relay identities and managed test-CA certificates issued from their KMS-generated CSRs.
- Use production-shaped manifests but an explicit `DEV_REFERENCE` artifact/profile marker.
- Exercise direct-path success and direct exhaustion followed by mTLS relay.
- Relay observes only authenticated NodeIds and opaque Noise ciphertext.
- Collect deterministic PASS/FAIL evidence; never relabel it production-ready.

## Architecture
Two QEMU AArch64 nodes run net-broker, service-net byte carriers, and
DEV_REFERENCE Protected Relay Authority TLS endpoints through the host relay
server. A test CA accepts each constrained CSR and issues a client certificate
with the exact NodeId extension.

## Assumptions
- **Claim:** The harness can launch two isolated QEMU instances with separate disk/config mounts and network endpoints. **Confidence:** medium. **How to verify:** inspect existing multi-node scripts before adding a harness.
- **Claim:** Current relay fixtures can issue the required private extension from a CSR. **Confidence:** medium. **How to verify:** extend `_relay_test_support.py` in isolation first.

## Related Code Files
| File | Action | Test impact |
|---|---|---|
| integration/QEMU harness under existing scripts/tests | Create | end-to-end oracle |
| `tools/relay-server` fixtures | Reuse/modify | CSR/certificate issuance |
| image/config generation scripts | Modify | per-node manifests |
| KMS/net/silo test hooks | Modify minimally | observable PASS markers |
| project docs/evidence | Modify | status qualification |

## Implementation Steps
1. Build reproducible development guest, authority TLS endpoint, service-net
   carrier, broker, disk, and relay-server artifacts.
2. Generate two distinct reference keys, export constrained CSRs, issue matching client chains, and mount separate manifests.
3. Start relay server and both nodes; wait for explicit service readiness rather than sleeps.
4. Prove valid mTLS registration and an opaque Noise exchange after direct-path exhaustion.
5. Run negative matrix: unauthorized caller, attacker TLS server, malformed CSR,
   wrong chain/SPKI/extension, stale TLS generation, chunk replay/reorder/
   truncation, authority reset, relay CA/SAN failure, revoked NodeId,
   malformed/oversized frame, duplicate session, and cancellation.
6. Prove direct Noise success does not contact relay and every unavailable prerequisite returns `NotSupported`/typed failure without raw fallback.
7. Archive concise logs and label the result `software-complete / non-hardware-qualified`.

## Todo List
- [ ] Build deterministic two-node harness.
- [ ] Provision distinct matching test identities through CSR.
- [ ] Prove direct-first relay behavior.
- [ ] Execute failure and downgrade matrix.
- [ ] Record non-production evidence accurately.

## Test Scenario Matrix
| Priority | Scenario | Expected |
|---|---|---|
| Critical | two-node relayed Noise message | delivered; relay cannot parse payload |
| Critical | raw/K1 fallback probe | no route/registration |
| Critical | CSR/certificate/KMS/Silo mismatch or outage | fail closed |
| High | duplicate/replacement/cancel race | bounded newest session only |
| Medium | direct path available | direct succeeds, relay unused |

## Success Criteria
- [ ] Two QEMU nodes complete CSR enrollment and authenticated mTLS relay with opaque Noise payloads.
- [ ] All ADR-0005 client-side negative paths fail before unsafe registration/delivery.
- [ ] No test or document claims hardware non-exportability or production readiness.

## Risk Assessment
Test hooks or fixture CA material must never enter production images. Evidence must distinguish host, QEMU guest, and relay observations.

## Security Considerations
Use disposable identities only. Scan generated artifacts/logs for private key leakage; keep test CA material outside tracked project files.

## Next Steps
Software delivery may stop here. Phase 6 closed NO-GO with no product selected; production remains `BLOCKED_BY_ADR_0006`, and Phases 7–8 require the full reopening process and a superseding GO ADR.

## Deviation Log
None.
