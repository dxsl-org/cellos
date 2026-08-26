---
title: "RISC-V PLIC Policy Plan"
description: "Move RV64 PLIC IRQ/context policy out of shared mechanism and into configured runtime data."
status: completed
priority: P2
effort: 9h
branch: fix/structure
tags: [refactor, hal, riscv, hardware]
blockedBy: []
blocks: []
created: 2026-08-18
---

# RISC-V PLIC Policy Plan

## Overview

This plan removes QEMU-shaped IRQ and PLIC-context assumptions from RV64 shared interrupt code while preserving `PlatformInfo`, `libs/api`, `libs/types`, root `boards/`, and single-copy shared drivers. Evidence is current-code OBSERVED unless marked PRIOR in phase assumptions; see [Scout Report](./reports/scout-report.md).

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Add PLIC Context Policy Data](./phase-01-plic-context-policy-data.md) | completed | none |
| 2 | [Configure PLIC From Runtime Platform Data](./phase-02-runtime-plic-configuration.md) | completed | 1 |
| 3 | [Route External IRQs And ACK By Runtime Data](./phase-03-runtime-irq-route-and-ack.md) | completed | 2 |

## Dependency Graph

Phase 1 creates data-only SoC context policy in `hal/soc/riscv`; Phase 2 consumes it plus existing `PlatformInfo` to configure shared PLIC mechanism; Phase 3 consumes that configured state and `PlatformInfo` lookups to remove fixed IRQ dispatch and RV64 VirtIO base arithmetic.

## Compatibility Strategy

Keep `PlatformInfo` fields unchanged (`uart_irq`, `virtio_mmio`, `plic_base`) and compute derived runtime state from them. Do not touch `libs/api/` or `libs/types/`, which require two explicit confirmations per `docs/code-standards.md:14`. Keep board descriptors in root `boards/` per `docs/system-architecture.md:52` and shared drivers in `cells/drivers/` per `docs/system-architecture.md:58`.

## Test Matrix

Baseline and final: `cargo fmt --all -- --check`; `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu`; `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu`; RV64 `cargo check` default, `--features board-vf2`, `--features board-pioneer`, and combined; AArch64 default and `board-rpi3` compile checks; RV64 release build; QEMU boot via `scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.

## Verification

- Final QA passed 11/11 gates against uncommitted source at `HEAD c6a31372`; QEMU boot passed, and VF2/Pioneer/RPi3 stayed compile-only.
- `docs/project-changelog.md` already records the 2026-08-18 runtime-data-driven PLIC entry; `docs/project-roadmap.md` now matches that status/date, and `docs/system-architecture.md` was already current.

## Deferred Risk

- JH7110 secondary-hart boot can still select physical hart0 before any S-mode PLIC context exists; current policy fails closed and makes no secondary external IRQ claim.

## Closure

Plan finalized; no further action.
