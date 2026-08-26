---
phase: 1
title: "Architecture Contract"
status: completed
priority: P0
effort: "0.5d"
dependencies: []
tier: thinking
---

# Phase 1: Architecture Contract

> **Required — deviation-log:** Log every Decision / Deviation / Surprise immediately in § Deviation Log.

## Overview

Turn the current broad "no cross-hart shootdown" wording into an architecture-specific contract before changing code.

## Requirements

- Functional: classify RV64, x86_64, and AArch64 W^X invalidation state from current code.
- Non-functional: no ABI edits, no new features, no claim unsupported by runtime evidence.

## Architecture

Data flow: current W^X docs/comments -> code evidence -> arch-scoped contract -> implementation guardrails.

- OBSERVED: W^X lowers pages before task registration in `kernel/src/task.rs:939`.
- OBSERVED: `protect_page` now documents arch-scoped invalidation: RV64 local `sfence.vma`, x86_64 local `invlpg`, AArch64 broadcast `tlbi ...is`, and bare-physical arches return `NotSupported` (`kernel/src/memory/page_protect.rs:7`).
- OBSERVED: RV64 has SBI IPI send/SSIP receive, but SSIP currently only clears and schedules (`kernel/src/task/scheduler.rs:195`, `hal/arch/riscv/src/rv64/trap.rs:72`).
- OBSERVED: non-RV SMP bring-up is no-op (`kernel/src/task/smp.rs:104`). Hart 0 is not marked in `HART_ONLINE`, and W^X can run on the calling hart rather than always hart 0.
- OBSERVED: AArch64 `flush_tlb_page` already does `dsb ishst; tlbi vaae1is; optional vae2is; dsb ish; isb` (`hal/arch/arm/src/aarch64/paging.rs:63`).
- OBSERVED: x86_64 has local `invlpg` only and documents non-shootdown (`hal/arch/x86/src/x86_64/paging.rs:166`).

## Assumptions

- **Claim:** AArch64 inner-shareable TLBI is sufficient for current Cellos stage-1 W^X mappings.
  **Confidence:** high
  **How to verify:** compare `hal/arch/arm/src/aarch64/paging.rs:63-87` with Arm TLBI `...IS` documentation and boot an SMP AArch64 lane.

## Related Files

- Modify: `kernel/src/memory/page_protect.rs`
- Modify: `kernel/src/loader/wx.rs`
- Modify: `docs/specs/02-memory.md`
- Modify: `docs/specs/19-hardware-isolation-layers.md`
- Modify: `docs/system-architecture.md`

## Implementation Steps

1. Re-grep `protect_page`, `flush_tlb_page`, `tlb_flush_all`, `sbi_send_ipi`, GIC/APIC helpers, and SMP bring-up before editing.
2. Replace broad comments/docs with arch-scoped wording: RV64 pending RFENCE/reuse barrier, x86_64 local-only/no SMP IPI primitive, AArch64 broadcast TLBI implemented but evidence-gated.
3. Keep the existing W^X ordering contract unchanged: relocate, lower, then register task.
4. Add no new public type, syscall, manifest bit, feature flag, or user-visible command.

## Success Criteria

- [x] Every changed claim names its provenance: OBSERVED code, PRIOR report/manual, or INFERRED gate.
- [x] No doc says x86_64 W^X cross-core closure exists.
- [x] AArch64 wording does not demand SGI/IPI for stage-1 TLBI correctness.
- [x] No wording claims the repository-wide D7 limit is closed while x86_64 or required hardware evidence remains gated.
- [x] `git diff --check` passes.

## Security Considerations

The security risk is overclaiming. This phase must make unsupported arch lanes explicit instead of hiding them behind generic W^X status.

## Risk Notes

- Risk: wording accidentally widens scope to Tier-2/domain tables. Mitigation: grep for Tier-2/ASID/domain additions in the diff and reject them.
- Rollback: revert only the doc/comment edits from this phase; no runtime state is changed.
- Irreversible: none.

## Deviation Log

- 2026-08-08 — Decision: kept `kernel/src/loader/wx.rs` in scope because its module contract still said `protect_page` invalidates only the calling hart; that is no longer truthful once the AArch64 HAL emits `tlbi vaae1is` / optional `vae2is`.
- 2026-08-08 — Decision: Layer A wording is split into observed code behavior vs. runtime-gated evidence. AArch64 broadcast TLBI is documented as implemented code, but not promoted to repository-wide D7 completion without a two-PE witness.
- 2026-08-08 — Scope correction: reverted an unrelated Spec 19 wording cleanup about boundary-merge W+X handling after review. Phase 1 remains limited to architecture-specific invalidation contracts.
- 2026-08-08 — Verification: `git diff --check` passed. `cargo check -p vicell-kernel` passed; warnings remained limited to missing host `strip` for `init` and `kernel_fs.img`.
