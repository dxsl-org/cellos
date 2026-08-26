---
phase: 2
title: "Consume Profiles in RV64 Platform"
status: completed
priority: P2
effort: "0.5d"
dependencies: [1]
tier: thinking
---

# Phase 2: Consume Profiles in RV64 Platform

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in this file when it occurs.

## Overview

Wire the RV64 platform discovery path to the new profile crate while preserving the current `PlatformInfo` public shape and all existing call sites.

## Requirements

- Functional: default RV64 uses generic QEMU virt profile; `board-vf2` uses JH7110; `board-pioneer` uses SG2042.
- Functional: Pioneer still forces SBI DBCN console, disables RTC MMIO, and hides VirtIO.
- Non-functional: no behavior change for AArch64, x86, RPi3, board descriptors, or shared drivers.

## Architecture

Data enters `platform::init(sbi_dtb)` at `kernel/src/platform.rs:81`. `active_riscv_soc_profile()` selects one static profile from Cargo features. DTB parser reads compatible arrays from that profile, transforms discovered nodes into the existing `PlatformInfo` at `kernel/src/platform.rs:236`, then applies access policy before publishing via `publish_boot` at `kernel/src/platform.rs:69`.

Existing consumers remain:

- PLIC base: `kernel/src/main.rs:109`
- MMIO mapping: `kernel/src/memory/paging.rs:184`
- VirtIO slots: `kernel/src/task/drivers/virtio_common.rs:46`
- UART base: `kernel/src/task/drivers/uart.rs:143`

## Assumptions

None - no unverified claims.

## Related Files

- Modify: `kernel/Cargo.toml`
- Modify: `kernel/src/platform.rs`
- Modify only if compile requires: `hal/soc/riscv/src/lib.rs`

## File Ownership

This phase owns `kernel/Cargo.toml` and `kernel/src/platform.rs`. It must not edit `kernel/src/boot.rs`; VF2 fallback memory remains at `kernel/src/boot.rs:291`.

## Implementation Steps

1. Add `hal-soc-riscv` only under `kernel/Cargo.toml:33` target `cfg(target_arch = "riscv64")` dependencies.
2. Add a private `#[cfg(target_arch = "riscv64")] fn active_riscv_soc_profile() -> &'static RiscvSocProfile` in `kernel/src/platform.rs`.
3. Select `SG2042` for `board-pioneer`, `JH7110` for `board-vf2`, and `GENERIC_VIRT` otherwise.
4. Replace hard-coded compatible arrays in `from_dtb` with profile arrays while keeping `reg_base`, `reg_base_size`, `irq_first`, and `collect_virtio` local to the kernel.
5. Replace the current `#[cfg(feature = "board-pioneer")]` mutation block at `kernel/src/platform.rs:91` with a profile-policy application helper that maps `SbiDbcnOnly`/`Unavailable`/`Absent` to the same `PlatformInfo` values.
6. Keep `PlatformInfo` fields unchanged; if a compile error suggests adding fields, stop and record a deviation before widening scope.
7. Keep PLIC IRQ enable/dispatch untouched in `hal/arch/riscv/src/common/plic.rs:92` and `hal/arch/riscv/src/rv64/trap.rs:103`.

## Success Criteria

- [x] `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` passes.
- [x] `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc --features board-vf2` passes.
- [x] `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc --features board-pioneer` passes.
- [x] Grep confirms no new board-specific driver files under `cells/drivers/`.

## Security Considerations

SG2042's sv39-inaccessible MMIO must remain fail-closed: UART and RTC bases stay zero, and VirtIO slots stay absent. Do not identity-map the high SG2042 UART/RTC addresses.

## Risk Notes

- Risk: precedence bug when multiple board features are enabled. Mitigation: explicit selector order and a unit test or compile-time guard documenting `board-pioneer` versus `board-vf2` precedence.
- Risk: DTB parser loses T-Head compatible support. Mitigation: preserve `thead,c900-plic` and `thead,c900-clint` in profile tests.
- Rollback: restore hard-coded arrays and the Pioneer mutation block in `kernel/src/platform.rs`, then remove the `hal-soc-riscv` dependency.

## Deviation Log

- Decision 2026-08-18: `active_riscv_soc_profile()` gives `board-pioneer` precedence over `board-vf2` when both features are present so the existing Pioneer fail-closed platform policy remains intact without introducing a new compile-time incompatibility in this phase.
- Evidence 2026-08-18: `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc --features board-vf2,board-pioneer` passed after switching selector branches to fully-qualified `hal_soc_riscv::*` constants, confirming combined-feature builds still resolve to Pioneer precedence without unused-import warnings.
- Decision 2026-08-18: valid-DTB builds now honor `VirtioMmioPolicy::Absent` before traversal via `virtio_mmio_entries_for_profile()`, while the existing `apply_riscv_soc_access_policy()` remains in place so `dtb_ptr == 0` and DTB-parse-failure fallback paths still clear SG2042 slots fail-closed.
