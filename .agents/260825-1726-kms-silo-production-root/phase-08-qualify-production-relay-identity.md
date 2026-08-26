---
phase: 8
title: "Qualify Production Relay Identity"
status: blocked
priority: P1
effort: "not estimated"
dependencies: [4, 7]
tier: thinking
---

# Phase 8: Qualify Production Relay Identity

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Never promote evidence by interpretation; hardware gates require observed results.

## Context Links
- `docs/decisions/0006-block-production-root-pending-exact-product-evidence.md`
- `docs/decisions/0005-mutual-tls-relay-identity.md`
- `phase-04-service-net-mutual-tls-integration.md`
- `phase-06-select-production-root-product.md`
- `phase-07-implement-selected-hardware-provider.md`
- `research/phase-06-production-root-kill-gate.md`

## Overview
`BLOCKED_BY_ADR_0006`. There is no selected production product or implemented product-specific provider to qualify. No physical procedure, production profile, provisioning act, OTP/lifecycle transition, board operation, manufacturing step, or qualification run is approved. Execution begins only after a superseding GO ADR names the exact product and the rewritten Phase 7 completes.

## Key Insights
A functional handshake, provider-ready signal, disabled adapter, simulator, FPGA, development board, or `DEV_REFERENCE` Silo result cannot establish production qualification. Phase 8 requires observed evidence for the exact product, TCB, board, lifecycle, device, protected state/time, per-device record, and shipped artifact approved by the future GO ADR.

## Requirements
Before this phase can be rewritten for execution, all of the following must be true:
- One coherent vendor-signed package satisfies every ADR-0006 product/supply, TCB, content-enforcing protocol, lifecycle/provisioning, AP/board, protected-state/time, and per-device qualification input for the same deployment.
- Fresh architecture, security, procurement, and board reviews accept every package item without inference, and a superseding GO ADR names the exact product and deployment.
- Phase 4 completes its independent software gates: real protected persistence, authenticated time, and a distinct reviewed pending-key binding under frozen KMS opcodes 9–14. Product selection is not a Phase 4 gate, and no KMS ABI change is approved.
- Phase 7 completes against the named product with package-backed physical evidence; disabled plumbing or provider readiness receives no completion credit.
- The exact qualification inventory, fixtures, irreversible-operation controls, provisioning authority, managed-CA inputs, test endpoint, relay verifier/denylist authorities, device-record authority, and evidence custody are named and reviewed.
- The revised phase preserves two independently provisioned production-lifecycle devices, non-transferable qualification records, rollback and power-fault coverage, lifecycle/recovery/RMA/revocation coverage, artifact exclusion, and zero unresolved Critical/High findings.
- Authenticated build provenance binds every qualified kernel, KMS, service-net, provider firmware, policy, and deployment artifact to the exact reviewed source/release inputs.

## Architecture
Current state: `ADR-0006 NO-GO → Phase 7 blocked → Phase 8 blocked`. Permitted future state: `vendor package → accepted reviews → superseding GO ADR → exact Phase 7 implementation/evidence → exact Phase 8 rewrite/review → physical qualification`. There is no executable qualification lane before that sequence completes.

## Assumptions
- No production samples, provisioning authority, manufacturing service, board, firmware, test endpoint, CA, or physical assurance capability is assumed available.
- No candidate-specific path, procedure, threshold, register, tool, or product behavior is inferred from generic evidence.
- Phase 5 evidence remains `DEV_REFERENCE` and cannot satisfy a Phase 8 prerequisite or acceptance gate.

## Related Code Files
No repository, harness, firmware, board, deployment, or external input path is approved. After the superseding GO ADR and completed Phase 7, this section must be replaced with an exact inventory tied to the approved package: qualification harness ownership, production artifact inputs, controlled provisioning/CA/relay inputs, device fixtures, immutable evidence outputs, and focused acceptance targets. The rewrite must not expose secrets or treat living-document updates as qualification evidence.

## Implementation Steps
No qualification or implementation step is authorized. The following are reopening steps, not executable hardware procedures:
1. Receive and accept the complete ADR-0006 vendor package for one exact deployment.
2. Accept a superseding GO ADR naming the exact product, TCB, board, protocol, provisioning contract, and support baseline.
3. Complete the rewritten Phase 7 and its package-backed physical evidence without placeholder completion.
4. Confirm Phase 4's independent software gates and the exact Phase 8 qualification inputs are satisfied.
5. Rewrite this section with product-specific fixtures, paths, procedures, destructive-operation approvals, evidence schemas, thresholds, and focused acceptance gates.
6. Obtain an independent review with no unresolved Critical/High finding; only then begin physical qualification.

## Todo List
- [ ] Pass the complete ADR-0006 reopening gate.
- [ ] Accept a superseding GO ADR naming the exact product.
- [ ] Complete the exact product-specific Phase 7.
- [ ] Complete Phase 4's independent software gates.
- [ ] Replace this blocked plan with exact reviewed qualification inputs and procedures.
- [ ] Begin physical qualification only after every preceding gate passes.

## Test Scenario Matrix
| Priority | Reopening or acceptance scenario | Required result |
|---|---|---|
| Critical | vendor package, review, or superseding GO ADR is incomplete | remain blocked |
| Critical | Phase 7 is placeholder, disabled, unprovisioned, or provider-ready only | remain blocked |
| Critical | exact product/TCB/board/device tuple is absent or inferred | remain blocked |
| Critical | QEMU, FPGA, development, or Phase 5 evidence is offered | deny production credit |
| Critical | qualification record is replayed, transferred, or mismatched | production remains unavailable |
| Critical | old AP/root/firmware/policy/profile/CA/time state is accepted | qualification fails |
| Critical | raw, generic-sign, exportable-key, software, Silo, or insecure fallback exists | qualification fails |
| High | reset, brownout, torn write, bus fault, RMA, or revocation evidence is absent | qualification fails |
| High | exact procedures or destructive-operation controls are not reviewed | do not execute |
| Critical | authenticated build provenance is absent, invalid, or mismatched | qualification fails |

## Success Criteria
- [ ] The superseding GO ADR, completed Phase 7, and revised exact Phase 8 plan all name the same accepted product deployment and TCB.
- [ ] Two independently provisioned production-lifecycle devices pass every product, board, boot, lifecycle, state/time, transport, recovery, and destructive-operation gate in the reviewed rewrite.
- [ ] Each device accepts only its own independently signed qualification record; replay, cross-device substitution, or any tuple change invalidates production state.
- [ ] The ADR-0005 TLS 1.3 client path carries opaque Noise traffic through the protected signer with no raw, generic-sign, exportable-key, development, or downgrade fallback.
- [ ] Production artifacts exclude every development/reference path, and no provider-ready or disabled-plumbing state can enable a production route.
- [ ] Authenticated build provenance binds every qualified production artifact to the exact reviewed source, toolchain, configuration, signer, and release manifest.
- [ ] Independent review reports zero unresolved Critical/High findings for the exact evidence package and observed results.

## Risk Assessment
Irreversible lifecycle operations, protected-counter endurance, destructive fault tests, vendor secrets, and manufacturing/CA custody cannot be safely planned without an exact product contract and controlled fixtures. Remaining blocked prevents speculative procedures from damaging devices or being mistaken for assurance.

## Security Considerations
No production key, provisioning token, RMA secret, vendor secret, or private certificate material may enter the repository or logs. Every production signing request must fail closed when any exact tuple, protected state/time, qualification record, lifecycle, transport, or artifact condition is missing or ambiguous.

## Next Steps
Wait for the full ADR-0006 reopening sequence. Receipt of vendor material alone does not start Phase 8. A superseding GO ADR, completed exact-product Phase 7, satisfied Phase 4 software gates, product-specific Phase 8 rewrite, and clean review are all required before execution.

## Deviation Log
2026-08-26 — ADR-0006 removed the speculative production qualification lane because no exact product was selected. Replaced candidate procedures and provisional paths with explicit reopening inputs and acceptance gates; preserved the two-device physical qualification scope for a future product-specific rewrite rather than claiming completion.
