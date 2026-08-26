---
phase: 1
title: "Define Board Package Contract"
status: completed
priority: P1
effort: 0.5d
dependencies: []
tier: thinking
---

# Phase 1: Define Board Package Contract

## Context Links

- Plan: `./plan.md`
- Scout: `./reports/scout-report.md`
- Evidence: `docs/TODO.md:5-25`, `docs/code-standards.md:12-20`, `Cargo.toml:33-70`

## Overview

Created the root `boards/` package contract and the first QEMU RV64 descriptor. This phase only established validated data and did not wire boot behavior yet.

## Key Insights

- Board packages belong at root `boards/`, matching the accepted decision and `docs/TODO.md:14-16`.
- Driver Cells remain shared under `cells/drivers/*`, already listed in workspace membership at `Cargo.toml:53-70`.
- `libs/api` and `libs/types` are ABI-sensitive per `docs/code-standards.md:12-20`; no ABI change was allowed.

## Requirements

- Functional: define board identity, compatible strings, boot contract, fallback memory map, MMIO defaults, and enabled driver set for `qemu-virt-riscv64`.
- Non-functional: no heap parser in early boot; no board-local driver code; no behavior change until Phase 2.

## Architecture

`boards/qemu/virt-riscv64/board.rs` is compiled as immutable `no_std` descriptor data and consumed directly by kernel boot/platform code. The descriptor is the only source of fallback constants in this slice.

## Related Code Files

- Create: `boards/Cargo.toml`
- Create: `boards/src/lib.rs`
- Create: `boards/src/descriptor.rs`
- Create: `boards/qemu/virt-riscv64/board.rs`
- Create: `boards/qemu/virt-riscv64/qemu-virt-riscv64.dts`
- Create: `boards/qemu/virt-riscv64/README.md`
- Modify: `Cargo.toml`
- Do not modify: `libs/api/*`, `libs/types/*`, `cells/drivers/*`

## Todo List

- [x] Board package exists at root, not under `hal/`.
- [x] Descriptor has no dependency on Driver Cell source code.
- [x] Unit tests cover invalid descriptor cases.

## Success Criteria

- [x] `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu` passes.
- [x] `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf` passes.
- [x] Descriptor constants match existing QEMU RV64 fallback/platform values.
- [x] No new `board-rpi3`/`board-vf2`/`board-pioneer` references were added in Phase 1 files.

## Evidence

- `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu` PASS (`8/8`).
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf` PASS.
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat` PASS.

## Risk Assessment

- Risk: schema grows beyond the QEMU RV64 slice. Likelihood medium, impact medium. Mitigation: include only fields consumed in Phase 2.
- Risk: runtime parser enters early boot. Likelihood low, impact high. Mitigation: static/generated descriptor only.
- Rollback: remove the `boards` workspace member and delete the new crate/package files. Non-undoable: none.

## Security Considerations

Descriptor data controls MMIO and memory ranges. Treat fallback maps as trusted build inputs and fail closed on validation errors before MMIO use.

## Next Steps

Phase 2 consumed the descriptor for QEMU RV64 only; no additional Phase 1 follow-up remains.

## Deviation Log

None.
