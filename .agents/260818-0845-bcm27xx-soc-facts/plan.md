---
title: "BCM27xx SoC Facts Slice"
description: "Add a data-only BCM27xx SoC crate and consume existing BCM2837 controller facts without moving board wiring or drivers."
status: completed
priority: P2
effort: 4h
branch: fix/structure
tags: [refactor, hal, soc, aarch64]
blockedBy: []
blocks: []
created: 2026-08-18
---

# BCM27xx SoC Facts Slice

## Scope Contract

- Deliver `hal/soc/bcm27xx` as a `no_std`, data-only crate for immutable BCM2837 controller layout and SDHCI access-policy facts.
- Consume those facts from the existing RPi3 platform/MMC paths without copying or moving driver mechanisms.
- Preserve root `boards/` ownership of identity, boot contract, fallback memory, pinmux group selection, PHY wiring, and enabled-driver lists.
- Exclude AArch64 boot assembly, IRQ/timer mechanisms, DTB parsing, board-feature collapse, SDHCI redesign, and physical-hardware claims.
- Do not modify `libs/api/` or `libs/types/`.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Add BCM27xx SoC facts](./phase-01-add-bcm27xx-soc-facts.md) | completed | none |
| 2 | [Rewire existing RPi3 consumers](./phase-02-rewire-rpi3-consumers.md) | completed | 1 |
| 3 | [Verify and document](./phase-03-verify-and-document.md) | completed | 2 |

## Baseline

Inherited checkpoint for this slice passed formatting, 3 `hal-soc-riscv` tests, 8 board tests, RV64 default check, and AArch64 `board-rpi3` check. The final post-review-fix checkpoint passed the complete 12-gate matrix.

## Compatibility Strategy

Add one target-scoped dependency and replace literals with immutable profile facts. Keep compile-time `board-rpi3` selection, every existing driver entry point unchanged, and the RPi3 compile-only boundary intact.

## Evidence

- Final checkpoint: cfg fix applied; formatting, 2 BCM tests, 3 RISC-V SoC tests, 8 board tests, six RV64/AArch64 compile lanes, RV64 release build, and QEMU boot all passed; reviewer verdict was PASS.
- Reviewer-found deviation: target gating was too broad before the cfg fix; it was narrowed so the BCM27xx dependency stays in the AArch64 lane and the RPi3 path remains compile-only.

## Deferred Work

- RPi3 board descriptor and fallback memory migration.
- BCM2835/BCM2836 interrupt and timer register policy.
- SD pinmux group description and generated board build configuration.
- RISC-V timebase-frequency policy.

## Evidence Boundary

QEMU RV64 regression may be runtime-verified. RPi3 remains compile-only unless a physical-board log is supplied.
