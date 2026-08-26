---
title: "KMS/Silo Protected Relay Identity"
description: "Deliver a fail-closed P-256 relay signer through KMS, a development-only Silo reference lane, and a separately hardware-qualified production provider."
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
Build `service-net → KMS → RootProvider`: typed TLS 1.3/P-256 relay signing, optional `DEV_REFERENCE` Silo evidence, and a production lane that remains blocked because ADR-0006 selects no root product.
This supersedes `.agents/260712-1902-dice-attestation-identity/phase-02-*` and `phase-03-*`; generic DICE expansion is not on the software mTLS critical path.

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

Phases 1–3 are complete; Phase 6 completed its specified NO-GO branch with no product or irreversible action approved. The overall plan remains **in progress**.
Phase 4's entry contract is approved in [`spec.md`](./spec.md); deep research selects a VF2 UART-root-stream plus STM32H573/SLB9672/AWS composition, but all three gates remain NO-GO until AC-001 through AC-011 are evidenced. Product selection is not its dependency.
Production remains `BLOCKED_BY_ADR_0006` until one ADR-0006 vendor-signed evidence package passes review and a superseding GO ADR names an exact product.

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
- Phase 4 preserves public KMS opcodes 9–14 and places protected persistence, authenticated time, and direct pending-SPKI validation in one root-owned Protected Relay Authority.

## Handoff Boundaries

- Phase 4 may proceed only after `spec.md` AC-001 through AC-011 are evidenced; approving the contract does not open Build and does not wait for production product selection.
- Phase 5 may provide `DEV_REFERENCE` evidence only and cannot satisfy a production gate.
- Phase 6 closed NO-GO without product, procurement, OTP, firmware, board, provisioning, or manufacturing approval.
- Phases 7–8 remain blocked until one coherent vendor-signed package satisfies every ADR-0006 reopening criterion, passes review, and a superseding GO ADR names the exact product.

## Completion Evidence

- Phase 1: 59/59 focused tests; unsafe provider/artifact paths rejected.
- Phase 2: 75/75 host tests and the signed AArch64 `DEV_REFERENCE` QEMU lane passed.
- Phase 3: 140/140 host tests (41 types, 58 KMS, 17 Silo, 24 net); exact out-of-order and full KMS suites passed.
- RV64/AArch64 KMS and current-tree AArch64 development-Silo checks passed clean; the latter used `LLVM_OBJCOPY`.
- Relay-enroll passed 10/10, relay-manifest 11/11, and OpenSSL verified the CSR self-signature.
- Production checker passed 2/2; direct unqualified input failed closed and produced no image.
- Final Phase 3 code and security re-reviews returned GO with no residual findings.
- Phase 6 completed **NO-GO**: ADR-0006 accepted no production product and `research/phase-06-production-root-kill-gate.md` records the evidence/refutation; final security and consistency re-reviews returned GO with zero residual findings, `research/protected-root-report.json` parsed, and the master-plan size check passed at 77 lines.

## Evidence

- `reports/phase-03-deviation-log.md`; `reports/harness/verification.json`; `reports/harness/execution-evidence.json`
- `reports/harness/adversarial-validation.json`; `reports/harness/risk-gate.json`; `reports/harness/review-decision.json`
- `docs/decisions/0006-block-production-root-pending-exact-product-evidence.md`; `research/phase-06-production-root-kill-gate.md`; [`spec.md`](./spec.md); [`DEV_REFERENCE research`](../reports/research-260826-1605-phase4-dev-reference-lane.md)
