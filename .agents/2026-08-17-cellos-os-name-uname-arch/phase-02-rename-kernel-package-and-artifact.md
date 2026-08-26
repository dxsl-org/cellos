---
phase: 2
title: "Rename Kernel Package and Artifact"
status: completed
priority: P2
effort: "4h"
dependencies: [1]
tier: medium
---

# Phase 2: Rename Kernel Package and Artifact

> Required — deviation-log: Log every Decision / Deviation / Surprise the moment it occurs.

## Overview

Rename the Cargo package/artifact from `vicell-kernel` to `cellos-kernel` across active build, run, image, boot, test, and current user docs after Phase 1 has made runtime-visible naming coherent. This phase deliberately does not rename `Vi*` source symbols, ABI sections, disk/protocol magic, historical changelog text, build error logs, or old built artifacts.

## Requirements

- Functional:
  - `kernel/Cargo.toml:2` package name becomes `cellos-kernel`.
  - Build commands use `-p cellos-kernel` instead of `-p vicell-kernel`.
  - Target artifact paths point to `target/<triple>/release/cellos-kernel`.
  - Bootloader/image configs load `/cellos-kernel` instead of `/vicell-kernel` (`limine.conf:6`, `limine-vf2.conf:6`, `limine-pioneer.conf:6`).
  - Current run/build/image scripts and integration tests reference `cellos-kernel`.
  - Phase 1 metadata reports `KERNEL_NAME = "cellos-kernel"` in `uname -a`.
- Non-functional:
  - Preserve `ViResult`, `ViError`, public `Vi*` traits/functions/types, and `ViCell_syscall_dispatch` (`kernel/src/task/syscall.rs:5228`).
  - Preserve linker/ABI sections such as `__ViCell_manifest`, `__ViCell_syscalls`, and `__ViCell_cluster` (`cells/demos/hello-cell/hello-cell.ld:35-44`).
  - Preserve disk/protocol magic and compatibility constants; do not broad-replace `ViCell`.
  - Do not edit historical changelog text, captured build error logs, binary artifacts, or old generated images.

## Architecture

Data flow:

1. Cargo package name in `kernel/Cargo.toml` enters cargo commands through `-p <name>`.
2. Cargo output binary name follows package/bin artifact naming and exits at `target/<target>/release/cellos-kernel`.
3. Scripts/image builders copy that artifact into boot media as `cellos-kernel`.
4. Bootloader configs reference the copied boot file path.
5. Tests and docs consume the new package name/path.

Dependency graph:

- Phase 1 must finish first so `uname -a` can be updated once from old artifact name to `cellos-kernel`.
- Package rename blocks every command/path update; tests must update after scripts/configs or they will point at non-existent artifacts.
- Verification starts from an observed baseline on the current dirty worktree, then compares post-rename package/path behavior.

## Assumptions

- None — current `vicell-kernel` references were grepped in this checkout; implementer must re-grep before editing because the worktree is already dirty.

## Related Files

- Modify: `kernel/Cargo.toml`
- Modify: `libs/ostd/src/system_info.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/perf.yml`
- Modify: root build/run scripts: `gen_disk.ps1`, `gen_disk_rpi3.ps1`, `run*.ps1`, `do_build.bat`
- Modify: boot configs: `limine.conf`, `limine-vf2.conf`, `limine-pioneer.conf`
- Modify: scripts under `scripts/` that build, copy, test, flash, or document kernel artifact paths.
- Modify: integration tests under `tests/integration/` that reference package name or target artifact path.
- Modify: current docs under `docs/baremetal/`, `docs/specs/10-testing.md`, `docs/vf2-bringup.md`, and `docs/pioneer-bringup.md`.
- Do not modify: `docs/project-changelog.md`, build logs under `build/`, binary/generated artifacts, `target/`, linker scripts with `__ViCell_*`, syscall dispatch bindings, protocol constants.

## Implementation Steps

1. Baseline guard: run `git status --short` and `git grep -n -F "vicell-kernel" -- . ":!target" ":!build"`; save counts in the deviation log or implementation report.
2. Baseline board-rpi3 check before rename, if host prerequisites exist:
   `cargo build --release --features board-rpi3 -p vicell-kernel --target aarch64-unknown-none-softfloat`.
   If host-gated, record exact missing prerequisite and continue with static/package checks only.
3. Rename `kernel/Cargo.toml` package to `cellos-kernel`.
4. Update Phase 1 metadata `KERNEL_NAME` to `cellos-kernel`.
5. Update active package selectors, artifact paths, copy destinations, and bootloader `PATH=` entries from `vicell-kernel` to `cellos-kernel` in current scripts/configs/tests/docs.
6. Preserve derived variant suffixes by renaming them mechanically:
   - `vicell-kernel-test-hooks` => `cellos-kernel-test-hooks`
   - `vicell-kernel-shell-test` => `cellos-kernel-shell-test`
   - `vicell-kernel-srv-test` => `cellos-kernel-srv-test`
7. Re-grep stale references with explicit exclusions for allowed old text/artifacts.

## Success Criteria

- [x] `cargo metadata --no-deps` shows package `cellos-kernel` and no package named `vicell-kernel`.
- [x] `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` passes or records an already-known host-gated reason.
- [x] `cargo check -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc` passes or records an already-known host-gated reason.
- [x] `cargo build --release --features board-rpi3 -p cellos-kernel --target aarch64-unknown-none-softfloat` produces `target/aarch64-unknown-none-softfloat/release/cellos-kernel`, or the host-gated blocker is recorded.
- [x] Static stale guard passes: `git grep -n -F "vicell-kernel" -- . ":!target" ":!build" ":!docs/project-changelog.md"` returns only explicitly allowed historical/old-artifact references, ideally none in active configs/scripts/tests/current docs.
- [x] Static preserve guard passes: `git grep -n "ViCell_syscall_dispatch\\|__ViCell_\\|ViResult\\|ViError" -- kernel hal cells libs` still finds the expected internal identifiers.
- [x] Boot configs reference `/cellos-kernel`, not `/vicell-kernel`.

## Test Matrix

- Baseline before rename:
  - AArch64 board-rpi3 build path/package check with `vicell-kernel`.
  - `cargo metadata --no-deps` records old package name.
- Post rename:
  - AArch64 board-rpi3 build verifies package selector and artifact name.
  - Generic RV64 package check verifies `-p cellos-kernel` resolves.
  - Generic x86_64 package check verifies `-p cellos-kernel` resolves.
  - Static stale-reference guard covers active configs/scripts/tests/current docs.
  - Static preserve guard covers ABI/source identifiers that must not change.

## Security Considerations

This is a compatibility-sensitive rename. The security risk is accidental ABI/protocol drift, not data exposure. Use targeted replacements and never rename exported ABI sections, syscall dispatch symbols, or `Vi*` API names.

## Risk Assessment

- High likelihood / High impact: stale artifact path in image/boot scripts causes boot failure. Mitigation: update package selector, target path, copied filename, and bootloader `PATH=` in one phase; verify AArch64 board-rpi3 and at least RV64/x86 package checks.
- Medium likelihood / High impact: broad rename mutates ABI sections or public APIs. Mitigation: exclude `__ViCell_*`, `ViCell_syscall_dispatch`, `ViResult`, `ViError`, `Vi*` traits/types; run preserve grep after edit.
- Medium likelihood / Medium impact: captured logs/docs still mention old name and create noisy grep output. Mitigation: exclude build logs, old artifacts, and historical changelog by policy; update only current docs.
- Rollback: revert this phase's tracked file changes and rebuild; remove newly generated `cellos-kernel*` artifacts if created. Non-recoverable part: none expected; no migration or external state.

## Backwards Compatibility

- Source/API/ABI compatibility is preserved for cells and HAL/kernel boundaries.
- CLI/build compatibility intentionally changes: `-p vicell-kernel` becomes `-p cellos-kernel`, and artifact paths change.
- Existing external automation that boots `vicell-kernel` must update to `cellos-kernel`.

## File Ownership

Phase 2 owns package/artifact references in current build/run/image/boot configs/tests/current docs. It does not own runtime prompt/banner/help strings except `KERNEL_NAME` metadata produced by Phase 1.

## Evidence

- Format: PASS
- Metadata package: `cellos-kernel` present; no `vicell-kernel` package
- Host integration compile: PASS
- RV64/x86 checks: PASS
- Board-rpi3 release build: PASS
- Stale refs/preserve guards: PASS
- Tester: all-pass
- Reviewer: CLEAR

## Deviation Log

- None.
