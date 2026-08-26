---
phase: 1
title: "Rename Placeholder Board"
status: complete
priority: P2
effort: 30m
dependencies: []
tier: fast
---

# Phase 1: Rename Placeholder Board

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Escalate only if the implementation would add runtime support, board registration, a Cargo feature, CI lane, or a non-placeholder 32-bit x86 contract.

## Overview

Rename the inactive QEMU 32-bit x86 placeholder from `q35-i686` to `q35-x86_32` and update exact references. This improves human traceability against `hal/arch/x86/src/x86_32.rs` without claiming 32-bit x86 support.

## Requirements

- Functional: `boards/qemu/q35-x86_32/README.md` exists and remains README-only.
- Functional: no `board.rs`, `BoardDescriptor`, Cargo feature, catalog entry, or CI/runtime lane is added for 32-bit x86.
- Functional: tracked exact references to `q35-i686` are updated to `q35-x86_32`.
- Non-functional: preserve existing meanings: Rust target triple remains `i686-unknown-none`; QEMU binary remains `qemu-system-i386`; QEMU machine remains `q35`; HAL name remains `x86_32`.
- Non-functional: keep board/SoC/HAL ownership intact per `docs/code-standards.md:49-63`.

## Architecture

Data flow:

1. Input: repository naming references from `boards/`, `scripts/`, and `docs/`.
2. Transform: rename only the placeholder directory and exact textual references from `q35-i686` to `q35-x86_32`.
3. Exit: validation confirms the placeholder guard still treats `q35-x86_32` as README-only and no stale `q35-i686` references remain.

Dependency graph:

- Rename directory first so later path edits target the final location.
- Update `scripts/check-board-configs.sh` after the directory rename because its placeholder guard owns allowed placeholder paths.
- Update docs after the script path is settled.
- Run grep and board validation only after all text edits are complete.

Backwards compatibility:

- No runtime/API compatibility issue because the old directory is explicitly documentation-only and absent from registration at `scripts/check-board-configs.sh:95-99`.
- Human compatibility is handled by keeping README wording explicit: board slug is `q35-x86_32`; Rust target is `i686-unknown-none`; QEMU command is `qemu-system-i386 -machine q35`.

## Assumptions

- **Claim:** `.claude/scripts/set-active-plan.cjs` exists and can sync the active plan.
  **Confidence:** high
  **How to verify:** `test -f .claude/scripts/set-active-plan.cjs`

## Related Files

- Move: `boards/qemu/q35-i686/README.md` -> `boards/qemu/q35-x86_32/README.md`
- Modify: `boards/qemu/q35-x86_32/README.md`
- Modify: `scripts/check-board-configs.sh`
- Modify: `docs/system-architecture.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`

File ownership:

- Phase 1 owns only the files listed above.
- It must not touch `boards/src/lib.rs`, `boards/src/catalog_tests.rs`, `Cargo.toml`, `hal/arch/x86/src/x86_32.rs`, or `hal/soc/x86/src/lib.rs` unless grep finds an exact stale placeholder reference that must be corrected.

## Implementation Steps

1. Move `boards/qemu/q35-i686` to `boards/qemu/q35-x86_32`.
2. Edit the README title to `QEMU q35 x86_32 placeholder`.
3. In the README, state the terminology contract explicitly: Cellos board taxonomy `x86_32`, HAL module `x86_32`, Rust target `i686-unknown-none`, QEMU launcher `qemu-system-i386 -machine q35`.
4. Update `scripts/check-board-configs.sh:80-83` to use `boards/qemu/q35-x86_32`.
5. Update the placeholder-registration grep at `scripts/check-board-configs.sh:95-99` from `q35-i686` to `q35-x86_32`.
6. Update docs references in `docs/system-architecture.md:65-67`, `docs/project-roadmap.md:289`, and `docs/project-changelog.md:21-23`.
7. Re-grep tracked files for `q35-i686`; any remaining hit is either fixed or logged as a deliberate historical reference with justification.

## Success Criteria

- [x] `test -d boards/qemu/q35-x86_32 && test ! -e boards/qemu/q35-i686`
- [x] `find boards/qemu/q35-x86_32 -mindepth 1 -maxdepth 1 -print` outputs only `boards/qemu/q35-x86_32/README.md`.
- [x] `grep -RIn --exclude-dir=.git --exclude-dir=target --exclude-dir=.agents 'q35-i686' .` returns no tracked current-reference hits.
- [x] `bash scripts/check-board-configs.sh` passes with `PATH="$HOME/.cargo/bin:$PATH"`.
- [x] `git diff --stat` shows only the planned rename and exact reference/docs edits.

## Evidence

- `test -d boards/qemu/q35-x86_32 && test ! -e boards/qemu/q35-i686 && printf 'PASS: board rename on disk\n'` -> `PASS: board rename on disk`
- `find boards/qemu/q35-x86_32 -mindepth 1 -maxdepth 1 -print` -> `boards/qemu/q35-x86_32/README.md`
- `grep -RIn --exclude-dir=.git --exclude-dir=target --exclude-dir=.agents 'q35-i686' . || true` -> no output
- `PATH="$HOME/.cargo/bin:$PATH" bash scripts/check-board-configs.sh` -> PASS; `HAL/SoC/board boundaries are intact` and the full board matrix checks completed
- Manual diff review of `git diff` -> PASS, no findings

## Test Matrix

- Unit: none; no code path changes.
- Integration: `bash scripts/check-board-configs.sh` exercises board assets, placeholder guard, HAL boundary check, board host contracts, and target cargo checks.
- E2E/runtime: not required and not meaningful; this placeholder has no runtime board lane.

## Risk Assessment

- Medium likelihood x Low impact: a stale `q35-i686` reference may remain in docs or scripts. Mitigation: final repo-wide grep excluding generated/build/plan dirs.
- Low likelihood x Medium impact: someone may read `x86_32` as a supported board. Mitigation: README must retain "not implemented", "no board.rs", "no Cargo feature", "no CI lane", and "not supported" bullets.
- Low likelihood x Medium impact: replacing every `i686` string would corrupt Rust target terminology. Mitigation: replace only the board slug `q35-i686`; keep `i686-unknown-none` where it names the target triple.
- Rollback: move `boards/qemu/q35-x86_32` back to `boards/qemu/q35-i686` and revert the exact references in scripts/docs. No irreversible part.

## Security Considerations

N/A. No runtime, firmware parsing, MMIO, driver, syscall, or boot-path behavior changes.

## Deviation Log

None.
