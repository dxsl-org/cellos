---
title: "Cellos HAL SoC BCM27xx Slice"
description: "Introduce the first real hal/soc crate and move BCM27xx platform/MMC facts out of generic kernel drivers."
status: superseded
priority: P2
effort: 1d
branch: fix/structure
tags: [refactor, architecture, hal, soc]
blockedBy: []
blocks: []
created: 2026-08-17
---

# Cellos HAL SoC BCM27xx Slice

> Superseded by `.agents/260818-0845-bcm27xx-soc-facts/plan.md` after verifying
> that the previously claimed Phase 1 crate was not present in the tree.

## Scope Contract

- Delivered: `hal/soc/bcm27xx` crate with reusable BCM2837 facts and SDHCI quirks, plus kernel consumers for platform/MMC.
- Preserved: `cells/drivers/*` ownership, existing `board-rpi3` boot path, and all board fallback maps in `kernel/src/boot.rs`.
- Excluded: AArch64 boot fallback migration, RPi3 IRQ/timer extraction from `hal/arch/arm`, generated board manifests, and board feature collapse.

## Phases

| Phase | Name | Status | Depends |
|---|---|---:|---:|
| 1 | Add BCM27xx SoC crate | completed | - |
| 2 | Rewire platform and MMC consumers | in_progress | 1 |
| 3 | Verify, review, and docs | pending | 2 |
