---
phase: 1
title: "Update Runtime Branding and Architecture Metadata"
status: completed
priority: P2
effort: "3h"
dependencies: []
tier: medium
---

# Phase 1: Update Runtime Branding and Architecture Metadata

> Required — deviation-log: Log every Decision / Deviation / Surprise the moment it occurs.

## Overview

Replace runtime-facing `ViCell` labels with `Cellos` in the shell, sys-tools, and init banner while preserving internal `ViCell`/`Vi*` technical identifiers. Make `uname -a` derive architecture from the compiled target instead of hardcoding `riscv64`; package/artifact rename is owned by Phase 2.

## Requirements

- Functional:
  - Shell prompt prints `Cellos > ` instead of `ViCell > ` (`cells/tools/shell/src/shell.rs:33`).
  - Shell readiness banner prints `=== Cellos shell ready — type 'help' for commands ===` (`cells/tools/shell/src/shell.rs:51`).
  - Shell help title prints `Cellos Shell v0.2.1 — built-in commands:` (`cells/tools/shell/src/commands.rs:7`).
  - Shell built-in `uname` prints `Cellos`; `uname -a` prints `Cellos vicell-kernel 0.2.1 <arch> Cellos` (`cells/tools/shell/src/cmd_sys.rs:16-22`).
  - `/bin/uname` prints the same full string as shell `uname -a` (`cells/tools/sys-tools/src/bin/uname.rs:8-11`).
  - Shell and `/bin/env` print `OS=Cellos` (`cells/tools/shell/src/cmd_sys.rs:39-43`, `cells/tools/sys-tools/src/bin/env.rs:8-14`).
  - Init prints `Init: Starting Cellos Orchestrator...` (`cells/tools/init/src/main.rs:70-76`).
- Non-functional:
  - Do not rename package/artifact `vicell-kernel` in this phase; Phase 2 owns that after runtime branding is coherent (`.github/workflows/ci.yml:180`, `kernel/Cargo.toml:2`).
  - Do not rename ABI symbols/sections `__ViCell_*` or `ViCell_syscall_dispatch` (`cells/demos/hello-cell/hello-cell.ld:35-44`, `kernel/src/task/syscall.rs:5228`).
  - Do not rename `ViResult`, `ViError`, public `Vi*` traits/types, syscall numbers, protocol constants, or unrelated docs/history.
  - No `unsafe` required.

## Architecture

Data flow:

1. Compile target enters Rust through `#[cfg(target_arch = "...")]`.
2. A tiny no_std metadata source maps target arch to display string: `riscv32`, `riscv64`, `aarch64`, `x86_64`, with an `unknown` fallback for unsupported host/test targets.
3. Shell built-ins and sys-tools read the same metadata constants and render:
   - `uname`: OS name only.
   - `uname -a` and `/bin/uname`: OS name + current kernel artifact name + version + target arch + OS name.
   - `env`: `OS=Cellos`.
4. Integration tests/scripts consume the new prompt/banner strings as boot-readiness outputs.

Dependency graph:

- Metadata source must land before shell/sys-tools use it.
- Runtime string updates must land before tests/scripts switch expected prompt/banner.
- Verification starts after all expected strings are updated, because partial updates produce predictable false failures.

Recommended design:

- Add a small `ostd` module such as `libs/ostd/src/system_info.rs`, exported from `libs/ostd/src/lib.rs:37-52`, because both `app-shell` and `app-sys-tools` already depend on `ostd` (`cells/tools/shell/Cargo.toml:14-19`, `cells/tools/sys-tools/Cargo.toml:8-11`).
- Keep constants simple: `OS_NAME`, `KERNEL_NAME`, `VERSION`, `ARCH`, and a small formatter/helper only if needed to avoid duplicated full uname string construction.
- Do not place metadata in `libs/api/` or `libs/types/`; `docs/code-standards.md` marks those interfaces sacred.
- Set `KERNEL_NAME` to the current artifact in this phase; Phase 2 updates it to `cellos-kernel` with the package/artifact rename.

## Assumptions

- None — file paths, strings, package dependencies, and target-arch usage were grepped/read in this checkout.

## Related Files

- Create: `libs/ostd/src/system_info.rs`
- Modify: `libs/ostd/src/lib.rs`
- Modify: `cells/tools/shell/src/shell.rs`
- Modify: `cells/tools/shell/src/commands.rs`
- Modify: `cells/tools/shell/src/cmd_sys.rs`
- Modify: `cells/tools/sys-tools/src/bin/uname.rs`
- Modify: `cells/tools/sys-tools/src/bin/env.rs`
- Modify: `cells/tools/init/src/main.rs`
- Modify: `tests/integration/tests/boot.rs`
- Modify: `tests/integration/tests/capacity-observability.rs`
- Modify: all `ViCell >` gate references under `tests/integration/tests/*.rs`
- Modify: `scripts/qemu-boot-test.sh`
- Modify: `scripts/qemu-aarch64-test.sh`
- Modify: `scripts/qemu-x86_64-test.sh`
- Do not modify: package/artifact build references except user-facing `uname` metadata, linker scripts with `__ViCell_*`, HAL syscall dispatch bindings, `docs/project-changelog.md`, unrelated historical docs.

## Implementation Steps

1. Re-run `git status --short` and preserve pre-existing dirty edits; abort or narrow patch if any target file is already dirty from another change.
2. Add `ostd::system_info` constants using `cfg(target_arch)`:
   - `target_arch = "riscv32"` => `riscv32`
   - `target_arch = "riscv64"` => `riscv64`
   - `target_arch = "aarch64"` => `aarch64`
   - `target_arch = "x86_64"` => `x86_64`
   - fallback => `unknown`
3. Update shell prompt/banner/help/env/uname to use `Cellos` and shared metadata.
4. Update sys-tools `env`/`uname` to use the same metadata.
5. Update init banner to `Cellos`.
6. Update runtime gates in tests/scripts from `ViCell >` to `Cellos >` and shell-ready waits from `=== ViCell shell ready` to `=== Cellos shell ready`.
7. Re-grep to confirm remaining `ViCell` matches are only internal identifiers, ABI/linker names, package/artifact names, demos/descriptions, or historical docs.

## Success Criteria

- [x] `git grep -n -F "OS=ViCell" -- cells tests scripts` returns no runtime/test gate matches.
- [x] `git grep -n -F "ViCell vicell-kernel 0.2.1 riscv64 ViCell" -- cells tests scripts` returns no matches.
- [x] `git grep -n -F "=== ViCell shell ready" -- cells tests scripts` returns no matches.
- [x] `git grep -n -F "ViCell >" -- tests scripts cells/tools/shell/src` returns no runtime/gate matches; historical docs are not part of this criterion.
- [x] `cargo check -p app-shell -p app-sys-tools` passes for the host-available lane, or the implementer records the exact target/build-std command if bare-metal check is required.
- [x] At least one RV64 boot gate reaches `Cellos >` after rebuild, using the pre-Phase-2 kernel artifact path.
- [x] AArch64 build/gate, if host prerequisites exist, shows `uname -a` containing `aarch64`; if host-gated, mark deferred with prerequisite reason.

## Test Matrix

- Unit/static:
  - Compile `ostd::system_info` on the available host/cross target.
  - Grep negative tests for stale runtime strings.
- Integration:
  - RV64 QEMU boot: prompt and readiness banner are `Cellos`.
  - Shell commands: `uname`, `uname -a`, `env`, and `help` show `Cellos` where user-visible.
  - AArch64/RPi3 lane: `uname -a` reports `aarch64`.
  - x86_64 lane: prompt gate scripts expect `Cellos >`; `uname -a` reports `x86_64` when booted.
- Regression:
  - Verify `vicell-kernel` artifact name and `ViCell_syscall_dispatch` symbols are untouched by grep.

## Security Considerations

N/A. This changes display metadata only. Avoid changing `libs/api/`, `libs/types/`, syscall ABI, linker sections, or dispatch symbol names because those are compatibility/security boundaries.

## Risk Assessment

- High likelihood / Medium impact: many tests/scripts wait for the old prompt. Mitigation: update all `tests/` and `scripts/` gate strings in the same phase; use grep count before/after.
- Medium likelihood / High impact: accidental rename of ABI/internal `ViCell` symbols. Mitigation: use targeted replacements only; explicitly exclude `__ViCell_*`, `ViCell_syscall_dispatch`, `ViResult`, `ViError`, `vicell-kernel`, and docs/history.
- Medium likelihood / Medium impact: `ostd` metadata export changes compile surface for many cells. Mitigation: constants-only no_std module, no alloc/unsafe, and compile app-shell/app-sys-tools before boot gates.
- Rollback: revert this phase's files as one patch. Non-recoverable part: none expected; no data migration or external state.

## Backwards Compatibility

- User-visible output changes intentionally from `ViCell` to `Cellos`.
- Internal crate/artifact names remain unchanged until Phase 2.
- ABI/linker/protocol identifiers remain compatible (`__ViCell_*`, `ViCell_syscall_dispatch`, `Vi*` types).
- Existing scripts/tests must move with the new prompt; old external automation that waits for `ViCell >` will need the same string update.

## File Ownership

Phase 1 owns runtime branding files. Phase 2 may later edit `libs/ostd/src/system_info.rs` only to change `KERNEL_NAME` from `vicell-kernel` to `cellos-kernel`.

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
