---
phase: 1
title: "Prove U-Boot Handoff Compatibility"
status: pending
priority: P1
effort: "3h"
dependencies: []
tier: thinking
---

# Phase 1: Prove U-Boot Handoff Compatibility

## Overview

Before writing the SD/TFTP workflow, prove the exact U-Boot artifact, image format, load/entry address, and DTB-in-`x0` handoff that Cellos needs on RPi3B v1.2.

## Requirements

- Functional: prove U-Boot target/artifact source: primary `u-boot-rpi` artifact `/usr/lib/u-boot/rpi_3/u-boot.bin`, fallback upstream build `make rpi_3_defconfig` producing `u-boot.bin`.
- Functional: prove Cellos image compatibility: current raw `kernel8.img`, legacy `uImage` via `mkimage`, or AArch64 `Image` wrapper.
- Functional: prove load and entry address. Cellos RPi3 is linked and entered at `0x80000`, not `0x40000000`.
- Functional: prove DTB handoff. `hal/arch/arm/src/aarch64/boot.rs` treats `x0` as DTB pointer and moves it to `x19`; the selected U-Boot command must provide that.
- Non-functional: do not change source code while proving; use generated artifacts and U-Boot scripts only.

## Architecture

Data flow:

1. Build `vicell-kernel` with `board-rpi3`; convert ELF to raw binary as current SD path does.
2. Inspect ELF entry and raw first bytes; record whether it already matches U-Boot `Image` expectations.
3. Generate candidate images: raw `cellos-rpi3.bin`, legacy `cellos-rpi3.uImage` with load/entry `0x80000`, and only if needed an `Image`-compatible wrapper proposal.
4. Boot U-Boot from SD and run candidate scripts manually from UART first.
5. Capture whether Cellos reaches its early UART marker and whether DTB-dependent platform init behaves.

Dependency graph:

`U-Boot artifact proven` -> `Cellos image candidates generated` -> `DTB handoff verified` -> `one static boot script selected`.

## Assumptions

- Claim: U-Boot `bootm <addr> - <fdt>` is the most likely path to pass FDT cleanly for a wrapped image; raw `go 0x80000` may not set `x0` to FDT.
  Confidence: medium
  How to verify: run both candidate scripts and compare UART/platform logs; inspect U-Boot docs and command behavior for the chosen installed version.
- Claim: loading to `0x80000` is safe after U-Boot relocates itself high.
  Confidence: medium
  How to verify: use `bdinfo` and a manual `tftp 0x80000` probe before auto-booting.

## Related Files

- Read-only: `kernel/linker-rpi3.ld`
- Read-only: `hal/arch/arm/src/aarch64/boot.rs`
- Read-only: `gen_disk_rpi3.ps1`
- Create: `.agents/debug/<timestamp>-rpi3-uboot-handoff-report.md`
- Create: `.agents/debug/<timestamp>-rpi3-uboot-uart.raw`

## Implementation Steps

1. Verify installed U-Boot source path and version. Preferred artifact: `/usr/lib/u-boot/rpi_3/u-boot.bin`; if absent, plan a local upstream U-Boot build with `rpi_3_defconfig`.
2. Build Cellos board-rpi3 and produce raw `cellos-rpi3.bin` using the same `objcopy -O binary` flow as `gen_disk_rpi3.ps1`.
3. Record `readelf -h` entry point and confirm linker origin `0x80000`.
4. Generate a legacy `uImage` candidate with `mkimage -A arm64 -O linux -T kernel -C none -a 0x80000 -e 0x80000 -n Cellos-RPi3 -d cellos-rpi3.bin cellos-rpi3.uImage`.
5. Prepare manual U-Boot commands for two probes:
   - raw probe: `setenv serverip 192.168.42.1; setenv ipaddr 192.168.42.2; tftp 0x80000 cellos-rpi3.bin; go 0x80000`
   - wrapped probe: `setenv serverip 192.168.42.1; setenv ipaddr 192.168.42.2; tftp 0x200000 cellos-rpi3.uImage; bootm 0x200000 - ${fdt_addr}`
6. If both fail due image-format validation, document the minimum Image-compatible wrapper needed before proceeding.
7. Select one boot script only after hardware logs prove DTB handoff and boot progress.

## Success Criteria

- [ ] U-Boot target/artifact is pinned with version, path, and SHA-256.
- [ ] Selected Cellos image format is proven on hardware, not guessed.
- [ ] Selected command passes DTB to Cellos or uses an approved shim/wrapper that does.
- [ ] Selected load/entry address is `0x80000` or explicitly justified by a wrapper that relocates/jumps there.

## Test Matrix

- Static: `readelf -h target/aarch64-unknown-none-softfloat/release/vicell-kernel`.
- Static: raw header inspection vs U-Boot `booti` requirements.
- Static: `mkimage -l cellos-rpi3.uImage`.
- Hardware manual: U-Boot UART `bdinfo`, `printenv fdt_addr`, raw `go`, wrapped `bootm`.
- Evidence: UART raw log plus server TFTP log for each candidate.

## Backwards Compatibility

No SD mutation beyond adding U-Boot test files after backup. Existing full SD local boot backup remains the rollback path.

## Risk Assessment

- High likelihood x High impact: raw `go` boots without DTB and breaks platform discovery. Mitigation: treat raw `go` as a probe only; do not make it default unless DTB handoff is proven or no DTB is needed.
- Medium likelihood x High impact: `bootm` rejects a freestanding kernel despite `uImage`. Mitigation: test before tooling; fall back to Image-compatible wrapper plan.
- Medium likelihood x Medium impact: U-Boot overwrites low memory near `0x80000`. Mitigation: verify relocation with `bdinfo` and choose separate TFTP container load address if using `bootm`.
- Rollback: remove U-Boot files from SD and restore the saved local boot partition. Irreversible part: none if backup exists.

## Security Considerations

Do not fetch boot scripts or firmware from TFTP in this phase. Only the Cellos candidate image is served.

## Deviation Log

None.
