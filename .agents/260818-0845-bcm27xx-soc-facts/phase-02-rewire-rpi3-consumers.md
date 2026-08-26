---
title: "Rewire Existing RPi3 Consumers"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 02 — Rewire Existing RPi3 Consumers

## Requirements

- Add the BCM27xx crate only to the AArch64 kernel dependency lane.
- Replace existing BCM2837 controller-address literals in platform/MMC paths with profile facts.
- Keep RPi3 SD pin selection in the board-specific pinmux module.
- Keep SDHCI register access and timing loops in the shared driver.

## Related Code Files

- Modify `kernel/Cargo.toml`.
- Modify `kernel/src/platform.rs`.
- Modify `kernel/src/task/drivers/mmc.rs`.
- Modify `kernel/src/task/drivers/mmc/pinmux_rpi3.rs`.
- Modify `kernel/src/task/drivers/mmc/sdhci.rs` only for policy-value consumption.

## Todo List

- [x] Wire target-scoped dependency.
- [x] Replace only observed SoC literals.
- [x] Keep public contracts and board feature behavior unchanged.

## Evidence

- Inherited checkpoint evidence: the AArch64 `board-rpi3` compile check passed after the cfg fix, and the BCM27xx dependency stayed target-scoped.
- Reviewer-found deviation fixed: the target gate was tightened so the RPi3 path remains compile-only and no board-driver copy was introduced.

## Risk Assessment

A mismatched target dependency or constant could regress AArch64 builds. Undo by restoring the four consumer literals; RISC-V PLIC files are outside this phase.

## Success Criteria

RPi3 consumers compile against the SoC crate with no new board-specific driver copy and no changes under `libs/api/` or `libs/types/`.
