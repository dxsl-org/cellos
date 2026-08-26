# Test Report — 2026-08-11 — board-rpi3 hardware preflight

## Test Results Overview

- **Build**: PASS — `vicell-kernel` release build for `aarch64-unknown-none-softfloat` with `board-rpi3`
- **QEMU kernel-direct**: PASS to scheduler start under `qemu-system-aarch64 -machine raspi3b`; stopped by the 20-second harness timeout
- **Real Raspberry Pi 3**: HOST-GATED — board, LAN, power, and UART are not connected yet
- **SD-image boot**: NOT-PACKAGED — `disk_rpi3.img` and required firmware binaries are absent

## Artifact Evidence

- Kernel: `target/aarch64-unknown-none-softfloat/release/vicell-kernel`
- Size: 5,742,240 bytes
- SHA-256: `a97051d141a06fd576e71b01c300ba10b088ddc66903e3d5d8d9129c9d817f34`
- Git: `main` is clean and four commits ahead of `origin/main`

## QEMU Evidence

- Kernel reached paging, heap, HAL, scheduler, embedded init spawn, and `Starting scheduler...`.
- No SD card was attached, so MMC reported `NotFound`, the MBR and cell table were unavailable, and this run does not validate SD-image packaging or the real BCM2837 peripheral path.
- Build warnings are limited to unused board-specific timer/IRQ symbols and unavailable strip tooling.

## Host Preflight

- UART: only `COM1` is visible; no USB-to-TTL adapter is currently detected.
- Ethernet: Intel I226-V exists but is disconnected; WSL2 uses mirrored networking with firewall enabled.
- Removable media: no removable disk is currently visible. Only the two fixed host disks were enumerated; neither is an approved flash target.
- Missing tooling/artifacts: `parted`, `disk_rpi3.img`, `bootcode.bin`, `start.elf`, and `fixup.dat`.

## Packaging Risk

`gen_disk_rpi3.ps1` currently creates two FAT32 partitions and copies cell binaries as files into the second partition. The current kernel loader instead expects the canonical raw cell table at `PART_CELLTBL_BASE_LBA` (LBA 526,336). Treat the script as unverified against the current loader until an SD-image QEMU boot proves the final image layout.

## Recommended Gate Order

1. Confirm the exact board model (3B or 3B+) and connect USB-to-TTL plus direct Ethernet.
2. Package and QEMU-test a fresh SD image; do not flash before confirming the removable device by model, size, and device path.
3. Flash once for the baseline/recovery and SD/MMC evidence.
4. Add DHCP/TFTP as a kernel-iteration lane while retaining UART and the known-good SD card.

