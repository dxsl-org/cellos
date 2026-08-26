---
title: "Add BCM27xx SoC Facts"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 01 — Add BCM27xx SoC Facts

## Requirements

- Create a `no_std` workspace crate under `hal/soc/bcm27xx`.
- Model BCM2837 peripheral, GPIO, AUX mini-UART, and Arasan SDHCI addresses as immutable SoC facts.
- Model the existing Arasan minimum write-spacing quirk as policy data.
- Add host unit tests for address relationships and policy values.

## Related Code Files

- Create `hal/soc/bcm27xx/Cargo.toml`.
- Create focused files under `hal/soc/bcm27xx/src/`.
- Modify root `Cargo.toml` workspace members.

## Todo List

- [x] Add the crate and focused profile/policy modules.
- [x] Add tests proving BCM2837 facts.
- [x] Run crate tests on the host target.

## Evidence

- Inherited checkpoint evidence: the host tests for the BCM2837 facts crate passed before consumer rewiring.

## Risk Assessment

Wrong MMIO facts can break RPi3 boot. Undo by removing the crate before consumer rewiring; no persistent format or ABI change is involved.

## Success Criteria

The crate is `no_std`, under 200 lines per code file, has no driver mechanism, and its host tests pass.
