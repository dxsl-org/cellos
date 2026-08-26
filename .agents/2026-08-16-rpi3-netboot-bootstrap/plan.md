---
title: "RPi3 Netboot Bootstrap Plan"
description: "Bootcode.bin-only SD bootstrap plus Windows-native direct-Ethernet DHCP/TFTP for fast board-rpi3 kernel iteration without OTP."
status: cancelled
priority: P1
effort: 7h
branch: main
tags: [infra, hardware, aarch64, tools, docs]
blockedBy: []
blocks: []
created: 2026-08-16
---

# RPi3 Netboot Bootstrap Plan

## Superseded

Cancelled by `.agents/2026-08-16-rpi3-uboot-static-tftp/plan.md`. The active direction is SD-local U-Boot with static TFTP for only the Cellos image; DHCP/TFTP bootcode-only bootstrap is no longer the next plan.

## Overview

Goal: keep the RPi3B v1.2 recoverable from SD while adding a no-OTP network boot lane over direct PC Ethernet. The SD card will contain only `bootcode.bin`; the Windows host serves DHCP/TFTP from a local root containing `bootcode.bin`, `start.elf`, `fixup.dat`, `config.txt`, and `kernel8.img`.

Official evidence: Raspberry Pi documents special `bootcode.bin`-only mode for BCM2837/Pi3B and says the SD remains present while boot continues via USB host/Ethernet; the Raspberry Pi 2016 Ethernet article describes Pi3 ROM DHCP/TFTP and newer `bootcode.bin` fetching config, firmware, and kernel into RAM. Sources: https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#special-bootcodebin-only-boot-mode and https://www.raspberrypi.com/news/pi-3-booting-part-ii-ethernet-all-the-awesome/

## Phases

| Phase | Name | Status | Dependencies |
|---|---|---|---|
| 1 | [Build Recoverable Netboot Tooling](./phase-01-netboot-tooling.md) | pending | none |
| 2 | [Validate Direct Ethernet Boot Lane](./phase-02-netboot-validation.md) | pending | 1 |
| 3 | [Document Operator Workflow](./phase-03-docs-and-rollback.md) | pending | 2 |

## Dependencies

- Windows physical NIC target: alias `Ethernet`, ifIndex `14`, Intel I226-V, currently disconnected/APIPA; Wi-Fi remains internet.
- Network services: UDP 67 occupied only on `172.23.96.1`; UDP 69 currently free. Tooling must bind explicitly to the physical NIC static IP, not WSL mirrored `eth1`.
- Scope excludes OTP programming, kernel NIC driver work, Linux-root/NFS, and permanent network infrastructure.
- Active-plan sync not run: `.claude/scripts/set-active-plan.cjs` is absent in this checkout.

## File Ownership

- Phase 1 owns `tools/rpi3-netboot/`.
- Phase 2 owns generated logs/backups under ignored `tools/rpi3-netboot/{root,backups,logs,state}/`.
- Phase 3 owns `docs/baremetal/load-cellos.md` and optional `tools/rpi3-netboot/README.md` refinements.

## Validation Log

- VERIFIED current SD-image path copies required boot files to FAT boot partition: `gen_disk_rpi3.ps1:153`.
- VERIFIED repo firmware instructions identify `bootcode.bin`, `start.elf`, and `fixup.dat`: `tools/rpi3-firmware/README.txt:4`.
- VERIFIED current board config expects `kernel8.img` and UART: `tools/rpi3-firmware/config.txt:10`.
- VERIFIED current bare-metal doc only describes flash-image SD boot and should gain netboot lane: `docs/baremetal/load-cellos.md:10`.
- PRIOR session recon says UDP 67 conflict is on `172.23.96.1` only and UDP 69 is free; implementation must re-check before binding.

## Unresolved Questions

- Whether Windows firewall prompts can be handled non-interactively on this host. Plan requires an admin boundary: prompt/report before adding or removing firewall/static-IP state.
