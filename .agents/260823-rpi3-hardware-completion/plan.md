---
title: "RPi3 Hardware Completion"
description: "Five RPi3 enhancement phases: semihosting fix, HDMI framebuffer, persistent SD storage, G1 peripheral drivers, and LAN9514 Ethernet."
status: in-progress
priority: P2
effort: 26d
branch: main
tags: [hardware, rpi3, aarch64, embedded]
blockedBy: []
blocks: []
created: 2026-08-23
---

# RPi3 Hardware Completion

## Overview

Complete five RPi3-facing enhancements of increasing complexity. Phases 1-3
can execute in parallel (no dependencies). Phase 4 depends on physical sensors.
Phase 5 is the largest and may be deferred or staged independently.

## Contract

All work targets the physical RPi3 Model B (BCM2837, AArch64). QEMU `raspi3b`
may supplement but does not replace physical board evidence. No Tier 2
qualification claim, Manifest v3, or ledger promotion is authorized.

## Phases

| Phase | Name | Status | Effort | Depends |
|-------|------|--------|--------|---------|
| 1 | [AArch64 Test-Hooks Semihosting](./phase-01-aarch64-semihosting.md) | completed | 4h | — |
| 2 | [Persistent SD Storage](./phase-02-persistent-sd-storage.md) | completed | 2d | — |
| 3 | [G1 Peripheral Sensor Drivers](./phase-03-g1-peripheral-drivers.md) | in-progress | 3d | — |
| 4 | [HDMI Framebuffer via VideoCore Mailbox](./phase-04-hdmi-framebuffer.md) | completed | 5d | — |
| 5 | [LAN9514 Ethernet via DWC2 USB Host](./phase-05-lan9514-ethernet.md) | blocked (5a steps 1–4 done; 5–6 await policy-v3 USB authority + one-shot IRQ contract) | 15d | — |

## Parallel Execution

Phases 1, 2, and 3 are independent and can execute concurrently.
Phase 4 touches compositor/display subsystem independently of 1-3.
Phase 5 is standalone but very large; consider staging as a separate plan.

## Dependencies

- Physical RPi3 Model B v1.2 with SD card, HDMI display, USB Ethernet.
- Physical I2C/SPI sensors for Phase 3 (MPU6050 or BNO055; SSD1306 or ST7789).
- BCM2835/BCM2837 ARM Peripherals datasheet for Phases 4 and 5.
