---
title: "Phase 08 Manifest ABI Pre-Design Corpus and Downgrade Model"
description: "Non-promotional evidence slice before any future Manifest ABI decision."
status: blocked
completion_state: "PREDESIGN_COMPLETE / PHASE08_BLOCKED"
priority: P1
tier: thinking
branch: main
created: 2026-08-22
tags: [manifest, abi, predesign, compatibility, downgrade, security]
---

# Phase 08 Manifest ABI Pre-Design Corpus and Downgrade Model

## Overview

This child records the completed, non-promotional evidence slice: freeze the Phase 05
v1/v2 baseline, inventory consumers, and model downgrade/replay threats. It does not
design, authorize, or implement a future Manifest ABI.

**Terminal:** `PREDESIGN_COMPLETE / PHASE08_BLOCKED`.

## Phase status

| Phase | Status | Navigational outcome |
|---|---|---|
| PD-01 — Materialize Phase 05 bytes | Complete | Frozen v1/v2 corpus and tri-state baseline. |
| PD-02 — Close consumer inventory | Complete | Exhaustive classified consumer inventory. |
| PD-03 — Generate downgrade matrix | Complete | Full inherited threat/downgrade model. |
| PD-04 — Validate and review pre-design | Complete | Read-only evidence validation and non-promotional review. |
| PD-05 — Dependency correction and blocked handoff | Complete | Direct gates corrected; terminal remains blocked. |

## Dependency graph

```text
Phase 05 frozen v1/v2 baseline ─┐
Phase 03 provenance, publisher identity/epoch, owner floor/generation ─┼─> pre-design artifacts
Phase 07 atomic publication, rollback, enforceable route ─────────────┘          │
                                                                                   v
                                             PREDESIGN_COMPLETE / PHASE08_BLOCKED
```

Real Phase 08 is directly gated by **Phase 03 + Phase 05 + full Phase 07**. Phase 03
retains `CELLOS-LOADER-SIG-001`; Phase 07 retains `CELLOS-LOADER-RACE-002` and
`CELLOS-LOADER-CLEANUP-003`. The pre-design result changes none of that ownership.

## Evidence and contract index

- [Validation report and authoritative pins](artifacts/predesign-validation-report.json)
  — terminal, counts, dependencies, immutable base revision, content/source-state digests.
- [Frozen corpus](artifacts/manifest-v1-v2-corpus.json) and
  [schema](artifacts/manifest-v1-v2-corpus.schema.json) — byte fixtures, v1/v2,
  tri-state, hostile malformed cases, and Phase 05 source identity.
- [Consumer inventory](artifacts/manifest-consumer-inventory.json) and
  [schema](artifacts/manifest-consumer-inventory.schema.json) — exhaustive roles,
  occurrence-v2 pin, source identity, and re-pin provenance.
- [Downgrade matrix](artifacts/manifest-downgrade-matrix.json) and
  [schema](artifacts/manifest-downgrade-matrix.schema.json) — all threat tuples,
  fail-closed outcomes, inherited ownership, and symbolic-future blocking.
- [Phase 05 Manifest-v2 Tooling](../260821-0642-app-tiers-completion/phase-05-manifest-v2-tooling.md)
  — frozen compatibility baseline and existing tri-state behavior.
- [Phase 08 Manifest-v3 ABI](../260821-0642-app-tiers-completion/phase-08-manifest-v3-abi.md)
  — prohibitions, reserved decisions, and real-design gate.

The report/artifacts are the sole detailed evidence and digest authority. Schema and
artifact changes, source-identity changes, or dependency/risk-ownership changes require
the artifact-defined re-pin/invalidation process; this plan remains only navigation.

## Blocked terminal and next gate

No Manifest v3 code, layout, fixture, migration, persistent routing metadata, Tier 2 or
native-domain work, weaker-route/SAS fallback, or approval is authorized by this child.

The next gate is a separately approved real-Phase-08 design process only after Phase 03
closes authenticated provenance and owner floor/generation semantics, Phase 05's frozen
pin remains continuous, and full Phase 07 qualifies atomic publication, rollback, and
route identity/failure behavior. Until then: `PREDESIGN_COMPLETE / PHASE08_BLOCKED`.
