---
title: "Rename QEMU x86_32 Placeholder Board"
description: "Rename the QEMU 32-bit x86 placeholder from q35-i686 to q35-x86_32 without adding runtime support."
status: complete
priority: P2
effort: 30m
branch: main
tags: [refactor, docs, hal]
blockedBy: []
blocks: []
created: 2026-08-19
---

# Rename QEMU x86_32 Placeholder Board

## Overview

Rename the documentation-only placeholder `boards/qemu/q35-i686` to `boards/qemu/q35-x86_32` so board naming matches the existing HAL module name while preserving the correct Rust/QEMU terminology.

Observed facts:

- `boards/qemu/q35-i686/README.md:1` names the placeholder "QEMU q35 i686".
- `boards/qemu/q35-i686/README.md:3` says it is for future `qemu-system-i386 -machine q35` work.
- `boards/qemu/q35-i686/README.md:5-12` says it has no implementation, `board.rs`, descriptor, Cargo feature, CI lane, firmware evidence, or support claim.
- `hal/arch/x86/src/x86_32.rs:1-3` names the HAL as `x86_32` while documenting the Rust target as `i686-unknown-none`.
- `scripts/check-board-configs.sh:80-83` lists `q35-i686` as a placeholder directory.
- `scripts/check-board-configs.sh:95-99` rejects placeholder board registration outside placeholder READMEs.
- `docs/code-standards.md:35-45` requires multi-architecture awareness.
- `docs/code-standards.md:49-63` defines board/SoC/HAL ownership and forbids per-board driver forks.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Rename Placeholder Board](./phase-01-rename-placeholder-board.md) | complete |

## Dependencies

- No cross-plan dependency. This is a mechanical naming cleanup on an existing placeholder.

## Open Questions

- None blocking. Keep `i686-unknown-none` only where it names the Rust target triple; use `x86_32` for Cellos board taxonomy.
