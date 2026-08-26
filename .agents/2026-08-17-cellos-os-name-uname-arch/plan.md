---
title: "Cellos OS Name and Kernel Artifact Plan"
description: "Align runtime branding, uname architecture reporting, and kernel package/artifact naming with Cellos."
status: completed
priority: P2
effort: 7h
branch: main
tags: [bugfix, user-facing, multi-arch]
blockedBy: []
blocks: []
created: 2026-08-17
---

# Cellos OS Name and Kernel Artifact Plan

## Overview

Implement the approved scope only: user-visible OS branding becomes `Cellos`; `uname -a` stops hardcoding `riscv64`; the Cargo package/artifact `vicell-kernel` becomes `cellos-kernel` across current build/run/image/boot configs/tests/current docs. `Vi*` functions/types, `__ViCell_*` ABI sections, disk/protocol magic, historical changelog text, build error logs, and old artifacts stay unchanged.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Update Runtime Branding and Architecture Metadata](./phase-01-runtime-branding-and-arch-metadata.md) | completed |
| 2 | [Rename Kernel Package and Artifact](./phase-02-rename-kernel-package-and-artifact.md) | completed |

## Dependencies

- No cross-plan dependency.
- Implementation must start from a dirty-worktree check and avoid unrelated existing edits noted in `reports/scout-report.md`.

## Key Decisions

- Use compile-time architecture metadata via `cfg(target_arch)` so RPi3/AArch64 reports `aarch64`, RV64 reports `riscv64`, and x86 reports `x86_64`.
- Centralize OS/kernel/version/arch strings for shell built-ins and sys-tools to avoid a second branding drift.
- Rename only the Cargo package/artifact string from `vicell-kernel` to `cellos-kernel` after runtime branding lands; leave `Vi*` API/ABI identifiers intact.
- Update current tests/scripts/docs that tell users how to build, boot, or locate the kernel; leave historical changelog, build logs, and stale artifacts alone.

## Open Questions

- None for approved scope.

## Cook Handoff

Run: `$hc-cook .agents/2026-08-17-cellos-os-name-uname-arch/plan.md`
