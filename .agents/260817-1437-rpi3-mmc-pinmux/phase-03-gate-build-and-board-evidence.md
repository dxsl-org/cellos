---
phase: 3
title: "Gate Build And Board Evidence"
status: completed
priority: P1
effort: 3h
dependencies: [1, 2]
tier: medium
---

# Phase 3: Gate Build And Board Evidence

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Validate the narrow fix with compile, static guards, and real Pi 3 evidence. Hardware is the release gate because QEMU raspi3b cannot prove physical SD pin routing. The board gate exposed and closed two further blockers in the same path: CMD9 ordering and the SDHCI data-timeout exponent.

## Requirements

- Functional: prove the implementation builds with `board-rpi3`.
- Functional: keep follow-on corrections confined to the MMC driver and its RPi3 static guard.
- Functional: collect real-board boot evidence showing CMD8 timeout is gone and SD read works through Arasan.
- Non-functional: record host-gated results honestly when hardware is unavailable.

## Architecture

Test flow: source diff inspection -> board-rpi3 build -> optional SD image/QEMU smoke using existing scripts -> flash/netboot real Pi 3 -> compare boot log against the known failing CMD8 timeout.

## Assumptions

- **Claim:** Real board and U-Boot evidence remain available to the implementor.
  **Confidence:** medium
  **How to verify:** confirm physical Pi 3 serial/netboot setup before closing the phase.

## Related Files

- Modify: `kernel/src/task/drivers/mmc/core.rs`, `kernel/src/task/drivers/mmc/regs.rs`, `kernel/src/task/drivers/mmc/sdhci.rs`, `tools/rpi3-netboot/test-netboot-scripts.ps1`.
- Optional create: `.agents/260817-1437-rpi3-mmc-pinmux/reports/phase-03-evidence.md`

## Implementation Steps

1. Run `git diff --name-only` and confirm all hardware follow-on changes remain inside the MMC driver.
2. Run `RUSTFLAGS="-C relocation-model=pic" cargo build --release --features board-rpi3 -p cellos-kernel --target aarch64-unknown-none-softfloat`.
3. If SD image path is needed, use existing `gen_disk_rpi3.ps1`; it already documents flashing in `gen_disk_rpi3.ps1:180`.
4. Run `.\run-rpi3.ps1 -SdImage` as smoke only; it already builds `board-rpi3` and attaches an SD image in `run-rpi3.ps1:53` and `run-rpi3.ps1:95`.
5. On real Pi 3, capture serial log from boot through MMC probe and first sector read.
6. Compare against the old failure: CMD8 timeout at 400k/100k must be absent without introducing SDHOST/overlay fallback.

## Success Criteria

- [x] `board-rpi3` release build passes.
- [x] QEMU smoke was not used for the physical pinmux gate; generic and board-specific AArch64 checks passed instead.
- [x] Real-board log shows existing Arasan path reads MBR P1-P4, mounts FAT16/FAT32, and reaches `Cellos >`; old CMD8/CMD17 timeout failures are absent.
- [x] Follow-on production changes remain within `kernel/src/task/drivers/mmc/`.

## Security Considerations

No new user MMIO grant. The quirk runs before cells start; it must not expand `resource_registry` allowlists.

## Risk Notes

- Risk: build passes but real board still fails due an unverified ALT/pull assumption. Mitigation: hardware gate blocks completion.
- Risk: adding evidence logs creates persistent debug noise. Mitigation: remove temporary diagnostics before final commit unless user explicitly wants them.
- Rollback: revert Phase 1-2 diff. Cannot undo external SD-card writes made during validation; keep validation read-first unless a write test is explicitly approved.

## Deviation Log

- Surprise: after pinmux fixed CMD8, SD CMD9 still ran after CMD7; the SD branch now reads CSD in Standby before selection.
- Surprise: after CMD9 was fixed, first-sector CMD17 failed with `INT_STATUS=0x00108000`; `setup_data_transfer` now programs `TIMEOUT_CONTROL=0x0E` before transfer registers.
- Evidence: `.agents/debug/rpi3-cmd17-timeout-fixed-hardware.raw` SHA-256 `1655E27864A8B8ED305F82E9C6527E26AEDD6EE8C87A28B8CA7EA486A8054FA3` records the complete hardware boot.
