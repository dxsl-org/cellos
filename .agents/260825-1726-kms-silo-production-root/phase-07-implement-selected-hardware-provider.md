---
phase: 7
title: "Implement Selected Hardware Provider"
status: blocked
priority: P1
effort: "not estimated"
dependencies: [3, 6]
tier: thinking
---

# Phase 7: Implement Selected Hardware Provider

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible OTP/lifecycle actions or contract changes.

## Context Links
- `docs/decisions/0006-block-production-root-pending-exact-product-evidence.md`
- `phase-06-select-production-root-product.md`
- `research/phase-06-production-root-kill-gate.md`
- `docs/decisions/0005-mutual-tls-relay-identity.md`

## Overview
`BLOCKED_BY_ADR_0006`. Phase 6 completed NO-GO and selected no product. No hardware adapter, provider, transport, board integration, provisioning flow, firmware integration, or disabled placeholder is approved. Implementation begins only after the full reopening gate passes and a superseding GO ADR names one exact product.

## Key Insights
A disabled provider, feature flag, mailbox shape, protocol stub, or hardware-neutral placeholder cannot complete this phase. Freezing plumbing before the exact product would invent its firmware ABI, transport, board topology, provisioning flow, and failure semantics. ADR-0006 approves no KMS ABI change; the existing typed KMS/provider seam remains the boundary.

## Requirements
One coherent vendor-signed evidence package must bind all of these to the same proposed deployment before reopening review:
1. Exact MPN/order code, package, marking, die/mask/metal revision, certification target, errata, and PCN baseline.
2. Authorized procurement, availability, lifecycle, security-response, last-order/last-ship, and dated product/firmware support terms.
3. Exact production ROM, ROM_EXT, application firmware, cryptolib, build configuration, signed hashes/manifests, update/recovery keys, and anti-rollback policy.
4. A versioned content-enforcing protocol with positive and negative vectors for protected reconstruction of complete CSR and TLS 1.3 signed content, with every generic-sign, digest, DER, test, rescue, and alternate-firmware bypass absent or cryptographically unreachable.
5. Exact lifecycle, OTP, entropy, personalization, custody, interruption, debug/rescue, RMA, zeroization, destruction, rekey, replacement, and revocation contracts.
6. A named AP/board/revision and approved schematic/netlist/BOM with root-owned first-stage boot authorization and exact bus, reset, interrupt, boot, power, strap, tamper, debug, and recovery behavior.
7. Vendor-qualified endurance and power-cut semantics for rollback-resistant atomic state and authenticated-time freshness, outage, recovery, and rollback rules.
8. A non-transferable per-device qualification record and hardware evidence plan covering substitution, replay, reset, brownout, torn writes, bus faults, debug lock, RMA wipe, post-RMA denial, revocation, and destructive zeroization.

Receipt alone is insufficient. Architecture, security, procurement, and board reviews must accept every item without inference, then a superseding GO ADR must name the exact product and approved deployment. No implementation or irreversible action is permitted before that ADR.

## Architecture
Current state: `Phase 6 NO-GO → ADR-0006 block`. Reopening state: `one vendor-signed package → four fresh reviews → superseding GO ADR → exact product-specific Phase 7 rewrite → implementation`. There is no approved production provider architecture between the block and the GO ADR.

## Assumptions
- No product, firmware, protocol, board, transport, provisioning, lifecycle, or manufacturing behavior is assumed.
- No candidate-specific design evidence is promoted into a product contract.
- Phase 4 is independent software work; satisfying its gates cannot satisfy this hardware gate.

## Related Code Files
No repository or vendor implementation path is approved. After a superseding GO ADR, this section must be replaced with an exact create/modify/delete inventory for the named product-specific provider, board transport, pinned protocol interfaces, protected state/time integration, production artifact inputs, provisioning/update tools, and pinned external firmware source or auditable binary. Each entry must cite the accepted package and name a focused acceptance target.

## Implementation Steps
No implementation step is authorized. The following are reopening steps, not hardware implementation:
1. Receive one vendor-signed package satisfying all eight input categories for one deployment.
2. Complete architecture, security, procurement, and board reviews with no inference or unresolved blocking finding.
3. Accept a superseding GO ADR naming the exact product, deployment, TCB, board, protocol, provisioning contract, and support baseline.
4. Rewrite this phase with exact approved paths, interfaces, procedures, ownership, destructive-operation controls, and focused physical acceptance targets.
5. Begin product-specific implementation only from that reviewed rewrite.

## Todo List
- [ ] Receive the complete vendor-signed evidence package.
- [ ] Accept all eight evidence categories in fresh reviews.
- [ ] Record a superseding GO ADR naming the exact product.
- [ ] Replace this blocked plan with exact product-specific paths and procedures.
- [ ] Begin implementation only after the preceding gates pass.

## Test Scenario Matrix
| Priority | Reopening or acceptance scenario | Required result |
|---|---|---|
| Critical | any evidence category is missing, split across products, or inferred | remain blocked |
| Critical | package received but reviews or GO ADR incomplete | remain blocked |
| Critical | product, firmware, board, or protocol is not named exactly | remain blocked |
| Critical | generic sign/digest/DER or alternate-firmware bypass is reachable | reject candidate |
| Critical | immutable boot, atomic state, or authenticated time is AP-asserted | reject candidate |
| Critical | disabled plumbing is proposed as phase completion | deny completion credit |
| Critical | KMS ABI change is proposed without a separate accepted decision | reject change |
| Critical | QEMU, FPGA, development, or reference evidence is substituted | deny production credit |

## Success Criteria
- [ ] A superseding GO ADR names one exact product and approved deployment after every ADR-0006 reopening input passes review.
- [ ] The rewritten plan contains only package-backed exact paths, interfaces, procedures, and physical gates.
- [ ] The named provider preserves the existing typed KMS/provider seam and passes the Phase 1 conformance contract without adding a generic signing or private-material path.
- [ ] The protected root independently enforces signed content, AP boot authorization, protected state, authenticated time, lifecycle, and per-device qualification before every operation.
- [ ] Production artifacts exclude development/reference providers and all downgrade paths.
- [ ] No disabled placeholder, unprovisioned device, or provider-ready signal is counted as completion or production qualification.

## Risk Assessment
Vendor errata, supply, protocol, state endurance, authenticated time, board boot topology, and irreversible lifecycle operations are load-bearing and currently unknown for an exact product. The safe action is to remain blocked rather than encode assumptions.

## Security Considerations
Production signing must remain unavailable whenever product identity, firmware, AP measurement, qualification, policy/profile state, monotonic state, authenticated time, transport freshness, or protected persistence is absent or ambiguous. No raw, generic-sign, exportable-key, development, or disabled-placeholder fallback is permitted.

## Next Steps
Wait for the single ADR-0006 vendor package and full review sequence. Phase 8 remains blocked even after receipt; it can be rewritten and started only after the superseding GO ADR and a completed, evidence-backed Phase 7 provider.

## Deviation Log
2026-08-26 — ADR-0006 replaced the speculative product-specific lane with a hard reopening gate. Removed candidate-specific assumptions and executable procedures, recorded that no KMS ABI change or disabled hardware plumbing is approved, and blocked implementation until a superseding GO ADR names the exact product.
