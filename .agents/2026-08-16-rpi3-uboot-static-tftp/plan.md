---
title: "RPi3 U-Boot Static TFTP Plan"
description: "SD-local U-Boot bootstrap for RPi3B v1.2 with static PC-to-Pi TFTP fetching only the Cellos image."
status: pending
priority: P1
effort: 8h
branch: main
tags: [infra, hardware, aarch64, tools, docs]
blockedBy: []
blocks: []
created: 2026-08-16
---

# RPi3 U-Boot Static TFTP Plan

## Overview

Pivot from bootcode-only DHCP/TFTP to SD-local U-Boot. The SD remains the rollback anchor and holds Raspberry Pi firmware, U-Boot, config, and boot script. The PC keeps `SharedAccess`/WSL running, uses static host IP `192.168.42.1`, and serves only the Cellos image by TFTP to Pi static IP `192.168.42.2`; no OTP and no DHCP.

Primary U-Boot target: `rpi_3_defconfig` for Raspberry Pi 3B, artifact `u-boot.bin`; Debian package source path is `/usr/lib/u-boot/rpi_3/u-boot.bin` when `u-boot-rpi` is installed. Official U-Boot docs list `rpi_3_defconfig` for Raspberry Pi 3B, and Debian's file list shows the `rpi_3/u-boot.bin` artifact. Sources: https://docs.u-boot.org/en/v2026.04/board/broadcom/raspberrypi.html and https://packages.debian.org/sid/arm64/u-boot-rpi/filelist

## Phases

| Phase | Name | Status | Dependencies |
|---|---|---|---|
| 1 | [Prove U-Boot Handoff Compatibility](./phase-01-uboot-handoff-proof.md) | pending | none |
| 2 | [Build SD and Static TFTP Tooling](./phase-02-sd-uboot-tooling.md) | pending | 1 |
| 3 | [Validate Hardware and Document Rollback](./phase-03-hardware-docs-rollback.md) | pending | 2 |

## Dependencies

- Host network stays static-only: PC `192.168.42.1/24`, Pi `192.168.42.2/24`, no gateway, no DHCP service.
- Keep WSL and Windows `SharedAccess` running; bind TFTP to the physical `Ethernet` NIC IP only.
- Scope excludes OTP, DHCP, kernel NIC driver, NFS/rootfs, and permanent service installation.
- Active-plan sync not run: `.claude/scripts/set-active-plan.cjs` is absent in this checkout.

## File Ownership

- Phase 1 owns proof artifacts/logs under `.agents/debug/` only.
- Phase 2 owns future implementation files under `tools/rpi3-uboot-tftp/`.
- Phase 3 owns docs changes in `docs/baremetal/load-cellos.md` and `tools/rpi3-uboot-tftp/README.md`.

## Validation Log

- VERIFIED RPi3 linker origin and entry are `0x80000`: `kernel/linker-rpi3.ld:2`, `kernel/linker-rpi3.ld:11`.
- VERIFIED board-rpi3 feature selects `kernel/linker-rpi3.ld`: `kernel/build.rs:14`.
- VERIFIED current SD image flow emits raw `kernel8.img` by `objcopy -O binary`: `gen_disk_rpi3.ps1:117`, `gen_disk_rpi3.ps1:158`.
- VERIFIED current boot entry expects the firmware/U-Boot handoff value in `x0` as DTB and stashes it to `x19`: `hal/arch/arm/src/aarch64/boot.rs:37`.
- VERIFIED existing U-Boot/TFTP docs use a stale generic `0x40000000` example; RPi3 plan must use `0x80000` or a proven wrapper path: `docs/hardware-dev-guide.md:230`.

## Unresolved Questions

- Which boot command preserves the required DTB handoff for freestanding Cellos: raw `go`, legacy `uImage` + `bootm`, or an AArch64 `Image`-compatible wrapper. Phase 1 must decide before Phase 2 writes operator tooling.
