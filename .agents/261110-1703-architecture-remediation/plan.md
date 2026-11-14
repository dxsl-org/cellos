---
title: "Architecture remediation — review 2026-11-10"
description: "Close the 2 Critical, 14 High, 22 Medium and 8 Low findings of the whole-architecture review without changing the ADR-0022 direction."
status: in-progress
priority: P1
effort: "~110–150 working days, single WIP lane (rough; per-phase ranges in phase files)"
branch: main
tags: [architecture, kernel, authority, ipc, ci, governance]
blockedBy: []
blocks: []
created: 2026-11-10
lock: CELL-CONTRACT-0024 (file-bounded exception required per slice)
---

# Architecture remediation plan

**Source:** [architecture review](../reports/architecture-review-261110.md). **Recon and finding→phase map:** [scout report](scout-report.md). **Self-run red team:** [report](reports/red-team.md) (10 findings applied).

**Authority:** nothing here is executable until Phase 00 is approved. Every slice touches files under `CELL-CONTRACT-0024`; each needs its own handoff request, file set and, where `libs/api` or `libs/types` change, Law-1 CP1 before edits and CP2 after evidence. One slice holds the Intel WIP lane at a time.

**Rules for every phase:** behaviour-preserving unless the phase says otherwise; frozen syscall numbers, opcodes and wire bytes do not move; RV64 and AArch64 lanes stay green on shared code (ADR-0022 preserves them); evidence ceiling is host + x86 QEMU (TCG, KVM where available), never physical.

## Phases

| # | Phase | Slice | Dep | Law-1 | Ckpt |
|---|---|---|---|---|---|
| 00 | [Governance, admission and owner decisions](phase-00-governance-admission.md) — completed 2026-11-10 ([approval](admission-approval.md)) | S0 truth | — | — | |
| 01 | [Truthful status baseline](phase-01-truthful-status-baseline.md) — completed 2026-11-10 ([evidence](reports/phase-01-evidence.md)); S0 closed | S0 | 00 | — | ✓ |
| 02 | [x86 C2C gate, lane manifest, reproducible CI](phase-02-ci-x86-gate.md) — completed 2026-11-10 ([evidence](reports/phase-02-evidence.md)); first GitHub run pending | S1 build | 01 | — | |
| 03 | [Explicit build inputs and single image recipe](phase-03-build-inputs.md) — in progress ([evidence](reports/phase-03-evidence.md)): manifest check + disk-layout guard done; untrack/hard error wait for a CI cycle; release blocked on signing | S1 | 02 | — | ✓ |
| 03b | [Single image recipe per architecture](phase-03b-single-image-recipe.md) — split from 03 (owner, 2026-11-10) | S1b | 03 | — | |
| 04 | [One trust posture, guarded copy always on](phase-04-posture-selector.md) — completed 2026-11-10 ([evidence](reports/phase-04-evidence.md)) | S2 authority | 03 | — | |
| 05 | [Single cell and driver catalog](phase-05-cell-catalog.md) — completed 2026-11-10 ([evidence](reports/phase-05-evidence.md)) | S2 | 04 | — | |
| 06 | [Launch by measured identity, per-device PCIe handout](phase-06-launch-identity.md) — completed 2026-11-11 ([evidence](reports/phase-06-evidence.md); Law-1 [CP2](cp2-supervisor-caller-identity.md) confirmed); S2 closed | S2 | 05 | CP1/CP2 if trailer changes | ✓ |
| 07 | [One authority record per Cell incarnation](phase-07-incarnation-authority.md) — completed 2026-11-11 ([evidence](reports/phase-07-evidence.md)); S3 closed | S3 | 06 | — | ✓ |
| 08 | [Generated syscall table and split dispatcher](phase-08-syscall-table.md) — completed 2026-11-11 ([CP2](cp2-syscall-table.md) confirmed; dispatcher match extraction moved to Phase 09); S4 closed | S4 kernel shape | 07 | CP1/CP2 | |
| 09 | [Kernel residents, selftests and module layout](phase-09-kernel-residents.md) — completed 2026-11-11 ([evidence](reports/phase-09-evidence.md)); S4b closed | S4 | 08 | — | ✓ |
| 10 | [Lock ranks and scheduler hot paths](phase-10-lock-ranks-scheduler.md) — completed 2026-11-11 (G1 barrier move deferred by owner); S5 closed | S5 IPC | 09 | — | |
| 11 | [One IPC operation model](phase-11-ipc-model.md) — completed 2026-11-12 ([CP2](cp2-ipc-model.md) confirmed) | S5 | 10 | CP1/CP2 | ✓ |
| 12 | [Shared service runtime and wire modules](phase-12-service-runtime.md) — completed 2026-11-13 ([CP2](cp2-service-wire.md) confirmed); the S6 handoff for Phase 12 is closed | S6 userspace | 11 | CP1/CP2 | |
| 13 | [Init reduction, hot-swap decision, `api` split](phase-13-init-hotswap-api.md) — completed 2026-11-13 ([CP1](cp1-api-split.md), [CP2](cp2-api-split.md) confirmed); S6 closes | S6 | 12 | CP1/CP2 | ✓ |
| 14 | [Kernel `arch/` seam and boot split](phase-14-arch-seam.md) — completed 2026-11-14 ([evidence](reports/phase-14-evidence.md)); the S7 handoff for Phase 14 is closed | S7 arch/memory | 09 | — | |
| 15 | [x86-64 Tier-2 parity](phase-15-x86-tier2-parity.md) | S7 | 14 | — | |
| 16 | [Frame ownership and shootdown contract](phase-16-frame-ownership.md) | S7 | 15 | — | ✓ |
| 17 | [Quota ownership, Tier-3 charging, shared-root hardening](phase-17-quota-shared-root.md) | S8 hardening | 16 | — | |
| 18 | [Driver model and Tier-1 TCB perimeter](phase-18-driver-model-tcb.md) | S8 | 05, 12 | — | |
| 19 | [Spec reconciliation, hygiene and closure](phase-19-closure.md) | S8 | all | — | ✓ |

## Dependencies and order

- S0 first: every later phase reports against the baseline it corrects.
- S1 before any kernel edit: the x86 C2C gate is the regression net for S2–S8.
- S2/S3 (authority) before S4 (file split): the fixes are small targeted diffs; splitting `syscall.rs` first would invalidate every cited line and collide with them.
- S7 depends on S4 only; it may run before S5/S6 if the owner prefers, never in parallel with them.
- Cross-plan: must not overlap an active slice of `261103-2218-x86-platform-breadth`, `261104-0801-x86-igpu-compute` or `261110-1200-cellbox-followups`; Phase 00 reconciles file sets.

## Owner decisions (approved 2026-11-10, all defaults)

1. Spec 26 skeleton services: **exclude from workspace and images**, keep source (default) / delete / keep as is.
2. CSpace: **promote existing `CellAuthority` to the single record** (default) / cut to file tickets.
3. `snapshot.rs`: **cfg-gate in place** (default) / move out of the kernel crate.
4. 32-bit targets (rv32, arm, x86-32): **keep, quarantined under `arch/legacy`** (default) / remove.

## Out of scope

Physical Intel qualification, production keys, new features, any Spec 25/26 implementation beyond making existing code honest, and the open items of the 2026-11-05 security audit.
