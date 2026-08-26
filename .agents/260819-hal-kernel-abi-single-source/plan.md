---
title: "HAL Kernel Rust ABI Single Source"
description: "Close HAL-to-kernel Rust ABI signature drift with one shared declaration surface and verified compile gates."
status: completed
priority: P2
effort: 1d
branch: refactor/hal-kernel-rust-abi
tags: [cellos, hal, kernel, rust-abi]
created: 2026-08-19
---

# HAL Kernel Rust ABI Single Source

## Overview

Finish the ABI signature cleanup now that RPi3 smoke testing is merged. Scope is only HAL/kernel Rust ABI declarations, kernel export assertions, validation, and narrow TODO closure; no board work, hardware claim, or `libs/api/` ABI change.

## Phases

| Phase | Name | Status | Effort |
|-------|------|--------|--------|
| 1 | [Single Source ABI Contract](./phase-01-single-source-abi-contract.md) | completed | 1d |

## Dependencies

- Blocks: none.
- Blocked by: none.
- Tooling note: `.claude/scripts/set-active-plan.cjs` is absent in this checkout, so active-plan sync could not be performed.

## Evidence

- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p hal-arch-trait'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target x86_64-unknown-none'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p hal-core --target riscv32imac-unknown-none-elf --no-default-features --features riscv32'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && bash scripts/check-hal-boundaries.sh'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && BOOT_WINDOW=30 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso'` reached `ViCell >` and timed out before the script's legacy `Cellos >` check; emulator-only evidence, not physical RPi3.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target riscv32imac-unknown-none-elf --no-default-features --features board-rpi3'` still fails on the pre-existing RV32 kernel baseline (`hal::paging`, `uart_bcm_mini`, `AtomicU64`, `u32` vs `usize`, `hal_soc_bcm27xx`).

## Reviewer

CLEAR

## Handoff

- Implementation command: `$hc-cook /home/dmin/cellos/.agents/260819-hal-kernel-abi-single-source/plan.md`
- Branch naming: keep `refactor/hal-kernel-rust-abi`; never create a `codex/` branch prefix.
