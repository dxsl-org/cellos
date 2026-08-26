# Phase 02 — modvfs.c Rewrite + stat/listdir/remove

**Priority:** P0 · **Status:** pending · **Effort:** ~4h · **Depends:** Phase 01

## Context Links
- File to rewrite: `cells/runtimes/micropython/src/c/ViCell/modvfs.c` (canonical casing per `build.rs:7,123`)
- Externs provided by: `cells/runtimes/micropython/src/vfs_bridge.rs` (Phase 01)
- Module-table reference: current `modvfs.c:158-173`
- QSTR evidence: `src/c/genhdr/qstrdefs.generated.h:1062 (stat), 841 (listdir), 131 (remove)`

## Overview
Replace all raw byte-opcode IPC in `modvfs.c` with calls to the `ViCell_vfs_*` bridge.
Add three Python functions: `vfs.stat`, `vfs.listdir`, `vfs.remove`. No qstr regen required.

## Key Insights
- **QSTR RISK RESOLVED (re-grepped):** `MP_QSTR_stat/listdir/remove` already exist in the generated
  header — adding module-table entries needs NO `qstrdefsport.h` edit and NO gen_genhdr re-run.
  (Also note `build.rs:15` only regenerates if `qstrdefs.generated.h` is MISSING — it exists.)
- All raw helpers (`write_le16`, `zero_scan`, `vfs_write_chunk`, `vfs_write_impl`) and constants
  (`OP_*`, `MAX_CHUNK`, `MAX_PATH`) become dead — delete them.
- Keep the `ViCell_net_*` extern decls only if still used elsewhere in this file — after rewrite they
  are NOT used here; remove them from modvfs.c (they stay defined in net_bridge.rs for modvnet.c).
- `vfs.listdir` parsing happens C-side: split the `"d:name\nf:name\n"` buffer on `\n` into a Python list.

## Data Flow
```
vfs.read(path)    → ViCell_vfs_read → mp_obj_new_str(s_read_buf, n)
vfs.write(p,d)    → ViCell_vfs_write → mp_const_true/false
vfs.stat(path)    → ViCell_vfs_stat → (size, is_dir) tuple | None
vfs.listdir(path) → ViCell_vfs_listdir → ["d:name", "f:name", ...] | None
vfs.remove(path)  → ViCell_vfs_remove → bool
```

## Requirements
**Functional**
- `read/write/append/mkdir` behavior unchanged from Python's view (str|None, bool).
- New: `stat(path) -> (size:int, is_dir:bool) | None`; `listdir(path) -> list[str] | None`; `remove(path) -> bool`.
**Non-functional**
- Compiles under `-std=c99 -Wno-implicit-function-declaration` (build.rs flags).
- Static buffers: `s_read_buf[64*1024]`, `s_listdir_buf[512]` (single-threaded cell — safe).

## Related Code Files
**Modify:** `cells/runtimes/micropython/src/c/ViCell/modvfs.c` (full rewrite of body + module table)
**No new files; no build.rs change** (already lists modvfs.c at `build.rs:123` + rerun-if-changed `:150`).

## Implementation Steps
1. Replace header comment block describing typed-IPC bridge.
2. Replace `ViCell_net_*` externs with the 7 `ViCell_vfs_*` externs:
   ```c
   extern size_t ViCell_vfs_read(const uint8_t *path, size_t pl, uint8_t *out, size_t out_size);
   extern int    ViCell_vfs_write(const uint8_t *path, size_t pl, const uint8_t *data, size_t dl);
   extern int    ViCell_vfs_append(const uint8_t *path, size_t pl, const uint8_t *data, size_t dl);
   extern int    ViCell_vfs_mkdir(const uint8_t *path, size_t pl);
   extern int    ViCell_vfs_stat(const uint8_t *path, size_t pl, uint64_t *size_out, int *is_dir_out);
   extern size_t ViCell_vfs_listdir(const uint8_t *path, size_t pl, uint8_t *out, size_t out_size);
   extern int    ViCell_vfs_remove(const uint8_t *path, size_t pl);
   ```
3. Delete `OP_*`, `MAX_CHUNK`, `MAX_PATH`, `READ_BUF_SIZE`, `write_le16`, `zero_scan`,
   `vfs_write_chunk`, `vfs_write_impl`.
4. Rewrite `vfs_read` (static `s_read_buf[64*1024]`, call bridge, `mp_obj_new_str` or `mp_const_none`).
5. Rewrite `vfs_write`/`vfs_append` as thin wrappers over `ViCell_vfs_write`/`_append` (return bool).
6. Rewrite `vfs_mkdir` over `ViCell_vfs_mkdir`.
7. Add `vfs_stat`: call bridge; on success build 2-tuple `(mp_obj_new_int_from_uint(size), mp_obj_new_bool(is_dir))`
   via `mp_obj_new_tuple(2, items)`; else `mp_const_none`. `MP_DEFINE_CONST_FUN_OBJ_1`.
8. Add `vfs_listdir`: call bridge into `s_listdir_buf[512]`; on n>0 build list with `mp_obj_new_list(0,NULL)`,
   iterate buffer splitting on `\n` (track start; on `\n` push `mp_obj_new_str(start, len)` if len>0,
   advance start); push trailing segment. Return list or `mp_const_none`. `MP_DEFINE_CONST_FUN_OBJ_1`.
9. Add `vfs_remove`: call bridge, return bool. `MP_DEFINE_CONST_FUN_OBJ_1`.
10. Extend module table with `{ MP_ROM_QSTR(MP_QSTR_stat), MP_ROM_PTR(&vfs_stat_obj) }`,
    `..._listdir...`, `..._remove...`.
11. `cargo build -p micropython` — confirm cc compiles modvfs.c with no implicit-decl errors.

## Todo
- [ ] Swap externs (net_* → vfs_*); delete dead constants + helpers
- [ ] Rewrite read/write/append/mkdir over bridge
- [ ] Add vfs_stat (tuple) + obj macro
- [ ] Add vfs_listdir (`\n`-split → list) + obj macro
- [ ] Add vfs_remove + obj macro
- [ ] Add 3 module-table entries (QSTRs already exist)
- [ ] `cargo build -p micropython` clean

## Success Criteria
- modvfs.c compiles; no `OP_READ`/`OP_WRITE`/`OP_MKDIR`/`OP_APPEND`/`vfs_write_chunk` tokens remain (grep).
- `vfs` module exposes 7 functions: read, write, append, mkdir, stat, listdir, remove.
- Link succeeds (bridge symbols resolved from vfs_bridge.rs).

## Risk Assessment
- **R1 — listdir C parse (MED×MED):** off-by-one on final segment / empty buffer. Mitigation: handle
  n==0 → None before loop; only push segments with len>0; cross-check against `bindings_vfs.rs:295-303` logic.
- **R2 — implicit decl if extern signature drifts (LOW×HIGH):** `-Wno-implicit-function-declaration` would
  HIDE a missing extern and link-fail later. Mitigation: copy signatures verbatim from Phase 01; verify link step.
- **R3 — 64 KB static buffer (LOW×LOW):** added to .bss; cell GC heap is 256 KB separate — acceptable.

## Security Considerations
- `mp_obj_str_get_data` gives Python-owned bytes; passed read-only to bridge — no mutation of Python strings.
- Path length passed as-is; VFS cell enforces path limits (bridge truncation only on read/listdir output).

## Next Steps
Phase 03 validates end-to-end via integration tests in QEMU.
