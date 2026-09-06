---
phase: 1
title: "Canonical CWD Paths"
status: completed
priority: P1
dependencies: []
tier: thinking
---

# Phase 01: Canonical CWD Paths

## Context Links

- [Parent plan](./plan.md)
- `kernel/src/task.rs:1439-1499,1584-1642` — resolver, open/remove paths, and cwd stubs.
- `kernel/src/task/syscall.rs:3831-3900,4672-4687,6494-6511` — caller ID, user-copy boundary, and syscall 107/108 ABI.
- `kernel/src/task/tcb.rs:209-210,500-502` — existing per-task `cwd`, initialized to `/`.
- `libs/api/src/services/fs.rs:23-68` and `kernel/src/fs/fat.rs:220-293` — `Stat` contract and VIFS1 FAT backend.
- `kernel/src/main.rs:692-741` and `scripts/{build-test-hooks-ci.sh,qemu-boot-test.sh,assert-boot-markers.sh}` — mounted-FS test window and RV64 evidence path.
- History: `38386260` introduced the CWD field/resolver/stubs; `4f09094f` left `chdir` as unconditional success.

## Overview

Completed this vertical kernel slice: canonicalized task-relative paths, routed `open`/`remove` through that resolver, validated and committed `chdir`, and returned `getcwd` atomically under the existing raw ABI. Added only the FAT metadata required by `VIFS1.stat` and a deterministic boot self-test.

## Key Insights

- `current_task_mut()` is per-hart state; syscall `caller_id` is the stable initiator and must select every CWD read/update and final FD insertion.
- The existing `ViFileSystem::stat` default is `NotSupported`; `ViFatFS` needs a real override before `chdir` can distinguish file, directory, and absence.
- Lock safety requires three disjoint regions: snapshot under `SCHEDULER`, validate under `VIFS1`, then commit under `SCHEDULER`; never nest the two locks.
- The existing getcwd staging buffer already prevents user writes before success. Preserve it and copy exactly `cwd.as_bytes()` with no invented trailing NUL.

## Requirements

- Reject only empty path input at the resolver boundary; produce `/` or a canonical absolute path.
- Ignore empty and `.` components, pop one component for `..`, and saturate at root.
- Resolve absolute input independently of CWD; resolve relative input from the initiating task's canonical CWD.
- Make `file_open`, `file_remove`, `file_chdir`, and `file_getcwd` take `caller_id`; migrate every syscall caller.
- `chdir` succeeds only when `VIFS1.stat` reports `exists && is_dir`; failed validation leaves CWD unchanged.
- `getcwd` fails before copying if the destination is too small; on success return the byte count and touch only that prefix.
- Preserve syscall IDs 107/108, `(ptr,len)` arguments, current error mapping, `MAX_LOG_MSG`/`MAX_USER_BUF`, and user-copy helpers.

## Architecture

`syscall caller_id → snapshot/resolve under SCHEDULER → release → VIFS1 operation → release → caller-specific commit when required`.

Implement one allocation-conscious lexical resolver in `task.rs`: reuse a single output `String`, append normal components, and truncate to the previous slash for `..` rather than building a second component vector. Absolute paths still require the caller task to exist, but need not clone its CWD. `open` reacquires `SCHEDULER` only to install the handle into that caller; `remove` needs no commit; `chdir` commits the validated string; `getcwd` snapshots bytes and returns before the syscall writes user memory.

`ViFatFS::stat` holds only its inner FAT lock: root is an existing directory of size zero; a file returns its end position; a directory returns size zero; a miss returns `ViError::NotFound`.

## Related Code Files

| File | Action | Symbols / purpose |
|---|---|---|
| `kernel/src/task.rs` | Modify | `resolve_path`, `file_open`, `file_remove`, `file_chdir`, `file_getcwd` |
| `kernel/src/task/syscall.rs` | Modify | pass `caller_id` in `Open`, `FileOp::remove`, `ChDir`, and `GetCwd` arms |
| `kernel/src/fs/fat.rs` | Modify | add `ViFatFS::stat` using existing `api::fs::Stat` |
| `kernel/src/task/path_selftest.rs` | Create | deterministic resolver, stat, caller isolation, and exact-copy cases |
| `kernel/src/task.rs`, `kernel/src/main.rs` | Modify | register/run the test-hook self-test before secondary harts start |
| `docs/project-changelog.md` | Modify | record only the verified bounded CWD/path claim |
| `kernel/src/task/tcb.rs`, `libs/api/src/services/fs.rs` | Read only | reuse `Task.cwd` and the existing trait/ABI contracts |

## Implementation Steps

1. Replace `resolve_path` with a fallible canonicalizer plus caller-aware snapshot helper; keep one canonical representation and one implementation.
2. Pass `caller_id` from all four syscall paths. Resolve and install `open` handles against the same caller, and route `remove` through the identical helper.
3. Implement `ViFatFS::stat` for root, regular files, directories, and missing paths without changing the trait or public ABI.
4. Implement `file_chdir(caller_id, path)`: resolve, release scheduler, stat under VIFS1, reject absent/non-directory, release VIFS1, then update only `tasks.get_mut(&caller_id)`.
5. Implement `file_getcwd(caller_id, buf)`: snapshot that task, check capacity before mutation, exact-copy bytes, return length; retain syscall staging so failed calls never reach `write_user_slice`.
6. Add `path_selftest`: table-test absolute/relative normalization, repeated slash, `.`, root-saturating `..`, empty rejection, FAT stat classes, failed-chdir immutability, two-task isolation, and exact/oversize/undersize getcwd sentinel behavior.
7. Register the self-test in the existing single-hart post-`task::init`, post-`fs::init` test-hook window and emit one stable `cwd-path self-test PASS` marker; teardown synthetic tasks/FDs.
8. Run baseline-relative gates, inspect the QEMU marker, review lock scopes/callsites, then add the narrow changelog entry.

## Todo List

- [x] Record baseline command results before implementation edits.
- [x] Implement canonical resolver and caller-specific file/CWD operations.
- [x] Implement FAT `stat` and deterministic kernel self-test.
- [x] Obtain paired green post-change non-test-hooks boot and test-hooks CWD/path-marker evidence.
- [x] Finalize exclusions, changelog, and roadmap status after the global boot gate is green.

## Baseline and Acceptance Commands

Baseline, before implementation:

```sh
cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf
cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf
BOOT_WINDOW=55 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel
```

Acceptance, after implementation:

```sh
cargo fmt --all -- --check
cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf
bash scripts/build-test-hooks-ci.sh
BOOT_WINDOW=55 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks
bash scripts/assert-boot-markers.sh qemu.log cwd-path "kernel CWD/path self-test:::cwd-path self-test PASS"
```

## Success Criteria

- [x] Resolver cases are deterministic and every successful output is canonical, absolute, and root-contained.
- [x] `open`/`remove` share the resolver; `chdir`/`getcwd` address `caller_id`, not per-hart current state.
- [x] No code path holds `SCHEDULER` and `VIFS1` simultaneously; failed `chdir` and undersized `getcwd` mutate nothing.
- [x] Raw syscall 107/108 ABI and error/user-copy boundaries remain unchanged.
- [x] Pair an exit-0 post-change non-test-hooks `qemu-boot-test` with the passing immutable-FAT test-hooks CWD/path oracle.
- [x] Final documentation says bounded kernel CWD/path slice, never POSIX complete.

## Completion Evidence

The resolver is canonical, absolute, lexical, and root-saturating. Relative `open`, `remove`, `chdir`, and `getcwd` are attributed by syscall `caller_id`; `chdir` validates an existing directory before a failure-atomic commit, while `getcwd` copies exactly the path bytes within bounds and adds no NUL. VIFS1 FAT `stat` distinguishes root, regular file, directory, and missing path. Source review found the implementation correct; formatting, RV64 check, clippy, and build gates passed, and the generated init artifact was restored and excluded.

The post-change release build exited 0, then the non-test-hooks `qemu-boot-test` exited 0 with exact `PASS: shell prompt reached — full boot successful` and zero panic or Cell faults. The fresh immutable-FAT test-hooks build exited 0; `assert-boot-markers.sh` exited 0 with exactly one `cwd-path self-test PASS` and no CWD failure, panic, or unclassified fault. The test-hooks generic runner alone exits 1 under the unchanged global policy for the exact deliberate classified `stack_overflow_probe` Cell 254 fault (`cause=0xf`, `pc=0x12000`, address matching armed target `0x8288cff8`) while boot continues to the shell. Paired with the green release boot, that classified self-test fault does not widen the bounded CWD/path claim.

## Risk Assessment

- FAT metadata errors can collapse absence and I/O failure if mapped loosely; return `NotFound` only for a true miss and preserve other backend errors.
- Stat-then-commit is intentionally non-atomic to avoid lock inversion. VIFS1 is the current validation authority; any future mutable namespace/path-handle design must revisit this TOCTOU boundary.
- Task disappearance between snapshot and commit must fail without updating another task or leaking an opened handle. Caller-ID reuse is outside this synchronous syscall slice and must not be broadened here.
- A boot script can pass on the FAT mount alone; the separate literal marker assertion is required evidence for this behavior.

## Security Considerations

Lexical `..` must never escape `/`; empty paths fail closed. User pointers remain behind `read_user_string`, `validate_user_buf`, and `write_user_slice`. CWD state and opened FDs must be attributed to the initiating task on SMP. No canonicalizer may grant access, bypass VIFS1, reinterpret symlinks, or expand the syscall allowlist. Exact-copy failure must expose no partial path bytes.

## Explicit Exclusions

No `fstat`, `rename`, `mkdir`, shell `cd`/`pwd`, C wrapper, new `ViSyscall` enum value, ABI/error-code redesign, symlink resolution, mount namespace, process inheritance redesign, or broad POSIX completeness claim.

## Next Steps

This phase is complete. Any `fstat`, `rename`, shell `cd`/`pwd`, C-wrapper, symlink, new-ABI, or broader POSIX work requires a separate approved lane.
