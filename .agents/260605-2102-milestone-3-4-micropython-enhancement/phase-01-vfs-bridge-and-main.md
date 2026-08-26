# Phase 01 — VFS Bridge + main.rs Migration

**Priority:** P0 (VFS currently broken) · **Status:** pending · **Effort:** ~5h · **Depends:** none

## Context Links
- Template (exact logic to mirror): `cells/runtimes/lua/src/bindings_vfs.rs:23-325`
- Bridge style: `cells/runtimes/micropython/src/net_bridge.rs:13-58`
- API types: `libs/api/src/ipc.rs:25-69`
- File to fix: `cells/runtimes/micropython/src/main.rs:28-46` (`vfs_read_to_buf` + `OP_READ` const)

## Overview
Create a C-callable Rust bridge exposing typed VFS IPC, then rewire `main.rs` to use it
(removing the raw `OP_READ` path). After this phase the Rust side speaks typed IPC; C side
is fixed in Phase 02.

## Key Insights
- `net_bridge.rs` proves the `#[no_mangle] extern "C"` + `from_raw_parts` pattern works for this cell.
- The bridge is a thin C-ABI veneer over the Lua helpers — copy `vfs_ok`, `vfs_get_file` (into-slice variant),
  `vfs_write_chunked` from `bindings_vfs.rs` verbatim; they have no Lua dependency except the `L`-stack glue (skip that).
- **Remove maps to `VfsRequest::Unlink`** (`ipc.rs:41`), NOT a "Remove" variant.
- `c_int` comes from `core::ffi` — no `libc` dep (`Cargo.toml` has only `api` + `ostd`).
- main.rs is `#![no_std]`; bridge must be too. Use `extern crate alloc;` only if Vec is needed
  (prefer into-slice helper for read to avoid alloc, matching `vfs_get_file`, not `vfs_get_file_vec`).

## Data Flow
```
C extern call → ViCell_vfs_read(path,pl,out,out_size)
  → vfs_get_file_into(path_str, &mut out[..])
    → encode(VfsRequest::GetFile) → sys_send(ep=3)
    → sys_recv → decode(VfsResponse::DataPtr{ptr,len})
    → copy_nonoverlapping(ptr, out, min(len,out.len)) → returns bytes copied
```

## Requirements
**Functional**
- C-callable: `ViCell_vfs_read/write/append/mkdir/stat/listdir/remove`.
- Read/listdir copy into a caller `*mut u8` buffer (no alloc across FFI) and return bytes written.
- write/append chunk at ≤400 bytes (`MAX_CHUNK`), Write for first chunk, Append for rest.
- stat writes `*size_out` (u64) + `*is_dir_out` (c_int 0/1), returns 1/0.
- remove = `VfsRequest::Unlink`.

**Non-functional**
- `#![forbid(unsafe_code)]` does NOT apply (bridge needs `unsafe` for FFI ptrs) — document each with `// SAFETY:`.
- Endpoint 3, frame 512, reply buffers 64B (Ok/Stat) or 512B (Data/DataPtr) — match Lua sizes.

## Related Code Files
**Create:** `cells/runtimes/micropython/src/vfs_bridge.rs`
**Modify:** `cells/runtimes/micropython/src/main.rs` (add `mod vfs_bridge;`; rewrite `vfs_read_to_buf`; delete `const OP_READ`)
**No build.rs change** (Rust module auto-compiled; modvfs.c already in cc list `build.rs:123`).

## Implementation Steps
1. Create `src/vfs_bridge.rs`. Header: `extern crate alloc;` omitted unless used. Add consts:
   `VFS_ENDPOINT=3`, `MAX_CHUNK=400`, `MAX_FILE_READ=64*1024`.
2. Port internal helpers from `bindings_vfs.rs`:
   - `pub(crate) fn vfs_ok(req: &api::ipc::VfsRequest) -> bool` — copy `bindings_vfs.rs:23-40`.
   - `pub(crate) fn vfs_get_file_into(path: &str, out: &mut [u8]) -> usize` — adapt `bindings_vfs.rs:55-86`
     (`vfs_get_file` is already the into-slice form; copy directly).
   - `fn vfs_write_chunked(path: &str, data: &[u8], append: bool) -> bool` — copy `bindings_vfs.rs:125-145`.
3. Add C exports (use `core::ffi::c_int`):
   - `ViCell_vfs_read(path,*const u8, pl, out *mut u8, out_size) -> usize`
   - `ViCell_vfs_write(path,pl, data,*const u8, dl) -> c_int`
   - `ViCell_vfs_append(path,pl, data, dl) -> c_int`
   - `ViCell_vfs_mkdir(path,pl) -> c_int`
   - `ViCell_vfs_stat(path,pl, size_out *mut u64, is_dir_out *mut c_int) -> c_int`
   - `ViCell_vfs_listdir(path,pl, out *mut u8, out_size) -> usize`
   - `ViCell_vfs_remove(path,pl) -> c_int`
   For each: `from_utf8(from_raw_parts(path,pl)).unwrap_or("")`; write* via `from_raw_parts(data,dl)`;
   read/listdir via `from_raw_parts_mut(out,out_size)`. Each `unsafe` block gets a `// SAFETY:` line.
4. `ViCell_vfs_stat`: encode `VfsRequest::Stat`, recv 64B, on `VfsResponse::Stat{size,is_dir}` write
   `*size_out=size; *is_dir_out = is_dir as c_int; return 1`; else `return 0`.
5. `ViCell_vfs_listdir`: encode `VfsRequest::ListDir`, recv 512B, on `VfsResponse::Data(bytes)`
   copy `min(bytes.len, out_size)` into `out`, return that count; else 0.
6. In `main.rs`: add `mod vfs_bridge;` after `mod net_bridge;` (line 7). Delete `const OP_READ` (line 11).
   Rewrite `vfs_read_to_buf` to call `vfs_bridge::vfs_get_file_into(path, buf)` (pub(crate), no FFI/unsafe needed).
7. `cargo build -p micropython --target riscv64gc-unknown-none-elf` (or project run script) — must compile clean.

## Todo
- [ ] Create `vfs_bridge.rs` with consts + 3 internal helpers
- [ ] Add 7 `#[no_mangle] extern "C"` exports with `// SAFETY:` docs
- [ ] `mod vfs_bridge;` in main.rs; delete `OP_READ`
- [ ] Rewrite `vfs_read_to_buf` to call `vfs_get_file_into`
- [ ] `cargo build` clean (no warnings on the new file)

## Success Criteria
- `cargo build -p micropython` succeeds; no `OP_READ`/raw-opcode references remain in main.rs.
- All 7 `ViCell_vfs_*` symbols present in the built object (`nm`/`rust-objdump` shows them, `T` global).
- `vfs_read_to_buf` resolves via `vfs_get_file_into` (typed IPC), confirmed by grep.

## Risk Assessment
- **R1 — Symbol name mismatch with C externs (MED×HIGH):** C decls in Phase 02 must match exactly.
  Mitigation: lock signatures here; Phase 02 copies them verbatim.
- **R2 — `DataPtr` lifetime (LOW×HIGH):** must copy bytes before next `sys_recv`. Mitigation: copy is
  inside the same match arm as recv, identical to `bindings_vfs.rs:72-79`.
- **R3 — alloc in no_std bridge (LOW×MED):** avoid by using into-slice read, not Vec.

## Security Considerations
- FFI ptr deref: every `from_raw_parts*` documented `// SAFETY:`; caller (C) guarantees buffer validity.
- `from_utf8(...).unwrap_or("")` — never panics on non-UTF8 path; empty path → VFS returns Err → 0/false.
- No path traversal handling here (VFS cell owns that boundary).

## Next Steps
Phase 02 rewrites `modvfs.c` to declare these externs and drop raw-opcode code.
