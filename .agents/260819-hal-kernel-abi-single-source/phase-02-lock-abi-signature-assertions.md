---
phase: 2
title: "Lock ABI Signature Assertions"
status: completed
priority: P1
effort: "4h"
dependencies: [1]
tier: thinking
---

# Phase 2: Lock ABI Signature Assertions

## Overview

Make signature drift fail at compile time on both sides of the HAL/kernel Rust ABI. Keep one declaration surface in `hal-arch-trait` and assert every kernel export against the same alias.

## Requirements

- Functional: HAL trap/syscall code imports ABI hooks only from `hal_arch_trait`; kernel exports compile-check against `crate::hal::*` aliases; declaration crate also checks declared items against aliases.
- Non-functional: keep `repr(C)` frame layout, no symbol renames, no new runtime state, no `libs/api/` ABI change.

## Architecture

Data flow: CPU trap/syscall frame enters arch HAL -> HAL fills or forwards `ViTrapFrame`/`ViTrapFrame32` -> calls centralized `extern "Rust"` hook from `hal/traits/arch/src/kernel_abi.rs:84` -> kernel export handles task, syscall, IRQ, or page-fault outcome -> returns to HAL where applicable.

Dependency graph: Phase 1 baseline -> this phase type-locks aliases and exports -> Phase 3 validates all target lanes. No state lifetime changes are planned; all work is function signatures, type aliases, and documentation.

Observed evidence:
- `hal/core/src/lib.rs:11` re-exports `hal_arch_trait` through `traits`, and `hal/core/src/lib.rs:21` re-exports traits to kernel `crate::hal::*`.
- `ViTrapFrame` is `#[repr(C)]` at `hal/traits/arch/src/kernel_abi.rs:8`; RV32-specific `ViTrapFrame32` is `#[repr(C)]` at `hal/traits/arch/src/kernel_abi.rs:31`.
- Central aliases exist at `hal/traits/arch/src/kernel_abi.rs:55` through `hal/traits/arch/src/kernel_abi.rs:82`; extern declarations exist at `hal/traits/arch/src/kernel_abi.rs:84` through `hal/traits/arch/src/kernel_abi.rs:123`.
- x86_64 IDT imports centralized hooks at `hal/arch/x86/src/x86_64/idt.rs:14` and calls page fault hook at `hal/arch/x86/src/x86_64/idt.rs:203`.
- x86_64 syscall glue already asserts dispatcher type at `hal/arch/x86/src/x86_64/syscall.rs:67`.
- Kernel export assertions already exist for `TerminateOnFault` at `kernel/src/task.rs:459`, `TerminateOnFaultAarch64` at `kernel/src/task.rs:511`, `CurrentCellId` at `kernel/src/task.rs:524`, `TimerTick` at `kernel/src/task.rs:570`, syscall dispatch at `kernel/src/task/syscall.rs:5302` and `kernel/src/task/syscall.rs:5359`, UART IRQ at `kernel/src/task/drivers/uart.rs:311`, GPIO IRQ at `kernel/src/task/drivers/gpio_irq.rs:65`, VirtIO IRQ at `kernel/src/task/drivers/virtio_common.rs:207`, and RISC-V IRQ hooks at `kernel/src/task/drivers/irq_dispatch.rs:7`.

## Assumptions

None - no unverified claims.

## Related Files

- Modify: `hal/traits/arch/src/kernel_abi.rs`
- Modify: `kernel/src/memory/paging.rs`
- Modify: `kernel/src/task.rs`
- Modify: `kernel/src/task/syscall.rs`
- Modify: `kernel/src/task/drivers/irq_dispatch.rs`
- Modify: `kernel/src/task/drivers/uart.rs`
- Modify: `kernel/src/task/drivers/virtio_common.rs`
- Modify: `kernel/src/task/drivers/gpio_irq.rs`

## Implementation Steps

1. In `hal/traits/arch/src/kernel_abi.rs`, add declaration-side compile-time assertions tying each declared hook item to its public alias.
2. If rustc treats non-`safe` extern items as unsafe function items, change the affected aliases/assertions deliberately to `unsafe extern "Rust" fn` while keeping `ViCell_syscall_dispatch` as the existing safe extern item.
3. Keep `ViTrapFrame`/`ViTrapFrame32` layout assertions and document `ViTrapFrame` as the syscall/dispatcher bridge shared by RV64, x86_64, and the ARM64 bridge.
4. Keep or add kernel-side assertion for `vi_handle_page_fault` against `crate::hal::HandlePageFault`.
5. Re-run grep for `extern "Rust"` under `hal/arch`; only comments should remain outside `hal/traits/arch/src/kernel_abi.rs`.

## Success Criteria

- [x] Every HAL-consumed kernel hook has exactly one declaration in `hal/traits/arch/src/kernel_abi.rs`.
- [x] Every kernel-side `#[no_mangle] pub extern "Rust" fn vi_*` / `ViCell_syscall_dispatch` in scope has a `const _:` assertion against `crate::hal::*`.
- [x] Declaration-side assertions compile for x86_64, aarch64, riscv64, and riscv32 HAL lanes.
- [x] No symbol name or public `libs/api/` ABI changes.

## Evidence

- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p hal-arch-trait'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target x86_64-unknown-none'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p hal-core --target riscv32imac-unknown-none-elf --no-default-features --features riscv32'`
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && grep -RInE \"extern \\\"Rust\\\"\" hal/arch --include=\"*.rs\" || true'` only found comment mentions outside `hal/traits/arch/src/kernel_abi.rs`.

## Reviewer

CLEAR

## Security Considerations

Rust ABI hooks cross a privileged HAL/kernel boundary. Signature mismatch can corrupt trap frames or IRQ state silently, so the compile-time assertions are a safety boundary, not style cleanup.

## Risk Assessment

- High likelihood x high impact: safe/unsafe function item mismatch causes false compile failures after adding declaration assertions. Mitigation: use explicit `unsafe extern "Rust" fn` aliases for unsafe extern declarations and keep only proven-safe declarations as `pub safe fn`.
- Medium likelihood x high impact: changing `ViTrapFrame` breaks trap/syscall frame layout. Mitigation: no field changes; retain `repr(C)` and size assertions.
- Rollback: revert `hal/traits/arch/src/kernel_abi.rs` assertion/alias edits and `kernel/src/memory/paging.rs` assertion hunk. Irreversible part: none.

## File Ownership

- Phase 2 owns all ABI declaration and kernel export assertion files listed above.
- Phase 3 may read but must not edit these files until Phase 2 passes compile gates.

## Deviation Log

None.
