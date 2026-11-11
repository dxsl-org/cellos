# S4 handoff request — kernel shape (phase 08)

**Date:** 2026-11-11 · **Plan:** [plan.md](plan.md) · **Prior slice:** S3 closed 2026-11-11. S1 remains open only for the post-CI untrack and the signing-blocked release item; neither touches S4 files.

## 1. Scope

Phase 08: one declarative syscall table generates the hand-synchronised lists (the api enum, decode and allowlist bit; the kernel decode and reverse map; the C and Zig headers). One allowlist gate, and `task/syscall.rs` (14 295 lines) split into domain modules with move-only commits. Law-1 CP1 is attached: [cp1-syscall-table.md](cp1-syscall-table.md). Phase 09 belongs to S4 as well, but is requested separately once Phase 08 closes.

## 2. File set (file-bounded exception)

- **New:**
  - `libs/api/abi/syscalls.toml`, `libs/api/abi/syscalls.golden.json`, `libs/api/build.rs`
  - `scripts/gen-syscall-headers.py`
  - `kernel/src/task/syscall/{mod,ipc,grant,spawn,fs,dev,vm,authority,misc}.rs`
  - `cp2-syscall-table.md`
- **Modify (Law-1 under CP1):** `libs/api/src/abi/syscall.rs`, `libs/api/Cargo.toml` (build script only).
- **Modify:**
  - `kernel/build.rs`, `libs/ostd/src/syscall.rs` (shared error decoding)
  - `libs/zig-syscall/src/syscall.zig`, `libs/port-platform/include/cellos_syscall.h`, `third_party/mlibc/sysdeps/vicell/include/vicell/syscall.h`
  - `scripts/check-{ai,lookupservicebound,multi-source-wait}-law1-digests.sh` (re-pin)
  - `.github/workflows/ci.yml` (header diff step), `scripts/lanes.toml` if a lane moves
  - every kernel file that calls `handle_syscall` directly (17 files; caller-origin token)
- **Delete:** `kernel/src/task/syscall.rs`, which the directory replaces.
- **All phases:** this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`, `docs/code-metrics.generated.md`, `scripts/mutable-globals.toml`, `scripts/unsafe-allowlist.toml` and `docs/generated/mutable-globals.md` (only for moved entries).

## 3. Behaviour changes the owner should expect

No syscall number, bit or encoding changes. Every in-kernel `handle_syscall` caller passes through the same allowlist gate, unless it carries an explicit kernel-origin token. A test-hooks selftest that relied on skipping the gate is either given a test token or fixed.

## 4. Irreversible or externally visible effects

None persistent. The C and Zig headers become generated files, with the same values.

## 5. Conflicts

- **iGPU DMA-export slice:** approved but not effective. It touches `task/syscall.rs` and `libs/api/src/abi/syscall.rs`, so it waits again. After the split, its edits land in the domain modules.
- **Commits:** one per step: golden dump → api generation → kernel generation and single gate → move-only split (several commits) → headers. Nothing is pushed.

## 6. Checkpoint

Phase 08 is not a checkpoint phase. Its acceptance is the CP2 evidence (CP1 §6) plus an independent review of the gate change.

## 7. Questions to the owner

1. Grant S4 for Phase 08 with the §2 file set.
2. Approve CP1.
