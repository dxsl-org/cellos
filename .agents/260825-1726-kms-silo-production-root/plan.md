---
title: "KMS/Silo Protected Relay Identity"
description: "Deliver a fail-closed P-256 relay signer through KMS, a development-only Silo reference lane, and a separately hardware-qualified production provider."
status: in-progress
priority: P1
effort: "not estimated"
branch: main
tags: [security, kms, silo, mtls, relay, opentitan]
blockedBy: []
blocks: [260825-sdk-delivery]
created: 2026-08-25
---

# KMS/Silo Protected Relay Identity

## Overview

Build one policy path: `service-net → KMS → RootProvider`. KMS exposes only a typed TLS 1.3 client CertificateVerify operation for a separate P-256 relay key purpose. QEMU Silo is optional `DEV_REFERENCE` evidence and cannot appear in production. Production remains gated until one exact secure-hardware product, firmware, board trust chain, provisioning flow, and rollback store are selected and qualified.

This plan supersedes the production assumptions in `.agents/260712-1902-dice-attestation-identity/phase-02-*` and `phase-03-*`; its landed P00 attestation library remains available but generic DICE expansion is not on the software mTLS critical path.

## Phases

| Phase | Name | Status | Depends on |
|---|---|---|---|
| 1 | [KMS TLS Signing Vertical Slice](./phase-01-kms-tls-signing-vertical-slice.md) | completed | — |
| 2 | [Contain Development Silo Provider](./phase-02-contain-development-silo-provider.md) | pending | 1 |
| 3 | [Certificate Activation and Provisioning](./phase-03-certificate-activation-and-provisioning.md) | pending | 1 |
| 4 | [Service-Net Mutual TLS Integration](./phase-04-service-net-mutual-tls-integration.md) | pending | 1, 3 |
| 5 | [QEMU Relay Software Evidence](./phase-05-qemu-relay-software-evidence.md) | pending | 2, 4 |
| 6 | [Select Production Root Product](./phase-06-select-production-root-product.md) | pending | 1 |
| 7 | [Implement Selected Hardware Provider](./phase-07-implement-selected-hardware-provider.md) | pending | 3, 6 |
| 8 | [Qualify Production Relay Identity](./phase-08-qualify-production-relay-identity.md) | pending | 4, 7 |

## Progress

Phase 1 is complete. Phases 2–8 remain pending and unapproved; Phase 2 is the
next implementation phase and requires explicit approval before work begins.

## Dependency Graph

```text
P1 ─┬─→ P2 ──────┐
    ├─→ P3 → P4 ─┼─→ P5 software-complete
    └─→ P6 → P7 ─┴─→ P8 hardware-qualified
```

P2 and P6 may run in parallel after P1. P5 is never production evidence. P7 cannot begin until P6 names the exact product and trust chain; absence of a viable product blocks P7/P8 without weakening P1–P5.

## Locked Decisions

- Preserve broker/X25519; add independent P-256 relay capability/readiness.
- KMS/provider reconstruct exact CSR and TLS messages; no generic sign/digest API.
- Service-net has a live cell/generation binding and sole runtime signer access.
- Public chain stays outside KMS; protected state binds the complete active
  profile, anti-rollback floors, authenticated time, and qualification latch.
- Production contains one hardware provider and no development/insecure fallback.
- OpenTitan is only a candidate; Phase 6 is a no-code kill gate.

## Handoff Boundaries

- Phases 1–5 are the only current software `/hc-cook` handoff.
- Phase 6 is a no-code product gate and may run in parallel after Phase 1.
- Phase 7 is `BLOCKED_PENDING_PHASE_6`; Phase 6 must replace its provisional
  inventory with exact product/firmware/board paths before a separate cook run.
- Phase 8 requires observed physical evidence from two production-lifecycle
  devices; QEMU/FPGA evidence cannot satisfy it.

## Validation Log

- 2026-08-25 draft review exposed CSR proof, independent readiness, concurrent
  renewal, caller-bound handles, full chain, low-S encoding, build ownership,
  content-enforcing hardware commands, profile/time rollback, and qualification
  gating. Each is now an explicit invariant or gate.
- Final security and consistency recheck: PASS, zero residual Critical/High
  findings (`reports/final-recheck.json`).
- 2026-08-25 Phase 1 completion: 59 focused tests passed (40 KMS, 19 types);
  KMS produced zero warnings and OSTD retained its seven baseline warnings.
  Ten of ten unsafe Cargo feature matrices and 18 of 18 unsafe artifact-checker
  probes were rejected. Clean checker and builder candidates remain intentionally
  blocked with `BLOCKED_PENDING_PHASE_6_7_8`, and the hardware relay provider
  remains compile-blocked pending Phases 6–7. Standard and security reviews
  reported zero residual Critical/High findings
  (`reports/harness/verification.json`,
  `reports/harness/execution-evidence.json`,
  `reports/harness/adversarial-validation.json`,
  `reports/harness/review-decision.json`).

## Evidence

- `reports/scout-report.md`; `research/codebase-report.json`; `research/protected-root-report.json`
- `reports/security-judge.json`; `reports/simplicity-judge.json`
- `reports/final-validator-draft.json`; `reports/final-red-team-draft.json`
- `reports/final-security-gate.json`; `reports/final-consistency-gate.json`; `reports/final-recheck.json`
- `reports/harness/verification.json`; `reports/harness/execution-evidence.json`
- `reports/harness/adversarial-validation.json`; `reports/harness/risk-gate.json`;
  `reports/harness/review-decision.json`
