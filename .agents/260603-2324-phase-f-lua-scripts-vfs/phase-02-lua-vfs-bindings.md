# Phase F.2 — `vfs.*` Lua Table

## Context Links

- IPC pattern source of truth: `cells/apps/shell/src/cmd_fs.rs`
  (`read_file_vfs` :351, `write_file` :285, `append_file` :305, `vfs_path_op` :47)
- Binding template + `lua_arg_bytes`: `cells/runtimes/lua/src/bindings_net.rs` (:42)
- Table registration template: `cells/runtimes/lua/src/main.rs` (`vnet` :32-49)
- FFI: `cells/runtimes/lua/src/ffi.rs` (`lua_pushboolean` :74, `lua_pushlstring` :80, `lua_pushnil` :72)

## Overview

- **Priority:** P2
- **Status:** pending
- **Description:** New `bindings_vfs.rs` exposes a `vfs` global table to Lua scripts:
  `read`, `write`, `append`, `mkdir`. All four marshal to the VFS cell over the
  same IPC wire format the shell already uses.

## Key Insights

- Wire formats are copied verbatim from cmd_fs.rs — no new VFS protocol.
- `vfs.write`/`vfs.append` chunk content >480 bytes: first chunk via OP_WRITE,
  remaining chunks via OP_APPEND. Mirrors the shell's `write_file`+`append_file`
  pattern (cmd_fs.rs:285/305). 480 is a conservative subset of the shell's
  `512 - 4 - path_len`.
- `lua_arg_bytes(L, idx)` exists in bindings_net.rs:42 — duplicate it (~8 lines).
  Per Phase D, separate `bin` crates have no shared lib module, so DRY is satisfied
  at the design level; duplication is the accepted minimal cost.
- `lua_pushboolean` ffi.rs:74 already exported — no ffi.rs change for this phase.
- Each binding parks nothing — they return to Lua (`c_int` return = result count).
  Only `main`'s top-level execution paths park.

## Requirements

**Functional**
- `vfs.read(path)` → string (content) | nil (not found / empty).
- `vfs.write(path, content)` → bool (chunked for >480 bytes).
- `vfs.append(path, content)` → bool.
- `vfs.mkdir(path)` → bool.

**Non-functional**
- `cargo check -p lua` → 0 warnings.
- Each `extern "C"` binding upholds the `lua_CFunction` contract (returns the
  number of results pushed).

## Architecture

### Lua API Surface

```
vfs.read(path)            → string | nil
vfs.write(path, content)  → bool      (chunked: OP_WRITE then OP_APPEND)
vfs.append(path, content) → bool
vfs.mkdir(path)           → bool
```

### IPC Constants (mirror cmd_fs.rs exactly)

```rust
const VFS_ENDPOINT: usize = 3;   // cmd_fs.rs:15
const OP_WRITE:  u8 = 4;          // cmd_fs.rs:279
const OP_MKDIR:  u8 = 5;          // cmd_fs.rs:16
const OP_READ:   u8 = 8;          // cmd_fs.rs:280
const OP_APPEND: u8 = 10;         // cmd_fs.rs:20
```

### Data Flow (write, chunked)

```
vfs.write('/data/x', big)
  → vfs_op_write_chunk(OP_WRITE, path, big[..480])  → sys_send(3) → reply[0]==0?
  → while offset < len:
        vfs_op_write_chunk(OP_APPEND, path, big[offset..offset+480])
  → lua_pushboolean(L, ok) ; return 1
```

### Stack Discipline (registration)

Mirrors `vnet` (main.rs:32-49). `lua_createtable` pushes table @ -1; each
`pushcclosure` + `setfield` pair is net-zero; `setglobal` pops the table.
Net stack delta = 0.

## Related Code Files

**Create**
- `cells/runtimes/lua/src/bindings_vfs.rs` — the four bindings + helpers
  (`lua_arg_bytes` dup, `vfs_op_write_chunk`).

**Modify**
- `cells/runtimes/lua/src/main.rs` — add `mod bindings_vfs;` and register the
  `vfs` table (after the `vnet` registration block, before `sys_spawn_args`).

**Delete** — none.

## Implementation Steps

1. **Create `bindings_vfs.rs`** with header + `extern crate alloc;` + imports
   (mirror bindings_net.rs:1-14):
   ```rust
   //! VFS filesystem bindings exposed to Lua via C FFI (`vfs.*`).
   //! Mirrors the verified IPC wire format used by cmd_fs.rs: messages go to the
   //! VFS service cell (endpoint 3). sys_recv returns the SENDER id, not a byte
   //! count — reply length is bounded by the buffer we pass (zero-scan for reads).
   #![allow(non_snake_case)] // reason: L is the Lua C API convention

   extern crate alloc;

   use core::ffi::{c_char, c_int};
   use crate::ffi::{self, LuaState};
   use ostd::syscall::{sys_recv, sys_send, SyscallResult};

   const VFS_ENDPOINT: usize = 3;
   const OP_WRITE:  u8 = 4;
   const OP_MKDIR:  u8 = 5;
   const OP_READ:   u8 = 8;
   const OP_APPEND: u8 = 10;
   /// Conservative per-IPC content cap (subset of shell's 512 - 4 - path_len).
   const MAX_CHUNK: usize = 480;
   ```

2. **Duplicate `lua_arg_bytes`** (from bindings_net.rs:42) into bindings_vfs.rs:
   ```rust
   /// Read the string arg at stack `idx` as a byte slice borrowed from Lua.
   /// # Safety
   /// `L` must be valid; the slice lives only while the value stays on the stack.
   unsafe fn lua_arg_bytes<'a>(L: *mut LuaState, idx: c_int) -> Option<&'a [u8]> {
       let mut len: usize = 0;
       let ptr = unsafe { ffi::lua_tolstring(L, idx, &mut len as *mut _) };
       if ptr.is_null() { return None; }
       Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
   }
   ```

3. **`vfs_read`** binding (mirror read_file_vfs cmd_fs.rs:351):
   ```rust
   #[no_mangle]
   pub unsafe extern "C" fn vfs_read(L: *mut LuaState) -> c_int {
       let raw = match unsafe { lua_arg_bytes(L, 1) } {
           Some(b) => b, None => { unsafe { ffi::lua_pushnil(L) }; return 1; }
       };
       let path = core::str::from_utf8(raw).unwrap_or("");
       if path.is_empty() { unsafe { ffi::lua_pushnil(L) }; return 1; }
       let pb = path.as_bytes();
       let pl = pb.len().min(253) as u8;
       let mut req = [0u8; 256];
       req[0] = OP_READ; req[1] = pl;
       req[2..2 + pl as usize].copy_from_slice(&pb[..pl as usize]);
       sys_send(VFS_ENDPOINT, &req[..2 + pl as usize]);
       let mut buf = alloc::vec![0u8; 4096];
       match sys_recv(0, &mut buf) {
           SyscallResult::Ok(_) => {
               let n = buf.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
               if n == 0 { unsafe { ffi::lua_pushnil(L) }; return 1; }
               unsafe { ffi::lua_pushlstring(L, buf.as_ptr() as *const c_char, n) };
               1
           }
           _ => { unsafe { ffi::lua_pushnil(L) }; 1 }
       }
   }
   ```

4. **`vfs_op_write_chunk`** helper (one IPC round-trip; mirror write_file cmd_fs.rs:285):
   ```rust
   /// Send one OP_WRITE/OP_APPEND chunk. Returns true on reply[0]==0.
   fn vfs_op_write_chunk(opcode: u8, path: &[u8], content: &[u8]) -> bool {
       let pl = path.len().min(253);
       let cl = content.len().min(MAX_CHUNK.saturating_sub(pl));
       let mut buf = alloc::vec![0u8; 4 + pl + cl];
       buf[0] = opcode;
       buf[1] = pl as u8;
       buf[2..4].copy_from_slice(&(cl as u16).to_le_bytes());
       buf[4..4 + pl].copy_from_slice(&path[..pl]);
       buf[4 + pl..4 + pl + cl].copy_from_slice(&content[..cl]);
       sys_send(VFS_ENDPOINT, &buf);
       let mut r = [0u8; 1];
       match sys_recv(0, &mut r) { SyscallResult::Ok(_) => r[0] == 0, _ => false }
   }
   ```

5. **`vfs_write`** + **`vfs_append`** bindings. Both parse path+content via
   `lua_arg_bytes(L,1)`/`(L,2)`, then chunk. `vfs_write` uses OP_WRITE for the
   first chunk + OP_APPEND for the rest; `vfs_append` uses OP_APPEND throughout:
   ```rust
   #[no_mangle]
   pub unsafe extern "C" fn vfs_write(L: *mut LuaState) -> c_int {
       vfs_write_impl(L, OP_WRITE)
   }
   #[no_mangle]
   pub unsafe extern "C" fn vfs_append(L: *mut LuaState) -> c_int {
       vfs_write_impl(L, OP_APPEND)
   }
   /// Shared write/append driver. `first_op` is OP_WRITE (truncate) or OP_APPEND.
   /// Content > MAX_CHUNK is split: first chunk uses `first_op`, rest use OP_APPEND.
   fn vfs_write_impl(L: *mut LuaState, first_op: u8) -> c_int {
       let pb = match unsafe { lua_arg_bytes(L, 1) } {
           Some(b) => b, None => { unsafe { ffi::lua_pushboolean(L, 0) }; return 1; }
       };
       let content = unsafe { lua_arg_bytes(L, 2) }.unwrap_or(&[]);
       let pl = pb.len().min(253);
       let max_chunk = MAX_CHUNK.saturating_sub(pl).max(1);
       let first_len = content.len().min(max_chunk);
       let mut ok = vfs_op_write_chunk(first_op, pb, &content[..first_len]);
       let mut offset = first_len;
       while ok && offset < content.len() {
           let end = (offset + max_chunk).min(content.len());
           ok = vfs_op_write_chunk(OP_APPEND, pb, &content[offset..end]);
           offset = end;
       }
       unsafe { ffi::lua_pushboolean(L, if ok { 1 } else { 0 }) };
       1
   }
   ```
   > Note: borrowing `pb` and `content` simultaneously off the Lua stack is safe —
   > both values stay on the stack throughout (no pop). Matches bindings_net.rs usage.

6. **`vfs_mkdir`** binding (mirror vfs_path_op cmd_fs.rs:47):
   ```rust
   #[no_mangle]
   pub unsafe extern "C" fn vfs_mkdir(L: *mut LuaState) -> c_int {
       // [OP_MKDIR][path_len:u8][path_bytes] → reply [0x00] ok
       let pb = match unsafe { lua_arg_bytes(L, 1) } {
           Some(b) => b, None => { unsafe { ffi::lua_pushboolean(L, 0) }; return 1; }
       };
       let pl = pb.len().min(253);
       let mut req = [0u8; 256];
       req[0] = OP_MKDIR; req[1] = pl as u8;
       req[2..2 + pl].copy_from_slice(&pb[..pl]);
       sys_send(VFS_ENDPOINT, &req[..2 + pl]);
       let mut r = [0u8; 1];
       let ok = match sys_recv(0, &mut r) { SyscallResult::Ok(_) => r[0] == 0, _ => false };
       unsafe { ffi::lua_pushboolean(L, if ok { 1 } else { 0 }) };
       1
   }
   ```

7. **main.rs** — add `mod bindings_vfs;` alongside the other `mod` lines (:7-10).

8. **main.rs** — register the `vfs` table after the `vnet` block (after :49,
   before `sys_spawn_args` :52):
   ```rust
   // Register the `vfs` table (read/write/append/mkdir). Net stack delta = 0.
   // SAFETY: L is non-null; binding fns uphold the lua_CFunction contract.
   unsafe {
       ffi::lua_createtable(L, 0, 4);
       ffi::lua_pushcclosure(L, bindings_vfs::vfs_read, 0);
       ffi::lua_setfield(L, -2, c"read".as_ptr());
       ffi::lua_pushcclosure(L, bindings_vfs::vfs_write, 0);
       ffi::lua_setfield(L, -2, c"write".as_ptr());
       ffi::lua_pushcclosure(L, bindings_vfs::vfs_append, 0);
       ffi::lua_setfield(L, -2, c"append".as_ptr());
       ffi::lua_pushcclosure(L, bindings_vfs::vfs_mkdir, 0);
       ffi::lua_setfield(L, -2, c"mkdir".as_ptr());
       ffi::lua_setglobal(L, c"vfs".as_ptr());
   }
   ```

9. Run `cargo check -p lua` → 0 warnings.

10. **boot.rs** — add the integration test after `lua_script_file` (from F.1):
    ```rust
    /// Phase F.2: Lua `vfs.*` file I/O — write then read back from /data/.
    #[test]
    fn lua_vfs_write_read() {
        if !prerequisites_ok() { return; }
        let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
        qemu.wait_for("ViCell >", BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("prompt: {e}\n{}", qemu.dump()));
        assert!(qemu.output_contains("FAT16 /data volume mounted"), "FAT16 not mounted\n{}", qemu.dump());
        std::thread::sleep(std::time::Duration::from_millis(500));
        // Single -e expression: write then read. Adjacent Lua stmts, no semicolons,
        // no spaces inside the path/content strings (shell joins argv on space).
        qemu.send_line("lua -e vfs.write('/data/lua_vfs.txt','HELLO_VFS') print(vfs.read('/data/lua_vfs.txt'))");
        qemu.wait_for("HELLO_VFS", 15)
            .unwrap_or_else(|e| panic!("vfs roundtrip failed: {e}\n{}", qemu.dump()));
    }
    ```

## Todo List

- [ ] Create `bindings_vfs.rs` (header, consts, `lua_arg_bytes` dup)
- [ ] `vfs_read` binding
- [ ] `vfs_op_write_chunk` helper
- [ ] `vfs_write` / `vfs_append` (`vfs_write_impl` shared driver)
- [ ] `vfs_mkdir` binding
- [ ] main.rs: `mod bindings_vfs;`
- [ ] main.rs: register `vfs` table (after `vnet`)
- [ ] `cargo check -p lua` → 0 warnings
- [ ] boot.rs: add `lua_vfs_write_read` test
- [ ] Run `lua_vfs_write_read` → `HELLO_VFS`

## Success Criteria

- `cargo check -p lua` → 0 warnings.
- `lua_vfs_write_read` passes (`HELLO_VFS` echoed).
- `vfs.read` of a missing file returns `nil`.
- All 25 prior integration tests pass (no regression).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| VFS recv buffer < 480 → large writes dropped | Med | Med | 480 cap is subset of shell's working 512; chunked APPEND for larger |
| Double mutable-borrow of Lua stack (path + content) | Low | High | Both stay on stack, no pop; matches bindings_net.rs; immutable borrows |
| `vfs_write_impl` infinite loop if chunk len 0 | Low | High | `max_chunk = (...).max(1)` guarantees forward progress |
| Wrong opcode → silent route to wrong cell | Low | High | Constants copied verbatim from cmd_fs.rs with line citations |
| `c"read"` C-string literal unsupported by toolchain | Low | Med | Already used for `vnet` (main.rs:35) — proven |

## Backwards Compatibility

Additive only. New `vfs` global does not collide with `vnet` or stdlib. No ABI
change (Law 1: `libs/api`/`libs/types` untouched). Existing scripts unaffected.
No migration.

## Rollback Plan

Delete `bindings_vfs.rs`; revert the `mod` line + `vfs` registration block in
main.rs; remove the boot.rs test. No persisted state or protocol change — clean
revert. F.1 (script loading) is independent and stays functional.

## Security Considerations

- VFS service enforces `/data/`/`/tmp/` authorization server-side — Lua scripts
  cannot write/read outside sanctioned paths.
- Path capped at 253, content chunk capped at 480 — fixed/bounded buffers, no overflow.
- All `unsafe` FFI blocks carry `// SAFETY:` notes (Law 4); cell uses `unsafe` only
  for the Lua C ABI, never raw hardware.

## Next Steps

Phase F complete after this. Follow-up candidates (out of scope, YAGNI): `vfs.list`
(directory enumeration), `vfs.exists`, lifting the 480-byte cap once the VFS recv
buffer size is confirmed.
