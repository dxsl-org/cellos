# S4b handoff request — kernel residents, selftests, module layout (phase 09)

**Date:** 2026-11-11 · **Plan:** [plan.md](plan.md) · **Prior slice:** S4 Phase 08 closed 2026-11-11 (Law-1 CP2 confirmed). S1 is still open, only for the post-CI untrack and the signing-blocked release item.

## 1. Scope (measured on `616120e2d`)

Phase 09 as planned, plus the dispatcher-match extraction moved here from Phase 08 by owner decision:

1. **Selftests out of production boots.** There are 37 `*selftest*` files, about 8.8k lines. `main.rs` runs them on every 64-bit boot, and a FAIL only logs. They move to `kernel/src/selftest/`, and both the module and the boot call block are gated on `test-hooks`.
2. **`snapshot.rs` behind its feature.** The file is 5 769 lines, always compiled, and gated only at runtime. It will compile only with `snapshot-qualified`. A stub answering `NotSupported` serves the 17 call sites outside the module.
3. **A checked resident list.** `kernel/residents.toml` names each resident with its Spec 15 category and migration target. `scripts/check-kernel-residents.py` runs in CI.
   - Spec 15 §2–§3 is amended to match reality: the PS/2, fb console, ACPI/SMBIOS and FD/cwd residents become `exception` entries with a target.
   - Pipes, futex and sessions are either admitted or scheduled to move. Either way it is your call, recorded in the amendment note.
4. **Directory hoist of `task/` (73 files) by responsibility.** Targets are `sched/`, `ipc/`, `dev/` (`task/drivers/**`) and `fsapi/`, one directory per commit, move-only.
5. **Dispatcher extraction.** The 5 212-line `match` in `task/syscall/mod.rs` becomes per-domain handler functions in the existing `syscall/{ipc,grant,spawn,fs,dev,vm,authority,misc}.rs`.
   - Each arm body moves into a function `fn <name>(caller_id, fields…) -> SyscallResult`. The arm calls it.
   - These commits are not move-only: control flow moves from `match` arms to functions. They are kept one domain per commit and reviewed with `git diff --color-moved`.
   - The golden test and the gate-equivalence test must stay green after each commit.
6. **Raw cfgs to features.** `--cfg cellos_p01_trace` and `cellos_g1_cpu` become named features, each confined to one module.
7. **Metric fix.** Code metrics exclude test code by cfg or module, not by file name. `Task` fields are grouped into sub-structs.

## 2. File set (file-bounded exception)

- **Create:**
  - `kernel/src/selftest/mod.rs`, `kernel/residents.toml`
  - `scripts/check-kernel-residents.py`
  - `kernel/src/{sched,ipc,dev,fsapi}/` (moved files)
- **Move:**
  - `kernel/src/**/*selftest*`
  - `kernel/src/task/drivers/**`
  - the IPC, scheduler, SMP, hart-local and file-API files under `kernel/src/task/`
- **Modify:**
  - kernel: `kernel/src/main.rs`, `kernel/src/task.rs`, `kernel/src/task/syscall/**`, `kernel/src/snapshot.rs` and its callers, `kernel/Cargo.toml` (features; drop `async-trait`), `kernel/build.rs`
  - every kernel file whose `use` paths change because of a move
  - scripts and CI: `scripts/generate-code-metrics.py`, `.github/workflows/ci.yml`, `scripts/lanes.toml`
  - lanes that grep selftest markers (`tests/integration/tests/boot.rs`, `scripts/qemu-phase09-ssh-test.sh`, `scripts/qemu-aarch64-test-hooks.sh`, `scripts/x86/qemu-domain-test.sh`, `scripts/qemu-native-domain-test.sh`, `scripts/qemu-phase02-session-test.sh`, `scripts/qemu-rpi4-test.sh`)
  - path-keyed inventories: `scripts/mutable-globals.toml`, `scripts/unsafe-allowlist.toml`, `docs/generated/mutable-globals.md`
  - Law-1 digest scripts, for kernel paths only
  - docs: `docs/specs/15-kernel-boundary.md`, `docs/code-metrics.generated.md`
- **Delete:** the `virtio_rng` no-op stub, the 21-line `nic.rs`, the stale `drivers.rs` header.
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`.
- **No `libs/api` / `libs/types` change.** If one turns out to be needed, it stops for CP1.

## 3. Behaviour changes the owner should expect

- **Default/release kernels stop running the boot selftests.** The default boot log has no `self-test` line, and no `*_selftest` symbol is left in a release kernel.
- **Lanes that read selftest markers switch to a test-hooks image.** These are the `boot.rs` selftest cases and the phase09-ssh `boot-ceiling` check; the domain and aarch64 test-hooks lanes already build test-hooks. One test-hooks boot lane per arch keeps the selftests on every PR.
- **Snapshot code is absent unless `snapshot-qualified` is on.** Callers get `NotSupported`, which they already get at runtime today.
- **Everything else** is file moves and function extraction, with no behaviour change.

## 4. Irreversible or externally visible effects

None persistent. Paths change; there is no ABI change.

## 5. Conflicts

- **The iGPU DMA-export slice** keeps waiting, since it touches `task/syscall/**` and `task/drivers/**`.
- **Your uncommitted UI work** (desktop, compositor, files, viui, `ostd/display.rs`) touches no S4b file. If you commit it first, nothing collides.

## 6. Checkpoint

Phase 09 is a checkpoint phase. It closes with:
- the full local lane set on three arches, including the test-hooks boot lanes;
- `x86-c2c` and the release-posture boot;
- a symbol check on the release kernel;
- the negative control for the resident guard;
- an independent review.

Commits are made one per step, or per directory or domain inside steps 4–5.

## 7. Questions to the owner

1. Grant S4b with the §2 file set.
2. Spec 15: admit pipes, futex and sessions as kernel residents, or record them as `exception` with a move target (a cell)?
3. Order: run dispatcher extraction (step 5) before the directory hoist (step 4), so the domain functions land in `syscall/` before files move?
