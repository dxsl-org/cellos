---
phase: 4
title: "Boot Storage Input Display Baseline"
status: completed
priority: P1
effort: "6d"
dependencies: [2]
tier: thinking
---

# Phase 04: Boot Storage Input Display Baseline

## Context Links

- RPi3 descriptor: `boards/raspberry-pi/3-model-b/board.rs:7-14`.
- SDHCI controller: `kernel/src/task/drivers/mmc/sdhci.rs:8-18`, `kernel/src/task/drivers/mmc/sdhci.rs:155-213`.
- x86/RV64 driver init: `kernel/src/main.rs:592-638`.

## Overview

Stabilize boot-critical storage, input, console, and display fallback so G1 and G2 driver work has a repeatable baseline. RV64 and AArch64 live QEMU baselines pass with the normal device set and with optional GPU/NIC omitted. Physical RPi3 TFTP boot, SDHCI discovery, partition reads, mounts, input-service startup, shell readiness, interactive `help`, and a lossless 100-command UART burst pass.

## Requirements

- Functional: UART console/input, SDHCI/MMC, VirtIO block/net/input/GPU fallback, framebuffer console where available.
- Non-functional: boot path must fail closed when board lacks a driver; no kernel panic on absent optional devices.

## Architecture

Data flow: boot firmware/DTB/fallback map -> platform storage/console drivers -> early loader/VFS -> Driver Cell registration -> services and shell consume block/input/GPU/NIC capabilities.

## Related Code Files

- Modify: `kernel/src/task/drivers/{mmc,uart,virtio_common,virtio_hal,virtio_rng,console_drv,input_irq_ack}.rs`.
- Modify: `cells/drivers/{virtio-blk,virtio-net,virtio-gpu,serial,gpu,input}/` if current cells need promotion.
- Scripts: `scripts/build-boot-ramdisk-ci.sh`, `scripts/build-srv-test-ci.sh`, `scripts/build-test-hooks-ci.sh`, `scripts/qemu-*.sh`, `scripts/assert-boot-markers.sh`.

## Implementation Steps

1. Audit which boot drivers remain kernel-resident versus Driver Cell owned.
2. Keep SDHCI PIO polling bounded; do not add DMA before DMA/IOMMU gates are ready.
3. Ensure VirtIO Driver Cells skip claimed kernel boot devices instead of resetting them.
4. Preserve UART-first diagnostics for physical RPi3 and COM1 x86.
5. Add boot markers for block registration, input registration, and display readiness.

## Todo List

- [x] Boot without optional GPU/NIC does not panic on live RV64 or AArch64 QEMU.
- [x] SDHCI card-present/read/mount markers separated from QEMU VirtIO.
- [x] Input path tested via VirtIO/QEMU separately.
- [x] Interactive input path tested via UART/RPi3; kernel-push service startup passes.

## Success Criteria

- [x] RV64 QEMU boot reaches prompt with block/input/GPU registration markers.
- [x] AArch64 QEMU reaches prompt with VirtIO block/input/GPU registration markers.
- [x] RPi3 physical SDHCI read/mount is PASS/FAIL/BLOCKED, not inferred from compile.

## Test Matrix

- Unit: SDHCI policy and VirtIO slot filters.
- Integration: `scripts/qemu-boot-test.sh`, `scripts/qemu-aarch64-test.sh`, build ramdisk scripts.
- E2E: RPi3 physical boot, UART burst, SDHCI mount, reboot; x86 q35 boot for G2 baseline.

## Risk Assessment

| Risk | LxI | Mitigation |
|---|---|---|
| Driver Cell resets kernel-owned boot disk | MxH | ownership probe and skip path before transport init. |
| SDHCI hangs boot | MxH | bounded polling and fail-open to ramdisk only where documented. |
| QEMU masks physical timing bugs | HxM | require physical board ledger before G1 promotion. |

## Security Considerations

No DMA storage path without Phase 05 IOMMU gate; MMIO windows stay SoC-bounded.

## Backward Compatibility

Keep existing boot images and `/bin` paths; any replacement Driver Cell must preserve VFS block wire format.

## File Ownership

Owns boot/storage/input/display baseline files; coordinates with Phase 03 before touching UART/GPIO IRQ files.

## Rollback

Revert to previous kernel-resident fallback drivers and old boot image scripts. Irreversible part: physical media writes, mitigated by explicit target confirmation and readback.

## Assumptions

None -- cited paths plus QEMU and physical RPi3 runtime behavior were verified.

## Deviation Log

- 2026-08-19: boot evidence scripts were added to separate marker checks from the actual QEMU/physical runs; they are not promotion evidence by themselves.
- 2026-08-19: live RV64 and AArch64 QEMU runs passed both the full baseline and optional GPU/NIC omission gate. Retained logs are under `reports/evidence/phase04-*-baseline.log` and `phase04-*-without-optional.log`.
- 2026-08-19: the current-head RPi3 image was built, wrapped, and staged to the TFTP root after payload verification. Physical execution was initially BLOCKED because the host Ethernet link was disconnected and `192.168.42.2` was unreachable; no SD or OTP mutation was performed.
- 2026-08-19: after the board was powered, the direct link reached 100 Mbps and TFTP delivered the verified payload. A 150-second COM4 capture proved SD card discovery, MBR P1-P4 reads, FAT16 plus `/mnt/sd` mounts, kernel-push Input Service startup, and shell readiness. Interactive UART RX remains pending because adapter TX stayed disconnected. Evidence: `reports/evidence/phase04-rpi3-physical-boot-20260819.md`.
- 2026-08-19: after shell readiness, adapter TX was connected and `help` returned the command list plus prompt. A numbered 100-command UART burst returned 100/100 unique responses with no missing IDs. Phase 04 is complete.

## Next Steps

Phase 05 consumes a stable boot/storage baseline for real PCIe storage/network.
