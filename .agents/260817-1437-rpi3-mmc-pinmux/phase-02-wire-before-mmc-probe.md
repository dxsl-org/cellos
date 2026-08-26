---
phase: 2
title: "Wire Quirk Before MMC Probe"
status: completed
priority: P1
effort: 1h
dependencies: [1]
tier: medium
---

# Phase 2: Wire Quirk Before MMC Probe

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Call the RPi3 pinmux helper immediately before the existing eMMC/SD probe so reset and CMD8 run against the intended external SD pins.

## Requirements

- Functional: call the quirk before `EmmcBlock::probe(SDHCI_BASE)`.
- Functional: retain the existing eMMC-then-SD probe order in `kernel/src/task/drivers/mmc.rs:115`.
- Non-functional: do not alter `SdhciController` RMW/write-spacing in `kernel/src/task/drivers/mmc/sdhci.rs:79`.

## Architecture

Data flow is unchanged after the new pre-probe hook: `mmc::init_driver()` receives no inputs, prepares pinmux on board-rpi3, then probes existing `EmmcBlock` and `SdBlock` using the same `SDHCI_BASE`.

## Assumptions

None — no unverified codebase claims.

## Related Files

- Modify: `kernel/src/task/drivers/mmc.rs`

## Implementation Steps

1. Add a `#[cfg]` module declaration for `pinmux_rpi3`.
2. In `init_driver()`, after `SDHCI_BASE == 0` return and before the first probe log, call `pinmux_rpi3::prepare_external_sd_for_arasan()` under the same `#[cfg]`.
3. Do not add fallback to `0x3F20_2000`/SDHOST or change `SDHCI_BASE`.
4. Do not change CMD8, clock, reset, or block I/O logic.

## Success Criteria

- [x] The hook appears before `EmmcBlock::probe(SDHCI_BASE)`.
- [x] `SDHCI_BASE` remains `0x3F30_0000` for `board-rpi3`.
- [x] Grep finds no new `sdhost`, `mailbox`, `overlay`, `7e202000`, or `0x3F20_2000` production code.

## Security Considerations

No ABI or user authority changes. `libs/api` and `libs/types` are out of scope per Law 1 in `docs/code-standards.md:12`.

## Risk Notes

- Risk: calling too early before MMIO map is valid. Mitigation: only call inside existing MMC init, whose safety comment already requires mapped MMIO before probe in `kernel/src/task/drivers/mmc.rs:116`.
- Rollback: remove the single call and module declaration. The remaining RMW/write-spacing behavior remains unchanged.

## Deviation Log

- Decision: retain the existing eMMC-then-SD probe sequence and controller base; only prepare the board pin routing before it.
