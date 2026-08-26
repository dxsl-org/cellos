---
title: "RPi3 MMC Pinmux Fix Plan"
description: "Narrow board-rpi3-only plan to route external SD pins to Arasan before existing MMC probe."
status: completed
priority: P1
effort: 7h
branch: main
tags: [bugfix, kernel, rpi3, mmc]
created: 2026-08-17
---

# RPi3 MMC Pinmux Fix Plan

## Scope

Implement the confirmed RPi3 root-cause fix narrowly: before the existing Arasan probe, disconnect GPIO34-39, route GPIO48-53 to ALT3, preserve existing pulls, and keep the BCM2835 RMW/write-spacing path. Real-board gating then exposed two independent follow-on blockers in the same read path: SD CMD9 was issued after CMD7, and data transfers left `TIMEOUT_CONTROL` at its reset minimum. The completed scope includes those two narrow SDHCI corrections and their static guards. No SDHOST driver, mailbox, overlay dependency, SD-card overlay, DT parsing, or non-RPi target changes.

## Verified Code Anchors

- `board-rpi3` selects Arasan SDHCI at `0x3F30_0000` in `kernel/src/task/drivers/mmc.rs:24`.
- MMC init probes eMMC first, then SD on the same base in `kernel/src/task/drivers/mmc.rs:115`.
- Driver init calls `mmc::init_driver()` from the shared driver init sequence in `kernel/src/task/drivers.rs:62`.
- RPi3 peripheral MMIO `0x3F000000-0x3FFFFFFF` is mapped before driver access in `kernel/src/memory/paging.rs:263`.
- Existing RPi3 SDHCI helpers already use 32-bit RMW/write-spacing in `kernel/src/task/drivers/mmc/sdhci.rs:79`.
- Existing GPIO code has GPFSEL RMW precedent for RPi3 mini UART in `hal/arch/arm/src/aarch64/uart_bcm_mini.rs:44`.
- Kernel-boundary docs classify kernel MMC as remaining G2 tech debt, so this plan is a hardware unbrick only, not a driver residency expansion: `docs/specs/15-kernel-boundary.md:210`.

## Phases

| Phase | Name | Status | Depends | Owner Files |
|-------|------|--------|---------|-------------|
| 1 | [Add RPi3 SD Pinmux Quirk](./phase-01-rpi3-sd-pinmux-quirk.md) | completed | none | `kernel/src/task/drivers/mmc/pinmux_rpi3.rs`, `kernel/src/task/drivers/mmc.rs` |
| 2 | [Wire Quirk Before Probe](./phase-02-wire-before-mmc-probe.md) | completed | 1 | `kernel/src/task/drivers/mmc.rs` |
| 3 | [Gate Build And Board Evidence](./phase-03-gate-build-and-board-evidence.md) | completed | 1, 2 | `kernel/src/task/drivers/mmc/core.rs`, `kernel/src/task/drivers/mmc/regs.rs`, `kernel/src/task/drivers/mmc/sdhci.rs`, `tools/rpi3-netboot/test-netboot-scripts.ps1` |

## Data Flow

Boot maps RPi3 peripheral MMIO -> driver init enters MMC -> board-rpi3 quirk writes GPIO GPFSEL only -> GPIO34-39 become input, GPIO48-53 become ALT3 -> existing Arasan reset/probe uses `0x3F30_0000` -> CMD8/ACMD/CMD17 traffic reaches external SD card -> block layer observes `mmc::is_present()`.

## Dependency Graph

Phase 1 must define the only new GPIO write surface before Phase 2 can call it. Phase 2 must land before Phase 3 hardware validation, because CMD8 timeout is the target observable. No phase may touch SDHOST, mailbox, firmware overlay, `libs/api`, `libs/types`, or non-RPi feature gates.

## Top Risks

- High likelihood x high impact: wrong ALT encoding or pin range silently keeps CMD8 timeout. Mitigation: encode constants with register/bit comments, verify GPFSEL4/5 math in review, require real-board log showing no CMD8 timeout.
- Medium likelihood x high impact: quirk regresses existing GPIO48-53 pulls. Mitigation: modify only GPFSEL, never GPPUD/GPPUDCLK.
- Medium likelihood x medium impact: QEMU raspi3b behavior diverges from real BCM2837 pinmux. Mitigation: QEMU is compile/boot smoke only; real board is release gate.

## Backward Compatibility And Rollback

Compile-time `cfg(all(target_arch = "aarch64", feature = "board-rpi3"))` keeps QEMU virt, RPi4, VF2, RV, and x86 unchanged. Rollback is deletion of the new quirk module plus removal of its single call before `EmmcBlock::probe`; RMW/write-spacing remains because it is independent current behavior.

## Test Matrix

- Unit/static: reviewer checks register math for pins 34-39 and 48-53, no pull-register writes.
- Build: `RUSTFLAGS="-C relocation-model=pic" cargo build --release --features board-rpi3 -p cellos-kernel --target aarch64-unknown-none-softfloat`.
- QEMU smoke: `.\run-rpi3.ps1 -SdImage` remains bootable, but does not prove real SD pin routing.
- Hardware gate: real Pi 3 U-Boot still sees SD on mmc0; Cellos log reaches SD probe success or later sector read, and the old CMD8 timeout is absent at both 400k and current driver path.

## Success Criteria

- [x] Implementation remains confined to the MMC driver plus the RPi3 static hardware guard listed in Phases 1-3.
- [x] No SDHOST, mailbox, firmware overlay, DT parser, or ABI files are touched.
- [x] Build passes for `board-rpi3`.
- [x] Real-board evidence proves CMD8 timeout is gone and SD read path works through existing Arasan.

## Open Questions

- None. Bounded boot-time register snapshots remain as bring-up evidence; no per-command or hot-path logging was added.
