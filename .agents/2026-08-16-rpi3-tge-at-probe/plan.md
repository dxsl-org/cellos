---
title: "RPi3 TGE AT Probe"
description: "One-variable board-rpi3 diagnostic to compare AT S1E0R behavior with HCR_EL2.TGE temporarily cleared inside the existing fault probe."
status: pending
priority: P1
effort: 2h
branch: main
tags: [bugfix, hardware, aarch64, diagnostic]
blockedBy: []
blocks: []
created: 2026-08-16
---

# RPi3 TGE AT Probe

## Overview

OBSERVED current failure reaches `probe_uncategorized_el2_fault` only on uncategorized board-rpi3 traps, then logs baseline `AT S1E0R` and `AT S1E2R` `PAR_EL1` results (`hal/arch/arm/src/aarch64/trap.rs:89`, `hal/arch/arm/src/aarch64/trap.rs:111`, `hal/arch/arm/src/aarch64/trap.rs:126`, `hal/arch/arm/src/aarch64/trap.rs:183`). This plan adds one temporary diagnostic variable: clear only `HCR_EL2.TGE` around a second `AT S1E0R(pc)`, restore `HCR_EL2`, and compare `PAR_EL1` against baseline.

Scope is intentionally diagnostic only. No descriptor policy, EL routing, loader, scheduler, flash tooling, TFTP, or architecture fix is approved by this plan.

## Phases

| Phase | Name | Status | Dependencies |
|---|---|---|---|
| 1 | [Add One-Variable TGE AT Probe](./phase-01-tge-at-probe.md) | pending | none |

## Dependencies

- Requires existing dirty board-rpi3 probe context to remain intact; do not revert unrelated user/session changes.
- Requires manual board-rpi3 SD flash/boot lane because the decisive signal is real Cortex-A53 firmware/EL2 behavior.
- Active-plan sync was not run: `.claude/scripts/set-active-plan.cjs` is absent in this checkout.

## File Ownership

- Phase 1 owns only `hal/arch/arm/src/aarch64/trap.rs` for a temporary probe edit.
- Phase 1 may regenerate build/image artifacts but owns no source changes outside that file.

## Validation Log

- VERIFIED `probe_uncategorized_el2_fault` exists and is board-rpi3 gated: `hal/arch/arm/src/aarch64/trap.rs:89`.
- VERIFIED baseline `AT S1E0R` and `PAR_EL1` read exists: `hal/arch/arm/src/aarch64/trap.rs:116`.
- VERIFIED trap handler calls probe only for `ec == 0`: `hal/arch/arm/src/aarch64/trap.rs:183`.
- VERIFIED boot HCR sets `RW|TGE`: `hal/arch/arm/src/aarch64/el2.rs:50`.
- VERIFIED USER executable pages currently set `PTE_PXN`: `hal/arch/arm/src/aarch64/paging.rs:138`.

## Unresolved Questions

- Does clearing `TGE` around `AT S1E0R` change only the translation regime sampled by `AT`, or can it perturb pending EL0 exception routing on Cortex-A53? The phase mitigates by restoring HCR immediately before any normal handler work continues.
