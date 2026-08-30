---
title: "KMS/Silo Protected Relay Identity"
description: "Deliver an authority-owned relay TLS endpoint, a development-only Silo reference lane, and a separately hardware-qualified production provider."
status: in-progress
priority: P1
effort: "not estimated"
branch: main
tags: [security, kms, silo, mtls, relay, production-root]
blockedBy: []
blocks: [260825-sdk-delivery]
created: 2026-08-25
---

# KMS/Silo Protected Relay Identity

## Overview
Build `net-broker Noise ciphertext → service-net byte carrier → Protected Relay
Authority TLS endpoint`: optional `DEV_REFERENCE` evidence and a production lane
that remains blocked because ADR-0006 selects no root product. Public KMS
opcodes 9–14 remain frozen; generic DICE expansion is not on the relay path.

## Phases

| Phase | Name | Status | Depends on |
|---|---|---|---|
| 1 | [KMS TLS Signing Vertical Slice](./phase-01-kms-tls-signing-vertical-slice.md) | completed | — |
| 2 | [Contain Development Silo Provider](./phase-02-contain-development-silo-provider.md) | completed | 1 |
| 3 | [Certificate Activation and Provisioning](./phase-03-certificate-activation-and-provisioning.md) | completed | 1 |
| 4 | [Service-Net Mutual TLS Integration](./phase-04-service-net-mutual-tls-integration.md) | blocked (software entry gates only) | 1, 3 |
| 5 | [QEMU Relay Software Evidence](./phase-05-qemu-relay-software-evidence.md) | pending | 2, 4 |
| 6 | [Production Root Product Kill Gate](./phase-06-select-production-root-product.md) | completed (NO-GO) | 1 |
| 7 | [Implement Selected Hardware Provider](./phase-07-implement-selected-hardware-provider.md) | blocked (ADR-0006) | 3, 6, superseding GO ADR |
| 8 | [Qualify Production Relay Identity](./phase-08-qualify-production-relay-identity.md) | blocked (ADR-0006) | 4, 7, superseding GO ADR |

## Progress

Phases 1–3 are complete; Phase 6 completed its specified NO-GO branch with no
product or irreversible action approved. ADR-0008 fixes Phase 4 TLS ownership,
but the overall plan remains **in progress**. Phase 4 stays NO-GO until
build-entry AC-001 through AC-011 are evidenced. After Build, AC-012 gates relay
enablement and Phase 4 completion. Production remains `BLOCKED_BY_ADR_0006`
until a vendor-signed evidence package passes review and a superseding GO ADR
names an exact product.

## Dependency Graph

```text
P1 → {P2, P3, P6}; P3 → {P4, P7}; {P2, P4} → P5; P6(NO-GO) → ADR-0006 evidence → superseding GO ADR → P7; {P4, P7} → P8
```
P4 is software-only and product-independent. P5 is `DEV_REFERENCE` only; P7–P8 cannot start before the ADR-0006 reopening gate passes.

## Locked Decisions

- Preserve broker/X25519; relay uses an independent P-256 capability.
- KMS/provider reconstruct exact CSR/TLS messages; no generic sign/digest API.
- Public chains stay outside KMS; protected state binds lifecycle and rollback floors.
- Frozen opcode 14 exposes only the active key; Phase 4 must add authenticated pending-key binding.
- Runtime stays sealed without protected persistence and authenticated time; no insecure fallback.
- ADR-0006 selects no production root product; no KMS ABI change or disabled hardware placeholder is approved.
- Phase 4 preserves public KMS opcodes 9–14 and places protected persistence, authenticated time, direct pending-SPKI validation, and the complete fixed relay TLS endpoint in one root-owned Protected Relay Authority; the legacy public signer denies in production.

## Handoff Boundaries

- Phase 4 may proceed only after `spec.md` AC-001 through AC-011 are evidenced; approving the contract does not open Build and does not wait for production product selection.
- Phase 4 must implement ADR-0008 after entry GO and pass AC-012 before relay enablement or completion; AC-012 is not a Build entry gate.
- Phase 5 may provide `DEV_REFERENCE` evidence only and cannot satisfy a production gate.
- Phase 6 closed NO-GO without product, procurement, OTP, firmware, board, provisioning, or manufacturing approval.
- Phases 7–8 remain blocked until one coherent vendor-signed package satisfies every ADR-0006 reopening criterion, passes review, and a superseding GO ADR names the exact product.

## Completion Evidence
- Phase 1 passed 59/59 focused tests; Phase 2 passed 75/75 host tests plus signed AArch64 `DEV_REFERENCE` QEMU; Phase 3 passed 140/140 host tests.
- RV64/AArch64 KMS, current-tree AArch64 development-Silo, relay-enroll 10/10, relay-manifest 11/11, and OpenSSL CSR verification passed.
- Production checker passed 2/2; direct unqualified input failed closed without an image; final Phase 3 code/security reviews returned GO.
- Phase 6 completed **NO-GO** with no product selected; ADR-0006 and `research/phase-06-production-root-kill-gate.md` record the reviewed evidence.
- Evidence: `reports/phase-03-deviation-log.md`; `reports/harness/{verification,execution-evidence,adversarial-validation,risk-gate,review-decision}.json`; [`spec.md`](./spec.md); [`DEV_REFERENCE research`](../reports/research-260826-1605-phase4-dev-reference-lane.md).
