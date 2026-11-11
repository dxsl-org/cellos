---
phase: 9
title: "Kernel residents, selftests and module layout"
status: in-progress
priority: P2
effort: "7–10d"
dependencies: [8]
tier: medium
checkpoint: true
---

# Phase 09: Kernel residents, selftests and module layout

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-8, Medium kernel shape](../reports/architecture-review-261110.md) · [Spec 15](../../docs/specs/15-kernel-boundary.md) · `kernel/src/main.rs` · `kernel/src/task.rs` · `kernel/src/task/drivers.rs` · `kernel/src/snapshot.rs` · `scripts/generate-code-metrics.py` · owner decision 3 (Phase 00)

## Overview

Make the kernel tree say what it is: test code out of production boots, an inert subsystem behind its feature, a checked list of what resides in the kernel and why, and directories that match responsibilities.

## Key Insights

- Verified: `main.rs:1062-1125` runs nine selftests on every 64-bit boot with only some under `test-hooks`; a FAIL only logs. Spec 15 §3.5 says such code never ships.
- Reviewer-cited: PS/2, fb console, `virtio_rng` stub, ACPI power/SCI/HPET/SPCR, SMBIOS and a kernel FD/cwd layer are residents Spec 15 does not admit; `snapshot.rs` (5,769 lines) is always compiled and gated only by a runtime const (`:333`); the size metric drops files whose stem contains "test" (`generate-code-metrics.py:103-104`).

## Requirements

- Functional: all selftests under `kernel/src/selftest/` behind `cfg(posture = "test")` (or `test-hooks`), including the boot call block; `snapshot` compiled only with `snapshot-qualified`, with a stub API for callers; a resident manifest with a CI guard; `task/` split into `sched/`, `ipc/`, `dev/`, `fsapi/` by file moves; raw `--cfg cellos_p01_trace` / `cellos_g1_cpu` turned into named features confined to one module each; metric excludes by cfg/module, not filename.
- Non-functional: move-only commits; no behaviour change in non-test postures except the selftests no longer running.

## Architecture

- `kernel/residents.toml`: `path`, `spec15_category = A|B|C|D|exception`, `migration_target`, `owner`. Guard `scripts/check-kernel-residents.py` fails on any file under `kernel/src/{dev,acpi,fs}` not listed.
- Residents that Spec 15 forbids are not moved in this phase; they are listed as `exception` with a target (Platform/Supervisor cell). Spec 15 is amended to match reality and to admit pipes/futex/sessions or schedule their move — an owner call recorded in the spec's amendment note.

## Assumptions

- **Claim:** no production lane depends on a selftest's side effect (e.g. initialising state).
  **Confidence:** low
  **How to verify:** boot the x86 default image with the block compiled out before moving files; compare boot markers.
- **Claim:** `snapshot` callers in `boot.rs`, `main.rs`, `task/quiesce.rs` and the syscall arm can be satisfied by a stub returning `NotSupported`.
  **Confidence:** medium
  **How to verify:** list callers by LSP references on `snapshot::`.
- **Claim:** CI lanes that grep for "self-test PASS" lines exist.
  **Confidence:** medium
  **How to verify:** grep `self-test` in `scripts/` and `tests/integration/`; those lanes switch to a test-posture image.

## Related Code Files

- Create: `kernel/src/selftest/mod.rs`, `kernel/residents.toml`, `scripts/check-kernel-residents.py`
- Modify (moves): `kernel/src/task/*_selftest.rs`, `kernel/src/*_selftest.rs` → `kernel/src/selftest/`; `kernel/src/task/drivers/**` → `kernel/src/dev/`; IPC files → `kernel/src/ipc/`; scheduler/SMP/hart-local → `kernel/src/sched/`; file API from `task.rs` → `kernel/src/fsapi/`
- Modify: `kernel/src/main.rs`, `kernel/src/task.rs`, `kernel/Cargo.toml`, `kernel/build.rs`, `scripts/generate-code-metrics.py`, `docs/code-metrics.generated.md`, `docs/specs/15-kernel-boundary.md`, `scripts/mutable-globals.toml`, `scripts/unsafe-allowlist.toml` (paths)
- Delete: stubs (`virtio_rng` no-op, 21-line `nic.rs`, stale `drivers.rs` header), `async-trait` dependency

## Implementation Steps

1. Compile-out experiment for the selftest block; record boot-marker diff.
2. Move selftests under `selftest/`, gate the root and the call block; switch marker-grepping lanes to the test posture.
3. Gate `snapshot` behind its feature with a stub; build both ways.
4. Write `residents.toml` from the current tree; add the guard; amend Spec 15 §2–§3 to match and record each exception's target.
5. Hoist `task/` by responsibility, one directory per commit; update inventories whose keys are paths.
6. Convert the two raw cfgs to features; confine their sites.
7. Fix the metric script; regenerate `code-metrics.generated.md`; group `Task` fields into sub-structs (caps already gone in Phase 07).

## Todo List

- [ ] Selftests gated
- [ ] Snapshot gated
- [ ] Resident manifest + guard + Spec 15 amendment
- [ ] Directory hoist
- [ ] Raw cfgs → features
- [ ] Metric corrected

## Success Criteria

- [ ] Default-posture x86 boot log contains no `self-test` line; test-posture image still reports all of them PASS.
- [ ] `strings`/symbol check: no `*_selftest` symbol in a release-posture kernel.
- [ ] `python3 scripts/check-kernel-residents.py` fails on an unlisted new file under `kernel/src/dev/` (negative control).
- [ ] Checkpoint: full `pr`-tier suite on three arches, `x86-c2c`, release-posture boot; regenerated metric committed; review recorded.

## Risk Assessment

- Path-keyed inventories (`unsafe-allowlist.toml`, `mutable-globals.toml`) break on moves: update in the same commit as each move. Rollback: revert the series.
- Selftests were the only coverage for some paths in default lanes: step 2 keeps them in a test-posture lane on every PR.

## Security Considerations

Removes ~3.4k lines of fixture code that drive the live scheduler from release images; makes the TCB size metric honest.

## Next Steps

Phases 10 and 14 both build on the new layout.

## Deviation Log

- **Scope (owner 2026-11-11):** the dispatcher-match extraction moved here from Phase 08. It runs first, ahead of the plan's step 1. Spec 15 admits pipes, futex and sessions as kernel residents.
- **Step 5 first (dispatcher extraction):** commits `b7441d8d2` (vm), `986f5e09e` (misc, grant, authority) and `1e0d135ab` (fs, dev, spawn, ipc).
  - **Mechanics.** A throwaway extractor turned each of the 153 arms into `pub(super) fn <variant>(caller_id?, fields…, #[cfg(cellos_p01_trace)] trace?) -> SyscallResult`, typed from the `Syscall` enum. The body is copied verbatim, with `super::` re-anchored. The arm now calls the function.
  - **Why the transform is sound.** The match is the tail expression of `handle_syscall_inner`, so a `return` in a body returns the same value from the new function. The argv-cleanup guard still drops after the match.
  - **Deviation:** the plan called for one commit per domain. That held for vm only; the others landed in two commits, because the batch loop did not stop between domains.
  - **Hand edits on top of the transform:**
    - two non-x86 and non-display branches add `caller_id` to their unused-argument tuple, since the parameter is now the handler's own;
    - four handlers that collided with existing helpers are renamed (`*_syscall`, and `yield_now` for `Yield`).
  - **Result.** `mod.rs` goes from 6 635 to 2 207 lines. Host tests 615 pass, including `syscall_golden` and the gate-equivalence test. Clippy is clean on 3 arches, and `test-hooks`, `bench-counters` and `getrandom-sas-test` check.
