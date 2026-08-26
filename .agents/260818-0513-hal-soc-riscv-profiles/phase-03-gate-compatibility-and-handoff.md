---
phase: 3
title: "Gate Compatibility and Document Handoff"
status: completed
priority: P2
effort: "0.5d"
dependencies: [2]
tier: medium
---

# Phase 3: Gate Compatibility and Document Handoff

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in this file when it occurs.

## Overview

Prove the RISC-V profile extraction preserved behavior across default QEMU, VF2, Pioneer, and AArch64 regression lanes, then update docs only with observed results.

## Requirements

- Functional: no source path outside the planned blast radius changes except docs.
- Functional: build/test evidence distinguishes QEMU proof from hardware-gated VF2/Pioneer/RPi3 proof.
- Non-functional: docs must not imply real hardware validation unless run.

## Architecture

Validation output exits through three channels: host unit tests for profile constants, compile checks for feature compatibility, and QEMU boot for default RV64 runtime. Documentation records only those channels and leaves hardware lanes deferred.

## Assumptions

None - no unverified claims.

## Related Files

- Modify after green gates: `docs/system-architecture.md`
- Modify after green gates: `docs/project-changelog.md`
- Modify after green gates: `docs/project-roadmap.md`
- No source edits beyond Phases 1-2.

## File Ownership

This phase owns docs and verification only. It must not modify `kernel/src/platform.rs`, `kernel/Cargo.toml`, or `hal/soc/riscv/**` unless a gate exposes a defect; if so, log the fix in the deviation log and rerun the relevant gate.

## Implementation Steps

1. Run `cargo fmt --all --check`.
2. Run `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu`.
3. Run RV64 compile gates for default, `board-vf2`, and `board-pioneer`.
4. Run AArch64 regression checks for default and `board-rpi3` because ARM must stay untouched.
5. Run `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`.
6. Run `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.
7. Update docs with exact commands and results; mark VF2/Pioneer/RPi3 hardware proof as not executed unless it actually ran.
8. Final grep guard: ensure no per-board copies of UART, SDHCI, DesignWare I2C/SPI, GIC/PLIC, or PCIe drivers were added.

## Success Criteria

- [x] All commands in the Test Matrix pass or are explicitly marked host-gated with the shortest decisive failure line.
- [x] `git diff --stat` shows only planned source/docs files.
- [x] Docs state `hal/soc/` now owns RISC-V profile data, while `boards/` still owns board descriptors and `cells/drivers/` still owns shared drivers.

## Security Considerations

No new user-facing ABI or syscall surface. Verification must preserve existing fail-closed Pioneer MMIO behavior.

## Risk Notes

- Risk: QEMU boot passes while board-specific hardware would fail. Mitigation: report VF2/Pioneer/RPi3 as compile-only unless hardware is actually run.
- Risk: docs overstate completion. Mitigation: write status as "RISC-V profile slice" only; keep BCM27xx/MMC and PLIC IRQ policy deferred.
- Rollback: revert docs plus Phase 1-2 source changes in one commit; no data migration exists.

## Deviation Log

- Decision: artifact validator was absent in this checkout, so harness validation was recorded manually in `reports/harness/verification.json` instead of claiming a validator PASS.
