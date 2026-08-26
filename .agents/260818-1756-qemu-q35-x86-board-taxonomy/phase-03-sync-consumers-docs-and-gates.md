---
phase: 3
title: "Sync Consumers Docs and Gates"
status: completed
priority: P1
effort: "2h"
dependencies: [1, 2]
tier: medium
---

# Phase 3: Sync Consumers Docs and Gates

## Overview

Update every consumer, test, script, and doc so q35 x86_64 is the only current x86 board identity and q35-i686 remains future-only documentation.

## Requirements

- Functional: update catalog tests, board config script, boundary script, CI labels if needed, and living docs to name QEMU q35 x86_64 accurately while keeping the three 32-bit QEMU placeholders unsupported.
- Non-functional: preserve boot order `boot -> COM1 -> ACPI -> timer -> SMP -> PCIe -> NVMe`; preserve QEMU-only evidence boundary; do not make networking first.

## Architecture

Tests validate the board descriptor and typed driver list at `boards/src/catalog_tests.rs:42` and `boards/src/catalog_tests.rs:62`. Scripts enforce supported board assets and boundaries via `scripts/check-board-configs.sh:65` and `scripts/check-hal-boundaries.sh:45`. CI builds and boots x86 q35 in the existing x86 job (`.github/workflows/ci.yml:646`, `.github/workflows/ci.yml:714`).

## Assumptions

- **Claim:** Existing docs have no active references to `generic/x86_64-pc` after premature edits.
  **Confidence:** medium
  **How to verify:** grep docs for old names and q35 naming before Build completion.

## Related Files

- Modify: `boards/src/catalog_tests.rs`
- Modify: `scripts/check-board-configs.sh`
- Modify: `scripts/check-hal-boundaries.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/project-changelog.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/system-architecture.md`

## Implementation Steps

1. Update catalog assertions to include exact q35 identity, model, compatible strings, empty fallback memory, and typed drivers.
2. Keep `scripts/check-board-configs.sh` counting `boards/qemu/q35-x86_64` only; do not add q35-i686 to `board_dirs`.
3. Keep `scripts/check-hal-boundaries.sh` requiring `QEMU_Q35` and rejecting COM1/firmware-window facts outside `hal/soc/x86`.
4. Update docs to say q35 x86_64 is QEMU integration evidence only; future physical PC descriptors must be vendor/model specific.
5. Add doc text for `q35-i686`, `virt-riscv32`, and `virt-aarch32` as planned/not implemented and explicitly unsupported.

## Success Criteria

- [x] `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
- [x] `bash scripts/check-hal-boundaries.sh`
- [x] `bash scripts/check-board-configs.sh`
- [x] Docs contain no claim that q35 x86_64 is a generic or physical PC board.
- [x] `q35-i686`, `virt-riscv32`, and `virt-aarch32` are absent from catalog, CI, features, and supported-board matrix.

## Security Considerations

Do not weaken validation or boundary checks while renaming. Evidence wording is security-relevant because it prevents using QEMU boot as physical hardware validation.

## Risk Notes

Likelihood medium, impact high: docs or scripts may overcount q35-i686 as supported. Mitigation: negative grep gate and no `board.rs`. Rollback: revert Phase 3 docs/scripts/tests. Cannot undo: none, until committed.

## Deviation Log

- Decision: CI workflow file remains unchanged.
  Why: current CI jobs are architecture-scoped and already exercise the x86 QEMU lane
  without embedding the retired `generic/x86_64-pc` board path or slug.
  Impact: taxonomy is corrected in source, scripts, and docs without churn in unrelated CI names.
  Revert: none needed unless CI later grows board-specific naming.
