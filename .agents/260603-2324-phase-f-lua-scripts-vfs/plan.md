---
title: "ViCell Phase F — Lua Script Files + vfs.* Filesystem Bindings"
description: "Run .lua files from VFS and expose vfs.read/write/append/mkdir to Lua scripts"
status: pending
priority: P2
effort: 4h
branch: main
tags: [lua, vfs, ipc, runtime, phase-f]
created: 2026-06-03
---

# Phase F — Lua Script Files + vfs.* Bindings

Two-phase delivery. Phase F.1 lets `lua /data/script.lua` load a file from the VFS
cell and execute it. Phase F.2 exposes a new `vfs.*` Lua table so scripts can do
file I/O against `/data/` (FAT16) and `/tmp/` (RamFS) over the same VFS IPC the
shell already uses.

All IPC wire formats are copied verbatim from the working shell built-ins in
`cells/apps/shell/src/cmd_fs.rs` — no new VFS protocol is introduced.

## Phases

| # | Phase | File | Status | Effort | Blockers |
|---|-------|------|--------|--------|----------|
| F.1 | Lua script file loading | [phase-01-lua-script-loading.md](phase-01-lua-script-loading.md) | pending | 2h | none |
| F.2 | `vfs.*` Lua table | [phase-02-lua-vfs-bindings.md](phase-02-lua-vfs-bindings.md) | pending | 2h | F.1 (shared main.rs edits: `extern crate alloc`) |

## Dependency Graph

```
F.1 (ffi.rs +luaL_loadbuffer, main.rs script branch + alloc)
  │  shares main.rs — F.2 layers mod + table registration on top
  ▼
F.2 (bindings_vfs.rs new file, main.rs vfs table)
```

F.1 must land first because both phases edit `main.rs` and F.1 introduces
`extern crate alloc;` that F.2's `alloc::vec!` calls depend on. Running them in
parallel would create a merge conflict in `main.rs` and a file-ownership clash.
**File ownership: F.1 and F.2 both touch `main.rs` and `boot.rs` — sequential only.**

## Data Flow (both phases)

```
Lua source / vfs.* call
   → bindings (Rust)  build [opcode][path_len][...]  byte request
   → sys_send(VFS_ENDPOINT=3, req)
   → VFS service cell (cells/services/vfs) handles op against FAT16 / RamFS
   → sys_recv(0, buf)   (returns sender_id, NOT length)
   → reply: OP_READ = raw bytes (zero-scan for length); OP_WRITE/APPEND/MKDIR = [0x00] ok
```

## Key Dependencies (verified)

- `VFS_ENDPOINT = 3` — cmd_fs.rs:15 (init=1, user_hello=2, vfs=3).
- `OP_READ=8` cmd_fs.rs:280, `OP_WRITE=4` :279, `OP_MKDIR=5` :16, `OP_APPEND=10` :20.
- `luaL_loadstring` ffi.rs:35 (loadbuffer NOT present — F.1 adds it).
- `lua_pushboolean` ffi.rs:74, `lua_pcallk` :44, `lua_tolstring` :57, `lua_settop` :67 — all present.
- `lua_arg_bytes(L, idx)` helper bindings_net.rs:42 — duplicate into bindings_vfs.rs (DRY note: separate bins, no shared lib module per Phase D).
- main.rs: `extern crate ostd; extern crate api;` (lines 4-5), NO `extern crate alloc` — F.1 adds it.
- Park loop `loop { ostd::task::yield_now(); }` main.rs:67-69 — all non-REPL paths must park.
- Test harness: `kernel_path` boot.rs:29, `disk_path` :36, `prerequisites_ok` :42, `CMD_TIMEOUT=10` :18, `BOOT_TIMEOUT=40` :16.
- `vwrite` built-in executor.rs:153 → cmd_fs.rs:324 — used by F.1 test fixture.
- `FAT16 /data volume mounted` boot string — real (vfs/src/main.rs), used in existing tests.

## Acceptance Criteria (whole phase)

1. `cargo check -p lua` → 0 warnings.
2. `lua_script_file` passes — prints `SCRIPT_OK` from a VFS-loaded `.lua` file.
3. `lua_vfs_write_read` passes — `HELLO_VFS` echoed via `vfs.write`/`vfs.read`.
4. All 25 prior integration tests still pass (no regression).

## File Inventory

**Modify:** `cells/runtimes/lua/src/ffi.rs`, `cells/runtimes/lua/src/main.rs`,
`tests/integration/tests/boot.rs`
**Create:** `cells/runtimes/lua/src/bindings_vfs.rs`

## 3-Task Rule

2 phases → task creation skipped.

## Unresolved Questions

- VFS receive-buffer size is unconfirmed. F.2 caps chunks at 480 bytes (subset of
  shell's `512 - 4 - path_len`) to stay conservative. If the VFS cell accepts
  larger frames, the cap can be lifted later — no correctness impact, only
  fewer round-trips.
