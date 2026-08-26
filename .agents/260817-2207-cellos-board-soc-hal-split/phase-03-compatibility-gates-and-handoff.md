---
phase: 3
title: "Gate Compatibility and Handoff"
status: completed
priority: P2
effort: 0.5d
dependencies: [2]
tier: medium
---

# Phase 3: Gate Compatibility and Handoff

## Context Links

- Plan: `./plan.md`
- Evidence: `scripts/qemu-boot-test.sh:8-13`, `.github/workflows/ci.yml:166-183`, `.github/workflows/ci.yml:249-272`

## Overview

Locked the slice with validation commands, documentation, and a migration handoff. This phase keeps the new board descriptor work from becoming an untracked architecture fork.

## Key Insights

- `scripts/qemu-boot-test.sh:8-13` states that build-only checks do not prove the kernel boots.
- CI already builds the RV64 kernel and uploads the boot artifact at `.github/workflows/ci.yml:166-183` and `.github/workflows/ci.yml:225-231`.
- The RV64 QEMU boot job runs `scripts/qemu-boot-test.sh` at `.github/workflows/ci.yml:249-272`.

## Requirements

- Functional: document how to build/rebuild the QEMU RV64 board package and verify behavior.
- Non-functional: no removal of legacy board features, no broad CI matrix redesign, no `hal/soc` extraction.

## Architecture

Validation flow: descriptor source enters compile checks, release build produces kernel ELF, QEMU boot consumes the ELF, and logs exit as pass/fail evidence. Handoff flow: docs record deferred RPi3/SDHCI and `hal/soc` work as separate plan inputs.

## Related Code Files

- Modify: `docs/system-architecture.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`
- Modify: `docs/code-standards.md` only if a durable dependency rule is required

## Todo List

- [x] Docs explain `boards/` vs `hal/soc` vs `cells/drivers`.
- [x] Handoff names deferred plans instead of silently expanding this slice.
- [x] Verification commands and host-gated failures are recorded.

## Success Criteria

- [x] Docs match actual files and do not overstate completion.
- [x] Final verification gates pass with no new failures.
- [x] Handoff names the next independently reversible `hal/soc` slice.

## Evidence

- `cargo fmt --all -- --check` PASS.
- `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu` PASS.
- RV64/AArch64 `cargo check` PASS, including `--features board-vf2`, `--features board-pioneer`, and `--features board-rpi3`.
- `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` PASS.
- `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel` PASS.
- `dtc -I dts -O dtb boards/qemu/virt-riscv64/qemu-virt-riscv64.dts` SKIPPED because `dtc` is not installed.
- `bash scripts/measure-coverage.sh` previously failed with `error[E0463]: can't find crate for 'profiler_builtins'` and `error[E0152]: duplicate lang item in crate 'core': 'sized'`; tracked as a non-blocking tooling limitation and not rerun in the final pass.

## Security Considerations

Document that fallback maps are trusted inputs and runtime DTB remains authoritative when valid.

## Risk Notes

The main risk is declaring the full HAL split complete. Keep `hal/soc`, AArch64/RPi3, SDHCI, and feature-collapse explicitly deferred.

## Next Steps

After this slice ships, create a separate plan for `hal/soc` extraction. Start that plan with BCM/JH/SG ownership rules and only then address RPi3 SDHCI quirks.

## Deviation Log

- None.
