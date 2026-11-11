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

- [x] Selftests gated
- [x] Snapshot gated
- [x] Resident manifest + guard + Spec 15 amendment
- [x] Directory hoist
- [ ] Raw cfgs → features
- [ ] Metric corrected

## Success Criteria

- [ ] Default-posture x86 boot log contains no `self-test` line; test-posture image still reports all of them PASS.
- [x] `strings`/symbol check: no `*_selftest` symbol in a release-posture kernel.
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
- **Decision (selftest gate, owner 2026-11-11 "not-release"):** boot fixtures are gated on `not(cellos_posture = "release")`, not on `test-hooks`. 13 integration-test files and 4 lane scripts read selftest markers from dev images, and those lanes stay as they are. Success criterion 1 therefore reads "release kernel" instead of "default posture".
  - Before this change, these fixtures ran on every 64-bit boot outside `test-hooks`: the 7a trust-model block, AArch64 `run_ipi_selftest`, P-TRUST, boot-ceiling, the MMIO allowlist and Manifest-v2. Their 13 always-compiled modules, plus the test-only helpers that only they use, are now compiled out of release kernels.
  - The `test-hooks` ones were already absent from release kernels, which forbid `test-hooks` (Phase 04).
  - **Kept in release on purpose:** the power-on known-answer tests for ed25519, signed-policy verify+parse and cell signing. They decide whether the crypto that admits code can be trusted at all, so they are not fixtures.
  - **Evidence.** The x86 release kernel has 0 symbols matching `selftest`/`self_test`. Its only matching strings belong to those three known-answer tests. `qemu-release-posture-test.sh` now fails if any such symbol appears.
- **Deviation (no move into `selftest/` yet):** the files stay where they are for now and move with the directory hoist, which updates every path-keyed inventory in one go.
- **Snapshot gate.** `snapshot.rs` became `snapshot_engine.rs` (5.7k lines). It compiles only with `snapshot-qualified` or under `cfg(test)`, so the host tests keep running.
  - Every other kernel gets `snapshot_disabled.rs` behind the unchanged `crate::snapshot` facade. That file keeps exactly the closed-gate behaviour: no scratch reservation, capture → `GateClosed`, and a cold boot that clears a stale header (read first, write only on magic).
  - The shared header identity (magic, base LBA, sector size) lives in `snapshot_format.rs`.
  - The four engine statics were removed from `mutable-globals.toml`, because the tool requires entries only for declarations linked in its profile. A note there asks for re-review when the qualified profile ships.
  - No lane exercised the qualified profile before this change, and none does now. Clippy is clean with `snapshot-qualified` on all 3 arches.
- **Fix:** the `WEAK_RNG_WARNED` inventory key had gone stale in commit `1e0d135ab` (the dispatcher extraction moved it into `ipc::get_random`). It is re-keyed here.
- **Hoist 1/4 (`task/drivers` → `dev/`).** The files moved and the module path changed from `crate::task::drivers` to `crate::dev`.
  - Visibility scoped to `crate::task` inside the moved files became `pub(crate)`. That is required now that `dev` is no longer under `task`, and it changes no behaviour.
  - Also updated: the inventory keys, the `check-hal-boundaries` path, the law-1 source pin, the metric script (it now excludes `dev/**`), and the two `#[path]`/`include_str!` integration tests.
- **Surprise (my Phase 08 regression).** The manual-tier integration tests `pcie-bdf-ownership` and `driver-registration-contract` used `include_str!` on `kernel/src/task/syscall.rs`. That file was deleted by the Phase 08 split, so neither test has compiled since. No workflow ran them.
  - Both tests only pin dispatcher source text (`// 417: …` arm markers and `contains` checks). Under the test policy, such tests are deleted, not re-pinned. They are deleted together with their `Cargo.toml` and `lanes.toml` entries.
- **Pre-existing failure (not mine).** `tests/rust-std-promotion` fails 2/33: frozen `.agents/260821-…` artifact digests no longer match the live sources. The same 2 tests fail on `8a005f998`.
- **Surprise.** `docs/code-metrics.generated.md` had gone stale over the earlier Phase 09 commits; it is regenerated here. Every later commit now runs `generate-code-metrics.py --check`.
- **Hoist 2/4 (`sched/`).** Moved out of `task/` into `crate::sched`: `scheduler`, `smp` (and `smp/`), `hart_local` (and `hart_local/`), `quiesce`, `cpu_accounting`, `domain_switch` and `domain_switch_tests`.
  - Two `pub(super)` items of `task` are used from `sched` and became `pub(crate)`: `Task::owner_death_matches_receive_mask` and `completion_wait::irq_wake_due`.
  - **Path-only re-pin.** The law-1 guard `check-multi-source-wait-law1-digests.sh` pins the scheduler's sweep call as source text. Its path moved from `super::completion_wait::irq_wake_due` to `crate::task::completion_wait::irq_wake_due`, with the same function and arguments. `K_SCHED` follows the file. This is not an ABI delta (S4b approval: "Law 1: no delta").
  - The `cellos_g1_cpu` + `cellos_p01_trace` build was checked locally using a dummy 96-byte manifest.
  - `qemu-tier3-ssh-guest-x86.sh` (manual) keeps its already-stale sha pins.
- **Hoist 3/4 (`ipc/`).** Moved into `crate::ipc`: `ipc_wire` (and its tests), `pending_mailbox`, `pipe`, `futex` and `cell_session`. `ipc.rs` keeps `task`'s `clippy::result_unit_err` allowance.
  - **Deviation:** `async_ipc` stays in `task`. Moving it would have meant widening 15 private `task` items to `pub(crate)`: the wait-state publication, source selection, block handoff and wake helpers. It is the task-blocking glue, not an IPC object.
  - Widened to `pub(crate)` because the moved modules use them: `syscall::{session_launch_request, snapshot_grant_elf}` and `task::ipc_target_is_tombstone`.
- **Hoist 4/4 (`fsapi`).** The BootFS file-descriptor/cwd layer moved out of `task.rs` (path resolution, `file_*` handlers; about 325 lines) into `kernel/src/fsapi.rs`, which is one file, so no `fsapi/` directory was created. Callers use `crate::fsapi::file_*`.
  - `resolve_path` became `pub(crate)` because the path selftest calls it. `dir_inherit` stays in `task`: it is spawn-time handle transport, not file API.
  - The task directory hoist is complete: `dev/`, `sched/`, `ipc/` and `fsapi`.
- **Selftest follow-up.** `task::ipc_test` and `memory::tests` are uncalled test harnesses that were compiled into every kernel. They are now gated off release like the boot fixtures. Found while classifying residents.
- **Stubs (step 7).** Deleted:
  - `dev/nic.rs`. Its two always-fail functions are inlined into `NetTx`/`NetRx`. Results are unchanged: `NetTx` still reads, and so validates, the frame and returns 0. `NetRx` still validates the buffer and returns 0, but no longer allocates a buffer it never filled, so an out-of-memory error can no longer occur there.
  - The uncalled `virtio_rng::init_driver` no-op. The module's misleading "NIC selector" comment is corrected.
  - The unused `async-trait` dependency, with its `Cargo.lock` entry.
  - The never-compiled `intrinsics.rs` and `prelude.rs`, together with their `[[unreached]]` acknowledgements in the inventory.
- **Residents (step 4).** `kernel/residents.toml` lists all 297 kernel files: A=82, B=27, C=41, D=28, exception=28, test=91. The manifest is per file and covers all of `kernel/src`, not only `{dev,acpi,fs}`, so every new kernel file needs an entry.
  - The guard is `scripts/check-kernel-residents.py`. CI runs it together with `--negative-control`, which injects a synthetic unlisted `kernel/src/dev/` file.
  - A real unlisted `kernel/src/dev/zz_unlisted.rs` also failed (rc=1) when tried locally.
  - None of the 91 `test` modules has a symbol in the x86 release kernel. Checked with `nm`; module paths are derived from file paths.
- **Surprise → fix.** `bench-counters` (measurement-only) was not in `FORBIDDEN_IN_RELEASE`. It is added in its own commit, and the build refuses `release-posture,bench-counters`.
- **Spec 15 amendment (owner decision on pipes/futex/sessions).** The amendment note covers:
  - §2 A gains rows for syscall entry and for IPC with its kernel objects.
  - New §2.5 points to the manifest as the authoritative exception list.
  - §3.1 exceptions are updated to the `dev/` paths and now list the real remaining device residents: ps2, the in-kernel text console, the input IRQ shim and the null block device. The `fb_console ✅` row was wrong and is corrected.
  - §3.2 records the snapshot gate.
  - §3.5 states the release-posture rule and the crypto known-answer-test carve-out.
