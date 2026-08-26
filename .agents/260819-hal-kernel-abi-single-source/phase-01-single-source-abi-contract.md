---
phase: 1
title: "Single Source ABI Contract"
status: pending
priority: P1
effort: "1d"
dependencies: []
tier: thinking
---

# Phase 1: Single Source ABI Contract

## Overview

Finish the post-merge HAL↔kernel Rust ABI cleanup by making `hal-arch-trait` the only declaration surface and making both declaration-side and kernel-side drift fail at compile time.

## Requirements

- Functional: HAL arch code imports kernel hooks from `hal/traits/arch/src/kernel_abi.rs`; every in-scope kernel export has a compile-time type assertion; `docs/TODO.md` closes only after verification.
- Non-functional: no runtime behavior changes, no `libs/api/` ABI change, no board expansion, no physical RPi3 claim.

## Architecture

Data flow: trap/syscall/IRQ state enters arch HAL -> HAL constructs or forwards `ViTrapFrame`/`ViTrapFrame32` -> HAL calls centralized `extern "Rust"` hook from `hal-arch-trait` -> kernel export handles scheduler/syscall/fault/IRQ work -> validation proves target lanes still compile.

Dependency graph: reconcile dirty root edits first -> lock aliases and assertions -> run validation matrix -> update only ABI TODO. No new state is introduced; lifetime analysis is N/A because this is function-signature and doc work only.

Verified anchors:
- `hal/core/src/lib.rs:11` re-exports `hal_arch_trait`; `hal/core/src/lib.rs:21` exposes those aliases as `crate::hal::*`.
- `hal/traits/arch/src/kernel_abi.rs:8` defines `#[repr(C)] ViTrapFrame`; `hal/traits/arch/src/kernel_abi.rs:31` defines `#[repr(C)] ViTrapFrame32`.
- aliases live at `hal/traits/arch/src/kernel_abi.rs:55` through `hal/traits/arch/src/kernel_abi.rs:82`; centralized declarations live at `hal/traits/arch/src/kernel_abi.rs:84` through `hal/traits/arch/src/kernel_abi.rs:123`.
- x86_64 IDT imports hooks at `hal/arch/x86/src/x86_64/idt.rs:14` and calls page-fault hook at `hal/arch/x86/src/x86_64/idt.rs:203`.
- syscall bridge asserts dispatcher type in HAL glue at `hal/arch/x86/src/x86_64/syscall.rs:67`.
- kernel assertions already cover timer/current-cell/fault/syscall/IRQ hooks at `kernel/src/task.rs:459`, `kernel/src/task.rs:511`, `kernel/src/task.rs:524`, `kernel/src/task.rs:570`, `kernel/src/task/syscall.rs:5302`, `kernel/src/task/syscall.rs:5359`, `kernel/src/task/drivers/irq_dispatch.rs:7`, `kernel/src/task/drivers/irq_dispatch.rs:9`, `kernel/src/task/drivers/uart.rs:311`, `kernel/src/task/drivers/gpio_irq.rs:65`, and `kernel/src/task/drivers/virtio_common.rs:207`.

Callers to enumerate during build:
- `vi_timer_tick`: `hal/arch/riscv/src/rv64/trap.rs:78`, `hal/arch/riscv/src/rv64/trap.rs:87`, `hal/arch/riscv/src/rv32/trap.rs:50`, `hal/arch/riscv/src/rv32/trap.rs:56`, `hal/arch/arm/src/aarch64/trap.rs:310`, `hal/arch/arm/src/aarch64/trap.rs:330`, `hal/arch/arm/src/aarch64/trap.rs:376`, `hal/arch/x86/src/x86_64/idt.rs:162`.
- `ViCell_syscall_dispatch`: `hal/arch/arm/src/aarch64/trap.rs:44`, `hal/arch/riscv/src/rv64/trap.rs:173`, `hal/arch/riscv/src/rv32/trap.rs:95`, `hal/arch/x86/src/x86_64/syscall.rs:294`.
- `vi_handle_page_fault`: `hal/arch/x86/src/x86_64/idt.rs:203`.

## Assumptions

- **Claim:** local Rust toolchain has the listed target triples installed.
  **Confidence:** medium
  **How to verify:** `rustup target list --installed`.

## Related Files

- Modify: `hal/traits/arch/src/kernel_abi.rs`
- Modify: `kernel/src/memory/paging.rs`
- Modify: `scripts/check-hal-boundaries.sh`
- Modify: `docs/TODO.md`
- Read-only unless assertion gaps appear: `kernel/src/task.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/drivers/irq_dispatch.rs`, `kernel/src/task/drivers/uart.rs`, `kernel/src/task/drivers/gpio_irq.rs`, `kernel/src/task/drivers/virtio_common.rs`

## Implementation Steps

1. Preserve existing dirty `docs/TODO.md` and `kernel/src/memory/paging.rs` hunks; do not overwrite root/user edits.
2. Add declaration-side compile-time assertions in `hal/traits/arch/src/kernel_abi.rs`.
3. If unsafe extern item coercion fails, use explicit `unsafe extern "Rust" fn` aliases/assertions for unsafe hooks and keep `ViCell_syscall_dispatch` as the current `pub safe fn`.
4. Keep or add `#[cfg(target_arch = "x86_64")] const _: crate::hal::HandlePageFault = vi_handle_page_fault;` in `kernel/src/memory/paging.rs`.
5. Document `ViTrapFrame` as the shared syscall/dispatcher bridge; do not change frame fields.
6. Verify no non-central handwritten HAL declarations remain except comments.
7. Make `scripts/check-hal-boundaries.sh` reject future non-central HAL declarations.
8. Update only the ABI paragraph in `docs/TODO.md` after all validation passes; keep the RV32 baseline note intact.

## Test Matrix

- Unit/compile: `cargo check -p hal-arch-trait`; `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf --no-default-features --features riscv32`.
- Integration/compile: `RUSTFLAGS="-C relocation-model=pic" cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2`; `cargo check -p cellos-kernel --target x86_64-unknown-none`; `RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`.
- E2E/smoke: run one existing QEMU smoke for emulator-only evidence; do not claim physical RPi3 coverage.
- Boundary: `bash scripts/check-hal-boundaries.sh` plus grep for `extern "Rust"` under `hal/arch`.

## Success Criteria

- [ ] ABI hooks have one declaration source in `hal/traits/arch/src/kernel_abi.rs`.
- [ ] Declaration-side and kernel-side assertions compile for x86_64, AArch64, RV64, and HAL RV32 lanes.
- [ ] Known RV32 kernel `u32` vs `usize` failure remains documented if still present.
- [ ] `docs/TODO.md` closes only the ABI debt after validation.

## Security Considerations

Rust ABI mismatch can corrupt privileged trap/syscall state silently. This phase treats type assertions as a safety boundary and avoids widening public `libs/api/` contracts.

## Risk Assessment

- High likelihood x high impact: safe/unsafe extern mismatch breaks compile after adding declaration assertions. Mitigation: deliberately split safe dispatcher from unsafe hooks in aliases/assertions.
- Medium likelihood x high impact: TODO closure gets ahead of evidence. Mitigation: validation matrix gates the doc edit.
- Medium likelihood x medium impact: host lacks toolchain targets. Mitigation: report host-gated, do not mark ABI TODO closed.
- Rollback: revert edits in `hal/traits/arch/src/kernel_abi.rs`, `kernel/src/memory/paging.rs`, and the narrow `docs/TODO.md` ABI paragraph. Irreversible part: none.

## File Ownership

Single-phase plan owns `hal/traits/arch/src/kernel_abi.rs`, `kernel/src/memory/paging.rs`, `scripts/check-hal-boundaries.sh`, and the ABI paragraph in `docs/TODO.md`. It reads other assertion files only unless a missing assertion is verified.

## Deviation Log

None.
