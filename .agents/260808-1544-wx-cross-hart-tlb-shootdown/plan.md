---
title: "W^X Cross-Hart TLB Shootdown Plan"
description: "Close the W^X stale-permission and VA-reuse window where architecture/runtime support exists, with explicit gates for the rest."
status: blocked
priority: P0
effort: 4d
branch: main
tags: [critical, kernel, memory, security]
blockedBy: []
blocks: []
created: 2026-08-08
---

# W^X Cross-Hart TLB Shootdown Plan

## Overview

P0 security exception scoped to W^X/page-permission invalidation only. It does not reorder the Midori §8 queue. It closes the runnable RV64 lane and corrects the AArch64 contract; D7 stays partially open until the required non-RV SMP/hardware gates exist. 2026-08-08 amendment: RV64 may now add a private physical-to-logical boot/secondary mapping and HSM/all-harts-started startup gate solely to unblock the two-hart W^X proof.

## Scope Contract

- Deliver: architecture-specific W^X invalidation contract, RV64 RFENCE for permission lowering and safe VA/frame reuse, RV64 private physical/logical hart mapping for boot and one secondary, AArch64 proof of existing TLBI broadcast, and gates that refuse x86_64 closure until SMP/IPI exists.
- Exclude: public ABI, syscall numbers, per-domain page tables, ASIDs, Tier-2, generic N-hart scheduler redesign, generic interrupt-controller bring-up, non-RV SMP/IPI, fast IPC, Midori VFS/reactor work.
- Preserve: W^X load-window-then-lower ordering before task registration and no claim that cross-cell data pages are isolated.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Architecture Contract](./phase-01-architecture-contract.md) | completed | none |
| 2 | [RV64 RFENCE and Reuse Barrier](./phase-02-rv64-rfence-shootdown.md) | completed | 1 |
| 4 | [RV64 Physical Hart Mapping and HSM Startup](./phase-04-rv64-physical-hart-mapping-hsm-startup.md) | completed | 2 |
| 3 | [Evidence and Closure Gates](./phase-03-evidence-and-closure-gates.md) | blocked | 2, 4 |

## Data Flow

1. ELF loader records final per-page flags, relocates while writable, then calls W^X lowering before task registration.
2. RV64 records the physical boot hart supplied by HSM firmware as logical boot role 0, starts exactly one HSM-stopped physical secondary as logical role 1, and keeps that mapping private to RV64. Firmware that enters all harts at `_start` remains explicitly host-gated.
3. RV64 probes RFENCE before enabling a secondary hart; lowering and cell-segment teardown publish PTE changes, fence locally, translate each logical target to `{hart_mask: 1, hart_mask_base: physical_id}`, invalidate every other online physical hart, then permit execution or VA/frame reuse.
4. AArch64 uses `TLBI ...IS` broadcast already present in the HAL; x86_64 remains local-only until a separate SMP/IPI primitive exists.
5. QEMU/hardware gates feed evidence reports; only passed architecture lanes may remove the documented limitation.

## Dependencies

- Independent of `.agents/260801-parallel-midori-closure/plan.md`; that plan is blocked on fast-IPC/VFS evidence, not W^X.
- Inherits the completed Phase 10 W^X baseline from `.agents/260727-2101-midori-lessons-cellos/plan.md`.
- `set-active-plan.cjs` was absent in this checkout, so active-plan sync was not performed.
- Phase 3 RV64 QEMU proof now has the Phase 4 mapping prerequisite satisfied; real RV64 hardware remains `HOST-GATED`, and the AArch64/x86_64 hardware/runtime gates remain open as recorded in Phase 3.

## Files

- Recon: [Scout Report](./reports/scout-report.md)
- Research: [IPI/code report](./research/haily-researcher-01-ipi-code-report.md), [architecture manual report](./research/haily-researcher-02-arch-manual-report.md)

## Gates

- Compile gates alone are insufficient.
- QEMU closure requires an SMP stale-write proof, not the existing single-hart `wx-text-write` pass.
- Hardware closure requires real SMP evidence per supported paged architecture class.

## Rollback

Revert the Phase 4 boot-physical mapping, per-hart trap-vector protocol, and all SBI target translation together with RFENCE and teardown/reuse ordering; then restore explicit local-only wording and the prior runtime gate. There is no ABI or persistent-data migration; a live RFENCE failure after PTE mutation is fail-stop because stale writes cannot be repaired.

## Red Team Review

2026-08-08: 8 deduplicated findings accepted (2 Critical, 4 High, 2 Medium). Applied: current-hart-safe mask, pre-reuse invalidation/fail-stop, explicit RV64 ordering and `a3` SBI call, real two-hart test hook, content oracle, exact SMP/compile gates. 2026-08-08 amendment: accepted narrow RV64 physical-hart/HSM startup work after QEMU selected boot hart 1 and rejected `hart_start(1)`. Follow-up red team accepted: all-harts-started firmware is host-gated (not partially supported), each SBI target uses `{mask: 1, base: physical_id}`, every RFENCE/IPI caller uses the mapping helper, and secondary interrupts stay disabled until a per-hart trap vector is installed. Generic x86 SMP/IPI, non-RV SMP/IPI, generic N-hart scheduling, ASID/Tier-2, and public ABI remain deferred by scope.

## Validation Log

Standard tier: 30 claims checked; 28 verified, 0 failed, 2 runtime-gated (AArch64/x86_64 SMP). Reviewer verdict is BLOCKED, not completed: RV64 QEMU sub-lane passed 5/5 and `wx-text-write` passed 2/2, but real RV64 hardware is HOST-GATED and AArch64/x86_64 remain RUNTIME-GATED.

## Handoff

Next: `$hc-cook /home/dmin/cellos/.agents/260808-1544-wx-cross-hart-tlb-shootdown`
