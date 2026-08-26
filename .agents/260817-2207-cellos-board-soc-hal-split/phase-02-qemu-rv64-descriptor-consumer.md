---
phase: 2
title: "Consume QEMU RV64 Descriptor"
status: completed
priority: P1
effort: 1d
dependencies: [1]
tier: thinking
---

# Phase 2: Consume QEMU RV64 Descriptor

## Context Links

- Plan: `./plan.md`
- Evidence: `kernel/src/platform.rs:43-56`, `kernel/src/platform.rs:119-148`, `kernel/src/boot.rs:240-265`, `kernel/src/boot.rs:477-515`

## Overview

Routed QEMU RV64 boot/platform defaults through the validated descriptor while preserving current behavior. This is the first executable migration slice.

## Key Insights

- `PlatformInfo` is the existing runtime handoff for UART, PLIC, CLINT, VirtIO, and RTC data.
- RV64 already prefers DTB memory via `dtb_memory::build` before falling back to static data at `kernel/src/boot.rs:491-515`.
- UART init already reads `platform::with(|p| p.uart_base)` at `kernel/src/task/drivers/uart.rs:139-153`.

## Requirements

- Functional: QEMU RV64 platform defaults and fallback boot info come from the descriptor source of truth.
- Non-functional: no change to DTB-first behavior, log strings only when necessary, no AArch64/RPi3/SDHCI changes.

## Architecture

`kernel::board::selected()` validates the compiled descriptor before returning it. `platform.rs` converts descriptor device data into the existing `PlatformInfo`; `boot.rs` converts descriptor ranges into existing `MemoryMapEntry` constants. Existing consumers remain unchanged.

## Related Code Files

- Create: `kernel/src/board.rs`
- Modify: `kernel/src/main.rs`
- Modify: `kernel/src/platform.rs`
- Modify: `kernel/src/boot.rs`
- Modify: `kernel/Cargo.toml`

## Todo List

- [x] QEMU RV64 boots with DTB-present path.
- [x] QEMU RV64 still falls back to the same static memory map when DTB is absent or rejected.
- [x] No new generic driver checks board identity.

## Success Criteria

- [x] RV64 and AArch64 kernel cargo checks match the green baseline.
- [x] Unit tests prove descriptor validation and exact QEMU constants.
- [x] `scripts/qemu-boot-test.sh` passes with the rebuilt RV64 release kernel.
- [x] No `libs/api`, `libs/types`, `cells/drivers`, `hal/arch/arm`, MMC, or RPi3 linker file changes.

## Evidence

- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf` PASS.
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2` PASS.
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-pioneer` PASS.
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat` PASS.
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3` PASS.
- `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` PASS.
- `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel` PASS.

## Security Considerations

MMIO and RAM ranges are trusted build inputs. Validation must fail before publishing platform data or using fallback memory.

## Risk Notes

The highest risk is const-conversion drift in the fallback map. Revert the kernel consumer edits while retaining the isolated board crate if boot evidence fails.

## Next Steps

Phase 3 locked compatibility, docs, and handoff. `hal/soc` extraction remains deferred.

## Deviation Log

- `dtc` syntax validation was skipped because `dtc` was not installed; this is a tooling limitation, not a pass.
