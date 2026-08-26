---
phase: 2
title: "Configure PLIC From Runtime Platform Data"
status: completed
priority: P2
effort: "3h"
dependencies: [1]
tier: thinking
---

# Phase 2: Configure PLIC From Runtime Platform Data

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in Section Deviation Log the moment it occurs.

## Overview

Replace hardcoded `context 1`, `IRQs 1..=8`, and `IRQ 10` inside `hal/arch/riscv/src/common/plic.rs` with runtime configuration computed by the kernel from existing `PlatformInfo`.

## Requirements

- Functional: Add a small `hal::common::plic` configuration API that stores active S-mode context and enabled IRQs before `plic::init()`.
- Functional: Compute enabled IRQs from `PlatformInfo.uart_irq` and `PlatformInfo.virtio_mmio` without adding fields to `PlatformInfo`, whose current shape is at `kernel/src/platform.rs:27`.
- Functional: Update both boot-time and restore-time callers; `kernel/src/main.rs:541` and `kernel/src/snapshot.rs:332` call `plic::init()`.
- Non-functional: Missing config must fail closed by enabling no device IRQs rather than reintroducing QEMU defaults.

## Architecture

Data flow: `platform::init` publishes immutable `PlatformInfo` at `kernel/src/platform.rs:83`, `main` already sets PLIC base from `p.plic_base` before driver init at `kernel/src/main.rs:107`, then a new kernel helper supplies context and IRQ list to `plic::configure`. `plic::init()` reads only configured runtime state, sets threshold for that context, and enables the listed IRQs.

## Related Files

- Modify: `hal/arch/riscv/src/common/plic.rs`
- Modify: `kernel/src/platform.rs`
- Modify: `kernel/src/main.rs`
- Modify: `kernel/src/snapshot.rs`

## Implementation Steps

1. Add a bounded no-alloc PLIC runtime config in `hal/arch/riscv/src/common/plic.rs`, using fixed storage sized for current `PlatformInfo.virtio_mmio` plus UART.
2. Keep `set_plic_base(base)` for existing boot sequencing; add `configure(context, irqs)` or `configure_runtime(config)` as a separate call.
3. Change `plic::init()` so it loops configured IRQs instead of `1..=8` and `10`, removing the QEMU/JH7110 comments at `hal/arch/riscv/src/common/plic.rs:90`.
4. Add a RV64-only `kernel/src/platform.rs` helper that gathers nonzero `uart_irq` and every nonzero `VirtioEntry.irq` from `PlatformInfo`.
5. Call that helper before `plic::init()` in both `kernel/src/main.rs` and `kernel/src/snapshot.rs`.
6. Preserve `PlatformInfo` shape; do not add fields beyond the current `uart_irq`, `plic_base`, and `virtio_mmio` fields at `kernel/src/platform.rs:29`.

## Success Criteria

- [x] `hal/arch/riscv/src/common/plic.rs` no longer contains `1..=8`, `IRQ 10`, or fixed `Context 1` as active policy.
- [x] `kernel/src/main.rs` and `kernel/src/snapshot.rs` both configure PLIC before init.
- [x] RV64 default, VF2, Pioneer, and combined feature `cargo check` pass.

## Evidence

- Final QA report `qa-2026-08-18-final.md` records RV64 kernel checks for default, `board-vf2`, `board-pioneer`, and combined `board-vf2 board-pioneer` features as passed.
- The same report records AArch64 default and `board-rpi3` checks, the RV64 release build with `-Z build-std=core,alloc`, and QEMU boot `PASS: FAT16 mounted — kernel booted (no disk)`.

## Security Considerations

No new unsafe MMIO surface beyond existing PLIC register writes. Any new unsafe remains limited to existing MMIO operations and keeps `// SAFETY:` comments per `docs/code-standards.md:65`.

## Risk Notes

- Risk: medium likelihood x high impact, snapshot restore loses runtime config and fails to re-enable IRQs. Mitigation: update `snapshot.rs` caller explicitly and add a grep gate for all `plic::init()` callers.
- Risk: low likelihood x medium impact, duplicate IRQs cause redundant enable writes. Mitigation: de-duplicate or accept idempotent MMIO enable writes; document chosen behavior.
- Rollback: restore prior `plic.rs`, `main.rs`, `snapshot.rs`, and `platform.rs` from the previous commit. Irreversible part: none.

## Assumptions

- Claim: Rewriting `plic::init()` to consume fixed-size runtime state will not require heap allocation.
  Confidence: high
  How to verify: implement with stack arrays or static atomics; check `hal/arch/riscv` remains `no_std`.

## Deviation Log

- 2026-08-18 Decision: collapsed the planned `configure(...)` plus later `init()` split into `plic::init(context, irqs)` so the shared mechanism only stores the active context needed by `claim/complete`; the runtime IRQ slice stays caller-owned and no static IRQ table is introduced.
