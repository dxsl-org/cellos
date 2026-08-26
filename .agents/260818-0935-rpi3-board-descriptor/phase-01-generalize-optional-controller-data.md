---
title: "Generalize Optional Controller Data"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 01 — Generalize Optional Controller Data

## Requirements

- Change only `plic`, `clint`, and `rtc` to `Option<MmioRegion>`.
- Validate present optional entries exactly as before.
- Preserve mandatory UART and QEMU RV64 descriptor values.
- Update tests for absent-controller boards and malformed present entries.

## Related Code Files

- `boards/src/descriptor.rs`
- `boards/qemu/virt-riscv64/board.rs`
- `boards/src/descriptor_tests.rs`
- `kernel/src/platform.rs`

## Todo List

- [x] Generalize the three optional fields.
- [x] Keep RISC-V fallback access fail-closed.
- [x] Pass board and SoC unit tests.

## Risk Assessment

An accidental permissive unwrap could defer a malformed QEMU descriptor into early boot. Undo by restoring mandatory fields; no persistent format exists.

## Success Criteria

Existing QEMU values and tests remain green, and absence is represented without zero-sized fake MMIO.
