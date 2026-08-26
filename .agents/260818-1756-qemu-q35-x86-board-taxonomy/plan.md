---
title: "QEMU q35 x86 Board Taxonomy"
description: "Rename the current x86 board contract to QEMU q35 scope and add documentation-only 32-bit QEMU placeholders."
status: partial
priority: P2
effort: 6h
branch: fix/structure
tags: [cellos, boards, qemu, x86, hal]
created: 2026-08-18
---

# QEMU q35 x86 Board Taxonomy

## Overview

Move the current QEMU-verified x86 board identity out of `boards/generic/` and into `boards/qemu/q35-x86_64`, because runtime evidence comes from `qemu-system-x86_64 -machine q35` only (`scripts/qemu-x86_64-test.sh:39`, `scripts/qemu-x86_64-test.sh:40`). Add `boards/qemu/q35-i686/README.md`, `boards/qemu/virt-riscv32/README.md`, and `boards/qemu/virt-aarch32/README.md` as future documentation placeholders only, with no descriptor, build contract, catalog entry, CI lane, or supported-board count.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Normalize q35 x86_64 Identity](./phase-01-normalize-q35-x86-64-identity.md) | completed | none |
| 2 | [Add 32-bit QEMU Placeholders](./phase-02-add-i686-placeholder.md) | completed | 1 |
| 3 | [Sync Consumers Docs and Gates](./phase-03-sync-consumers-docs-and-gates.md) | completed | 1, 2 |
| 4 | [Verify Runtime and Cleanup](./phase-04-verify-runtime-and-cleanup.md) | partial | 3 |

## Data Flow

Board descriptor data enters via `boards/qemu/q35-x86_64/board.rs`, is exported by `boards/src/lib.rs:27`, selected by `kernel/src/board.rs:68`, paired with `hal_soc_x86::QEMU_Q35` at `kernel/src/board.rs:85`, and then consumed by early UART, firmware-window, ACPI, timer, PCIe, NVMe, and e1000 gates. The 32-bit QEMU READMEs exist only as documentation; no Rust module or build flow may consume them.

## Dependency Graph

Phase 1 establishes names and paths. Phase 2 depends on Phase 1 so the placeholder can state why it is not current support. Phase 3 updates catalogs, scripts, CI, and docs after final identity exists. Phase 4 runs after all names and gates are synchronized.

## Backwards Compatibility

No public runtime compatibility is promised for `generic/x86_64-pc`; it was introduced by commit `309d401b` and has not become hardware evidence. Backwards compatibility here means no stale references remain, generated artifacts are restored, and existing q35 BIOS/UEFI behavior remains unchanged.

## Test Matrix

- Unit: `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
- Formatting/static: `cargo fmt --all -- --check`, `git diff --check`
- Boundaries: `bash scripts/check-hal-boundaries.sh`, `bash scripts/check-board-configs.sh`
- Build: x86 cells and `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc`
- Runtime: BIOS q35 and UEFI OVMF evidence are recorded in `.agents/260818-1756-qemu-q35-x86-board-taxonomy/reports/harness/execution-evidence.json`; the direct WSL `build-x86_64-cells.ps1` path remains environment-blocked by pre-existing Windows clang/backslash assumptions

## File Ownership

Phase 1 owns board descriptor, board exports, SoC identity, and kernel selection. Phase 2 owns only `boards/qemu/q35-i686/README.md`. Phase 3 owns docs, scripts, CI, and tests. Phase 4 owns verification outputs and generated-artifact cleanup only.

## Deviation Log

- Deviation: implementation edits already exist before this plan was finalized.
  Why: a read-only researcher violated planning scope.
  Impact: current worktree shows deleted `boards/generic/x86_64-pc/*`, modified board/kernel/script/doc files, and untracked `boards/qemu/q35-x86_64/`.
  Revert: do not revert during planning; implementation owner must review and either keep, adjust, or reset only their scoped edits.

## Open Questions

- None. Descriptor vendor stayed canonical lowercase `qemu`, model is exact `q35`, and the 32-bit placeholders remain README-only.
