---
phase: 3
title: "Route External IRQs And ACK By Runtime Data"
status: completed
priority: P2
effort: "4h"
dependencies: [2]
tier: thinking
---

# Phase 3: Route External IRQs And ACK By Runtime Data

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in Section Deviation Log the moment it occurs.

## Overview

Move RV64 external IRQ classification out of arch trap code and make unclaimed VirtIO ACK look up MMIO base by configured platform IRQ instead of QEMU arithmetic.

## Requirements

- Functional: Replace trap dispatch checks for `1..=8` and `10` in `hal/arch/riscv/src/rv64/trap.rs:105` with an internal kernel dispatcher or route helper.
- Functional: Make PLIC claim/complete use the configured context, not literal `1` at `hal/arch/riscv/src/rv64/trap.rs:181`.
- Functional: Replace the RV64 `0x1000_1000 + (irq - 1) * 0x1000` ACK formula at `kernel/src/task/drivers/virtio_common.rs:131` with a runtime lookup by IRQ.
- Non-functional: Do not use allocation in interrupt context; current `virtio_slots()` allocates a `Vec` on RV64 at `kernel/src/task/drivers/virtio_common.rs:46`, so the ACK lookup needs a direct `PlatformInfo` helper or equivalent.

## Architecture

Data flow: PLIC claim returns an IRQ through configured context, arch trap forwards the IRQ to one kernel-owned dispatcher, the dispatcher checks `PlatformInfo.uart_irq` and `PlatformInfo.virtio_mmio`, then invokes UART or VirtIO handlers. The unclaimed ACK path receives an IRQ and asks the same runtime map for its VirtIO base before touching `InterruptStatus`.

## Related Files

- Modify: `hal/arch/riscv/src/rv64/trap.rs`
- Modify: `hal/arch/riscv/src/common/plic.rs`
- Modify: `kernel/src/platform.rs`
- Modify: `kernel/src/task/drivers/virtio_common.rs`
- Optional create: `kernel/src/task/drivers/irq_dispatch.rs`
- Modify docs after verification only: `docs/system-architecture.md`, `docs/project-changelog.md`, `docs/project-roadmap.md`

## Implementation Steps

1. In arch trap, replace direct UART/VirtIO externs with a narrow `extern "Rust"` dispatcher such as `vi_handle_riscv_external_irq(irq) -> bool`.
2. Keep the PLIC complete-after-handler contract from `hal/arch/riscv/src/rv64/trap.rs:104`.
3. Implement the kernel dispatcher using `platform::with` against `uart_irq` and `virtio_mmio`; unknown IRQs should log and return false.
4. Add a direct RV64 helper like `virtio_mmio_base_for_irq(irq)` that scans `PlatformInfo.virtio_mmio` without constructing `Vec`.
5. Update `ack_unclaimed(irq)` to use that helper on RV64 while preserving the existing AArch64 branch unless a no-allocation shared helper is proven.
6. Update docs only after compile and QEMU evidence, preserving QEMU-versus-hardware wording.

## Success Criteria

- [x] `hal/arch/riscv/src/rv64/trap.rs` contains no active QEMU IRQ range or UART IRQ literal.
- [x] `kernel/src/task/drivers/virtio_common.rs` contains no RV64 QEMU base-from-IRQ formula.
- [x] Baseline/final test matrix from `plan.md` passes, including QEMU boot.
- [x] Documentation states QEMU runtime evidence only; VF2/Pioneer remain compile/hardware-gated unless real logs exist.

## Evidence

- Final QA report `qa-2026-08-18-final.md` records `cargo fmt --all -- --check` and the full kernel matrix as passed, ending with QEMU boot `PASS: FAT16 mounted — kernel booted (no disk)`.
- The same report keeps VF2, Pioneer, and RPi3 honest as compile-only lanes while QEMU runtime remains the only boot evidence.

## Security Considerations

Interrupt context must avoid allocation and locks that can deadlock. The current `irq_wait` path is atomic-only by design at `kernel/src/task/drivers/irq_wait.rs:110`, so new dispatch code must preserve that ISR contract.

## Risk Notes

- Risk: high likelihood x high impact, accidental allocation in ISR through `virtio_slots()`. Mitigation: direct `PlatformInfo` scan helper; grep for `virtio_slots()` in IRQ-only paths before final.
- Risk: medium likelihood x medium impact, unknown IRQ logging inside trap path becomes noisy. Mitigation: log once or at warning level only after deciding current logging policy.
- Risk: low likelihood x high impact, function symbol mismatch across `extern "Rust"` boundary. Mitigation: compile all RV64 feature combinations.
- Rollback: restore trap dispatch and RV64 ACK formula from the pre-plan commit; docs rollback is the matching docs commit. Irreversible part: none.

## Assumptions

- Claim: A single kernel dispatcher can replace the two direct extern calls without changing public ABI.
  Confidence: high
  How to verify: grep confirms current calls are internal externs in `hal/arch/riscv/src/rv64/trap.rs:207`, not `libs/api` or `libs/types`.

## Deviation Log

- 2026-08-18 Decision: unknown RV64 external IRQ warnings are globally bounded with a small atomic counter instead of per-IRQ tracking so the dispatcher stays allocation-free and lock-free in interrupt-adjacent paths.
