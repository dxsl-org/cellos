---
title: "Phase H: VFS Capability & Write Hardening"
description: "KernelPerms bitflags, POSIX type guards, recursive rmdir, and OP_APPEND for the FAT16 VFS service."
status: pending
priority: P2
effort: 5h
branch: main
tags: [vfs, fat16, kernel, capabilities, posix, shell]
created: 2026-06-03
---

# Phase H: VFS Capability & Write Hardening

Phases C–G built a complete FAT16 filesystem on the VirtIO disk. Phase H closes the four
remaining correctness and security gaps without touching the Law 1 ABI surface
(`libs/api/`, `libs/types/`).

## Goal

1. Replace single-purpose `can_block_io: bool` with a kernel-internal `KernelPerms(u32)` bitfield.
2. Enforce POSIX type semantics: `rmdir` only removes dirs, `unlink` only removes files.
3. Add recursive directory removal (`OP_RMDIR_RECURSIVE` + shell `rm -r`).
4. Add `OP_APPEND` so writes > 508 bytes can be chunked onto `/data/` files.

## Phases

| # | Phase | Crates touched | Status | Depends on |
|---|-------|----------------|--------|------------|
| 1 | [KernelPerms bitflags](phase-01-kernel-perms-bitflag.md) | `ViCell-kernel` | pending | — |
| 2 | [POSIX type checking](phase-02-posix-type-checking.md) | `service-vfs` | pending | — |
| 3 | [Recursive rmdir](phase-03-recursive-rmdir.md) | `service-vfs`, `shell`, tests | pending | Phase 2 |
| 4 | [OP_APPEND](phase-04-op-append.md) | `service-vfs`, `shell`, tests | pending | — |

## Dependency graph

```
Phase 1 (KernelPerms)   — independent (kernel-only)
Phase 2 (type checking) — independent (vfs-only, no opcode change)
Phase 3 (recursive rm)  — depends on Phase 2 (rmdir_fat16 reused in recursive path)
Phase 4 (OP_APPEND)     — independent (new opcode 10, new helper)
```

**Apply order: 1 → 2 → 3 → 4.** Phases 1 + 2 may run in parallel (disjoint crates,
disjoint files). Phase 3 must follow Phase 2. Phase 4 is independent of all.

## File ownership (no two parallel phases touch the same file)

| File | Owned by |
|------|----------|
| `kernel/src/task/tcb.rs` | Phase 1 only |
| `kernel/src/loader.rs` | Phase 1 only |
| `kernel/src/task/syscall.rs` | Phase 1 only |
| `cells/services/vfs/src/main.rs` | Phase 2, then 3, then 4 (serialized) |
| `cells/apps/shell/src/cmd_fs.rs` | Phase 3, then 4 (serialized) |
| `tests/integration/tests/boot.rs` | Phase 3, then 4 (serialized) |

`main.rs`, `cmd_fs.rs`, and `boot.rs` are each touched by multiple phases — these phases
MUST be serialized (3 after 2; 4 after 3) to avoid edit conflicts, even though their logic
is independent.

## Opcode allocation (verified cmd_fs.rs:16-18, main.rs:32-39)

Existing: `OP_GET_FILE=1, OP_LIST_DIR=2, OP_STAT=3, OP_WRITE=4, OP_MKDIR=5, OP_RMDIR=6, OP_UNLINK=7, OP_READ=8`.
New: `OP_RMDIR_RECURSIVE=9` (Phase 3), `OP_APPEND=10` (Phase 4).

## Verification per phase

Each phase ends with `cargo check -p <crate>`. Phases 3 and 4 add a QEMU integration test
(`QemuRunner`, `CMD_TIMEOUT=10`). Final acceptance = full boot + all integration tests green.

## Backwards compatibility

- **Opcodes 1–8 unchanged** — existing shell binary and VFS service stay wire-compatible.
- **`KernelPerms` is kernel-internal** — no Cell-facing ABI change; the VFS cell ELF is unchanged.
- **Disk format unchanged** — no FAT16 layout migration; existing `disk_v3.img` works as-is.
- **`rmdir`/`unlink` behavior tightens** — a script that relied on `rmdir file.txt` succeeding
  (a bug) will now get error `0x01`. This is the intended correctness fix; documented in changelog.

## Risk summary (full detail per phase)

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Recursive rmdir borrow-checker aliasing | High | Build break | Rebuild `root_dir()` per level, pass full rel paths, no nested handle (Phase 3) |
| `OP_APPEND` hits `BlockStream::seek(End)` Err | Low | Append fails | fatfs translates `File::seek(End)` → `disk.seek(Start)`; verified block_stream.rs:97 path unreachable (Phase 4) |
| `KernelPerms` field rename misses a caller | Low | Build break | Enumerated all 5 callers (Phase 1); compiler catches the rest |
| Type guard rejects legitimate ops | Low | Regression | `open_file`/`open_dir` probe matches fatfs entry kind exactly (Phase 2) |

## Out of scope

Full RBAC/capability delegation; `/tmp/` rmdir type-checking (RamFS already correct);
`cp -r`; streaming/async writes. `/tmp/` OP_APPEND is minimally included (read-extend-write)
but may be deferred if RamFS API proves awkward (decision recorded in Phase 4).

## Unresolved questions

See each phase file's "Unresolved questions" section.
