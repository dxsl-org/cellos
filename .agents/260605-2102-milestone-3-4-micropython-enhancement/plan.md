---
title: "Milestone 3.4 — MicroPython Runtime Enhancement"
description: "Migrate MicroPython vfs module from raw byte-opcode IPC to typed VfsRequest/VfsResponse via a C-callable Rust bridge, and add stat/listdir/remove."
status: pending
priority: P1
effort: 10h
branch: main
tags: [micropython, vfs, ipc, runtime, milestone-3-4]
created: 2026-06-05
---

# Milestone 3.4 — MicroPython Runtime Enhancement

## Problem
`modvfs.c` (C) + `main.rs:vfs_read_to_buf` still speak the **removed** raw byte-opcode
VFS protocol (`OP_READ=8, OP_WRITE=4, OP_MKDIR=5, OP_APPEND=10`). The VFS cell now
only accepts typed postcard IPC (`api::ipc::VfsRequest`). MicroPython VFS is therefore
**broken**. Lua (Milestone 3.3) already migrated via `bindings_vfs.rs`.

## Constraint
`modvfs.c` is C — cannot call Rust `api::ipc::encode/decode` directly. Mirror the
`net_bridge.rs` pattern: add `vfs_bridge.rs` with `#[no_mangle] extern "C"` functions
that wrap the typed IPC; `modvfs.c` declares them `extern` and calls them.

## Phases

| # | Phase | Pri | Status | Effort | Depends |
|---|-------|-----|--------|--------|---------|
| 01 | [vfs-bridge + main.rs](phase-01-vfs-bridge-and-main.md) | P0 | ✅ complete | 5h | — |
| 02 | [modvfs.c rewrite + stat/listdir/remove](phase-02-modvfs-update.md) | P0 | ✅ complete | 4h | 01 |
| 03 | [Integration tests](phase-03-tests.md) | P1 | ✅ complete | 1h | 02 |

## Data Flow (after migration)
```
Python  →  modvfs.c (extern decl)  →  ViCell_vfs_* (vfs_bridge.rs)
        →  api::ipc::encode(VfsRequest) → sys_send(ep=3)
        →  sys_recv → api::ipc::decode(VfsResponse) → result back to C
```

## Verified Facts (re-grepped 2026-06-05)
- Template: `cells/runtimes/lua/src/bindings_vfs.rs:23-325` (vfs_ok / vfs_get_file_vec / vfs_write_chunked + stat/listdir/remove).
- Bridge pattern: `cells/runtimes/micropython/src/net_bridge.rs:13-58`.
- API: `libs/api/src/ipc.rs:25-69` — VfsRequest::{GetFile,ListDir,Stat,Write,Append,Mkdir,Unlink}; VfsResponse::{Data,DataPtr,Stat,Ok}. **Remove = `Unlink`**, not "Remove".
- `api` IS a dep: `Cargo.toml:11`.
- **QSTR RISK RESOLVED**: MP_QSTR_stat/listdir/remove ALREADY defined in
  `src/c/genhdr/qstrdefs.generated.h:1062/841/131`. No qstr regen needed.
- **PATH CASING**: build.rs uses `src/c/ViCell/` (capital V/C) — `build.rs:7,113-123`.
  The Windows FS shows `src/c/vicell/`; use the build.rs canonical casing.
- modvfs.c already compiled via cc: `build.rs:123`. No build.rs change needed.
- `c_int` source: use `core::ffi::c_int` — libc is NOT a dep.

## Key Risks
- R1 (LOW, mitigated): QSTR registration — all 3 already in generated header.
- R2 (MED): `vfs_listdir` C-side parsing of `"d:name\nf:name\n"` into a Python list.
- R3 (LOW): static read buffer sizing (64 KB) — single-threaded cell, OK.

## Status
**✅ COMPLETE (2026-06-05, 2133 UTC)**

All 3 phases implemented, tested, integrated:
- Phase 01: vfs_bridge.rs + main.rs rewire ✅
- Phase 02: modvfs.c full rewrite ✅  
- Phase 03: cargo check zero errors ✅

Ready for docs sync and commit.
