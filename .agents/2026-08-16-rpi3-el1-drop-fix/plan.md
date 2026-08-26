---
title: "RPi3 EL1 Drop Fix"
description: "Board-rpi3-only boot fix that drops from firmware EL2 to EL1h before kmain while preserving the generic AArch64 EL2 virtualization lane."
status: pending
priority: P1
effort: 6h
branch: main
tags: [bugfix, hardware, aarch64, boot]
blockedBy: []
blocks: []
created: 2026-08-16
---

# RPi3 EL1 Drop Fix

## Overview

USER-OBSERVED hardware evidence: on real Pi 3 Cortex-A53, `HCR_EL2.TGE=1` makes `AT S1E0R(pc)` behave as if effective `SCTLR_EL1.M=0`; toggling TGE changed `PAR_EL1` from identity `0x100000000` to the correct Cell code PA `0x228c000`. Therefore the rejected alternative is clearing `PTE_PXN`: with TGE forcing the wrong translation regime/effective EL1 MMU-off state, the PTE is not consulted and weakening XN would not address root cause.

OBSERVED-CODE alignment: existing runtime branches already select EL1 vectors, EL1 context switch, EL1 timer, and EL1 paging when `el2::is_el2()` is false (`hal/arch/arm/src/aarch64/trap.rs:70`, `hal/arch/arm/src/aarch64/context.rs:63`, `hal/arch/arm/src/aarch64/timer.rs:74`, `hal/arch/arm/src/aarch64/paging.rs:217`). The fix is board-rpi3-only: if firmware enters at EL2, configure EL2 just enough to enter AArch64 EL1h, `eret` to `.el1_entry`, and do not call `el2_mark_active`.

## Phases

| Phase | Name | Status | Dependencies |
|---|---|---|---|
| 1 | [Split Board RPi3 EL2 Entry to EL1h](./phase-01-board-rpi3-el1-drop.md) | pending | none |
| 2 | [Run Regression and Hardware Gates](./phase-02-regression-and-hardware-gates.md) | pending | 1 |
| 3 | [Remove Temporary Probe After Hardware Pass](./phase-03-probe-cleanup.md) | pending | 2 |

## Dependencies

- Phase 1 must preserve generic non-board EL2 host behavior because `stage2_regs.rs` and `vcpu.rs` require true EL2 runtime (`hal/arch/arm/src/aarch64/stage2_regs.rs:3`, `hal/arch/arm/src/aarch64/vcpu.rs:204`).
- Phase 2 owns pass/fail evidence; Phase 3 cannot start until real hardware reaches the expected boot point with the temporary probe still present.
- Active-plan sync not run: `.claude/scripts/set-active-plan.cjs` is absent in this checkout.

## File Ownership

- Phase 1 owns `hal/arch/arm/src/aarch64/boot.rs` and, only if needed, `hal/arch/arm/src/aarch64/el2.rs` comments/helpers.
- Phase 2 owns generated debug evidence under `.agents/debug/` and no source files.
- Phase 3 owns `hal/arch/arm/src/aarch64/trap.rs` cleanup and optional `.agents/debug/` summary.

## Validation Log

- VERIFIED `_start` branches to `.el2_init` when `CurrentEL==2`: `hal/arch/arm/src/aarch64/boot.rs:37`.
- VERIFIED current `.el2_init` sets `RW|TGE`, marks `EL2_ACTIVE`, and calls `kmain`: `hal/arch/arm/src/aarch64/boot.rs:43`, `hal/arch/arm/src/aarch64/boot.rs:84`.
- VERIFIED `.el1_entry` already enables `CPACR_EL1`, selects `SP_EL1` via `SPSel`, sets stack, clears BSS, and calls `kmain`: `hal/arch/arm/src/aarch64/boot.rs:95`.
- VERIFIED trap init chooses `VBAR_EL1` when `EL2_ACTIVE` is false: `hal/arch/arm/src/aarch64/trap.rs:70`.
- VERIFIED context switch chooses `__switch_el1` when `EL2_ACTIVE` is false: `hal/arch/arm/src/aarch64/context.rs:63`.
- VERIFIED board-rpi3 timer path already uses EL1 CNTP and BCM2835 system timer independent of generic EL2 CNTHP path: `hal/arch/arm/src/aarch64/timer.rs:41`.

## Unresolved Questions

- Whether any boot firmware on supported non-RPi3 AArch64 hardware enters EL2 but still expects the current generic EL2 host path. This plan avoids the question by gating the EL1 drop behind `feature = "board-rpi3"` only.
