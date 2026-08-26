---
phase: 1
title: "Normalize q35 x86_64 Identity"
status: completed
priority: P1
effort: "2h"
dependencies: []
tier: medium
---

# Phase 1: Normalize q35 x86_64 Identity

## Overview

Finalize the current x86 board as a QEMU q35 x86_64 board, not a generic PC board, while preserving existing HAL mechanisms and fail-closed firmware behavior.

## Requirements

- Functional: replace `boards/generic/x86_64-pc` with `boards/qemu/q35-x86_64`; expose `QEMU_Q35_X86_64`; use slug `qemu-q35-x86_64`; use `SocId::QemuX86Q35`; pair kernel x86 selection with `hal_soc_x86::QEMU_Q35`.
- Non-functional: no changes to `hal/arch/x86` mechanism ownership; no new fallback LAPIC, IOAPIC, HPET, or ECAM addresses; no duplicated drivers.

## Architecture

Data enters as `BoardDescriptor` in `boards/qemu/q35-x86_64/board.rs`, flows through `boards/src/lib.rs`, then `kernel/src/board.rs`, and finally selects static SoC facts from `hal/soc/x86`. Runtime-discovered ACPI values remain dynamic and fail-closed. Current evidence for q35 comes from `scripts/qemu-x86_64-test.sh:39` and `scripts/qemu-x86_64-test.sh:40`.

## Assumptions

- **Claim:** Display vendor should become `QEMU` while compatible remains `qemu,q35`.
  **Confidence:** medium
  **How to verify:** compare existing board vendor naming and decide whether `BoardDescriptor.vendor` is display text or canonical machine token.

## Related Files

- Delete: `boards/generic/x86_64-pc/README.md`
- Delete: `boards/generic/x86_64-pc/board.rs`
- Modify: `boards/qemu/q35-x86_64/board.rs`
- Modify: `boards/src/descriptor.rs`
- Modify: `boards/src/lib.rs`
- Modify: `boards/src/catalog_tests.rs`
- Modify: `hal/soc/x86/src/lib.rs`
- Modify: `hal/soc/x86/src/tests.rs`
- Modify: `kernel/src/board.rs`

## Implementation Steps

1. Review the premature edits already present in this file set before changing anything.
2. Ensure no reference to `generic/x86_64-pc`, `GENERIC_X86_64_PC`, `GenericX86Pc`, or `generic-x86` remains in active source.
3. Ensure the board descriptor uses `QEMU_Q35_X86_64`, slug `qemu-q35-x86_64`, compatible strings including `qemu,q35`, model `q35`, and the resolved vendor convention.
4. Ensure `SocId::QemuX86Q35` is the only x86 q35 SoC identity and `hal_soc_x86::QEMU_Q35` is the only x86 q35 profile instance.
5. Verify driver list remains typed and shared: UART16550PortIo, IOAPIC, HPET, PCIe ECAM, NVMe PCI, e1000.

## Success Criteria

- [x] `grep -RInE 'generic/x86_64-pc|GENERIC_X86_64_PC|GenericX86Pc|generic-x86' boards hal kernel scripts .github docs` returns no active references.
- [x] `boards/qemu/q35-x86_64/board.rs` declares q35-only identity and model `q35`.
- [x] `kernel/src/board.rs` still validates board descriptor before returning it.
- [x] `hal/soc/x86/src/lib.rs` still has COM1 base, IRQ4, and bounded firmware windows.

## Security Considerations

Firmware discovery remains fail-closed; do not add fallback physical addresses for ACPI-discovered devices.

## Risk Notes

Likelihood medium, impact medium: name churn can break scripts or docs. Mitigation: grep old names and run board catalog tests. Rollback: restore old files from `HEAD` and revert only Phase 1 edits. Cannot undo: none, until committed.

## Deviation Log

- Deviation: implementation edits pre-exist plan.
  Why: read-only research scope was violated.
  Impact: Build must review existing edits before treating them as intentional.
  Revert: use scoped git restore only if implementation owner rejects them.
- Decision: `BoardDescriptor.vendor` stays lowercase `qemu` and `model` becomes exact `q35`.
  Why: existing QEMU descriptors use canonical lowercase vendor tokens, while the
  approved contract requires the machine model rather than an architecture suffix.
  Impact: the x86 QEMU descriptor now matches the naming style of the `virt` boards
  without widening support claims beyond the `qemu-q35-x86_64` slug.
  Revert: none unless descriptor field conventions change globally.
