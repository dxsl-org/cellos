---
phase: 4
title: "Verify Runtime and Cleanup"
status: partial
priority: P1
effort: "1h15m"
dependencies: [3]
tier: medium
---

# Phase 4: Verify Runtime and Cleanup

## Overview

Run the regression suite for the renamed q35 board and ensure generated runtime artifacts do not pollute the commit.

## Requirements

- Functional: prove formatting, unit tests, board/boundary scripts, x86 build, BIOS QEMU boot, and UEFI marker still pass.
- Non-functional: keep generated tracked artifacts clean; report QEMU as QEMU evidence only.

## Architecture

Verification follows the x86 bring-up order: boot, COM1, ACPI, timer, SMP, PCIe, NVMe. BIOS QEMU proof comes from `scripts/qemu-x86_64-test.sh:65`; UEFI proof should reproduce the prior marker evidence recorded at `.agents/260818-1600-x86-hal-split/reports/harness/execution-evidence.json:13`.

## Assumptions

- **Claim:** OVMF firmware is available in the local environment.
  **Confidence:** medium
  **How to verify:** run the same bounded UEFI command used in the previous x86 HAL split harness or locate `OVMF_CODE_4M.fd`.

## Related Files

- Verify only: all modified files from Phases 1-3
- Restore if generated: `build/vicell-x86.iso`
- Restore if generated: `build/x86-iso-root/boot/kernel.elf`
- Restore if generated: `kernel/src/embedded-x86_64/init`
- Restore if generated: `kernel/src/embedded-x86_64/kernel_fs.img`

## Implementation Steps

1. Run `cargo fmt --all -- --check`.
2. Run `git diff --check`.
3. Run host board and SoC tests.
4. Run board config and HAL boundary scripts.
5. Build x86 cells and release kernel.
6. Build a fresh x86 ISO and run BIOS QEMU q35 boot to shell.
7. Run bounded UEFI q35 boot and require `UEFI_QEMU_READY`.
8. Inspect `git status --short`; restore only generated tracked artifacts created by verification.
9. Ensure q35-i686 remains README-only after verification.

## Success Criteria

- [x] `cargo fmt --all -- --check`
- [x] `git diff --check`
- [x] `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
- [x] `bash scripts/check-hal-boundaries.sh`
- [x] `bash scripts/check-board-configs.sh`
- [ ] x86 release cells build passes.
- [x] x86 release kernel build passes.
- [x] BIOS QEMU q35 reaches `Cellos >`.
- [x] UEFI QEMU q35 reaches ACPI timer initialization, `Scheduler initialized`, and the shell (`UEFI_QEMU_READY` criterion).
- [x] `git status --short` contains only intentional source/doc changes.

## Security Considerations

Do not claim physical PC validation. If UEFI or BIOS QEMU fails, keep the failure visible and do not downgrade the gate to build-only.

## Risk Notes

Likelihood medium, impact medium: runtime build can regenerate tracked artifacts. Mitigation: inspect status after tests and restore only generated files listed above. Rollback: revert verification-generated artifacts; source rollback belongs to earlier phases. Cannot undo: external QEMU logs are disposable.

## Deviation Log

- Deviation: direct WSL `build-x86_64-cells.ps1` remains environment-blocked by
  pre-existing Windows clang/backslash assumptions.
  Why: the script expects the Windows-native build path rather than the WSL
  shell used for this verification pass.
  Impact: this is the only deferred line item; q35 BIOS/UEFI runtime, host
  tests, boundary gates, and release kernel build all passed.
  Revert: none; rerun the cells script in the intended Windows environment.
