---
title: "Canonical CWD and Path Slice"
description: "Completed bounded per-task canonical path resolution, chdir, getcwd, and VIFS1 FAT stat."
status: completed
priority: P1
branch: main
tags: [kernel, path, cwd, rv64]
blockedBy: []
blocks: []
created: 2026-09-02
---

# Canonical CWD and Path Slice

## Overview

Completed the reserved kernel CWD/path lane: one canonical absolute lexical resolver now serves caller-attributed relative `open`, `remove`, `chdir`, and `getcwd`; `chdir` validates through VIFS1 without nesting scheduler and filesystem locks.

## Scope Contract

- Canonical output is absolute; repeated `/` and `.` collapse, `..` saturates at `/`, and empty input fails.
- `chdir` validates existence plus directory type, then changes only the syscall initiator's `Task.cwd`.
- `getcwd` copies exactly the canonical bytes, returns their count, and performs no partial user copy.
- VIFS1 FAT `stat` distinguishes root, files, directories, and missing paths for this validation lane.
- Preserve raw syscall 107/108 arguments, results, bounds, and user-copy helpers.
- Evidence is deterministic kernel self-test, RV64 build, and QEMU marker proof; claims remain narrower than POSIX completeness.

## Boundaries

- Exclude `fstat`, `rename`, shell `cd`/`pwd`, C wrappers, symlinks/mount namespaces, broad POSIX behavior, and any new public ABI.
- Do not hold `SCHEDULER` and `VIFS1` together or identify a caller through mutable per-hart "current task" state.
- Do not add a second resolver, compatibility shim, new syscall number, or NUL-terminated `getcwd` contract.

## Phases

| Phase | Work | Status |
|---|---|---|
| 01 | [Canonical CWD paths](./phase-01-canonical-cwd-paths.md) | completed |

## Dependencies

- No cross-plan dependency. Reuse `Task.cwd`, `ViFileSystem::stat`, syscall IDs 107/108, existing user-copy limits, and boot self-test/QEMU runners.

## Completion Evidence

Source review found the canonical resolver, caller-ID cutover, failure-atomic `chdir`, exact bounded non-NUL `getcwd`, and VIFS1 FAT root/file/directory/missing stat implementation correct. Formatting, RV64 check, clippy, and build gates passed; the generated init artifact was restored/excluded. The post-change release build and non-test-hooks `qemu-boot-test` both exited 0 with the exact full-boot shell PASS and zero panic or Cell faults. A fresh immutable-FAT test-hooks build passed; its dedicated marker assertion exited 0 with exactly one `cwd-path self-test PASS` and no CWD failure, panic, or unclassified fault. Its generic boot runner still exits 1 solely under the unchanged global policy for the deliberate classified `stack_overflow_probe` Cell 254 fault matching the armed target while boot continues to the shell. This paired evidence closes only the bounded lane; `fstat`, `rename`, shell commands, C wrappers, symlinks, new ABI work, and broad POSIX compatibility remain future work.
