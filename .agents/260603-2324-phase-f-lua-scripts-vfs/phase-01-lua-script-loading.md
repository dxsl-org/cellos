# Phase F.1 — Lua Script File Loading

## Context Links

- Working IPC pattern: `cells/apps/shell/src/cmd_fs.rs` (`read_file_vfs` :351, OP_READ :280)
- Lua entry: `cells/runtimes/lua/src/main.rs` (`-e` branch :59-70, park loop :67-69)
- FFI block: `cells/runtimes/lua/src/ffi.rs` (`luaL_loadstring` :35)
- Test harness: `tests/integration/tests/boot.rs`

## Overview

- **Priority:** P2
- **Status:** pending
- **Description:** `lua /data/script.lua` reads the file from the VFS cell over
  OP_READ IPC and executes it with `luaL_loadbuffer` + `lua_pcallk`.

## Key Insights

- `luaL_loadbuffer` is preferred over `luaL_loadstring` for file content: it takes
  an explicit byte length and does NOT require NUL-termination, so it is correct
  for raw VFS bytes.
- `sys_recv` returns the SENDER id, not a byte count. Length is recovered by
  zero-scan (`rposition(|&b| b != 0)`), exactly like `read_file_vfs` cmd_fs.rs:361.
- OP_READ reply is raw file bytes — trailing NUL bytes are truncated by zero-scan.
  Lua source is ASCII, so this is acceptable (documented limitation).
- The chunk name passed to `luaL_loadbuffer` must be NUL-terminated; the source
  buffer must not.
- main.rs has NO `extern crate alloc` — must be added for `alloc::vec!`.

## Requirements

**Functional**
- `lua <path>` where `<path>` does not start with `-e` reads `<path>` from VFS and runs it.
- Missing file prints `lua: cannot open '<path>'`.
- Compile errors are printed (from the Lua error string) and popped off the stack.

**Non-functional**
- `cargo check -p lua` → 0 warnings.
- Script path must park after execution (same as `-e`), never return — kernel
  cell-exit does not yet unmap a returning cell's address space (main.rs:63-66).

## Architecture

### Data Flow

```
shell: `lua /data/hello.lua`
  → spawner publishes argv → sys_spawn_args(argbuf) → args = "/data/hello.lua"
  → args does NOT start with "-e" and is non-empty → script branch
  → vfs_read_to_buf(path, buf):  [OP_READ=8][path_len][path] → sys_send(3)
                                  sys_recv(0, buf) → zero-scan length n
  → luaL_loadbuffer(L, buf, n, "@<path>\0")  → LUA_OK ?
       yes → lua_pcallk(L, 0, LUA_MULTRET, 0, 0, null)   (runs the chunk)
       no  → lua_tolstring(L,-1) print error → lua_settop(L,-2) pop
  → park: loop { yield_now() }
```

### Control-Flow Placement (verified)

main.rs current order: `-e` branch (:59-70, ends in park loop) → REPL fallback
(:72-79). Insert the script branch **between** them, after line 70. Because the
`-e` branch parks (never falls through), the new branch is only reached when args
is non-empty and not `-e`. Empty args → falls through to REPL. This preserves REPL
behavior exactly.

## Related Code Files

**Modify**
- `cells/runtimes/lua/src/ffi.rs` — add `luaL_loadbuffer` to the `extern "C"` block.
- `cells/runtimes/lua/src/main.rs` — add `extern crate alloc;`, `vfs_read_to_buf`
  helper, script-file branch.

**Create** — none.
**Delete** — none.

## Implementation Steps

1. **ffi.rs** — after `luaL_loadstring` (:35), inside the same `extern "C"` block, add:
   ```rust
   /// Compile `sz` bytes at `buff` as a Lua chunk named `name`.
   /// Unlike `luaL_loadstring`, the buffer need not be NUL-terminated,
   /// making it correct for binary-file content from VFS.
   /// Returns `LUA_OK` on success; otherwise pushes an error string.
   pub fn luaL_loadbuffer(
       L: *mut LuaState,
       buff: *const c_char,
       sz: usize,
       name: *const c_char,
   ) -> c_int;
   ```

2. **main.rs** — add `extern crate alloc;` after line 5 (`extern crate api;`), so it
   sits alongside the existing `extern crate ostd; extern crate api;`.

3. **main.rs** — add the VFS read helper as a private fn before `main` (mirrors
   `read_file_vfs` cmd_fs.rs:351):
   ```rust
   /// Read up to 4096 bytes from a VFS path via OP_READ IPC.
   /// Returns byte count (zero-scan from reply; sys_recv returns sender_id not length).
   fn vfs_read_to_buf(path: &str, buf: &mut [u8]) -> usize {
       const VFS_ENDPOINT: usize = 3;
       const OP_READ: u8 = 8;
       let pb = path.as_bytes();
       let pl = pb.len().min(253) as u8;
       let mut req = [0u8; 256];
       req[0] = OP_READ;
       req[1] = pl;
       req[2..2 + pl as usize].copy_from_slice(&pb[..pl as usize]);
       ostd::syscall::sys_send(VFS_ENDPOINT, &req[..2 + pl as usize]);
       match ostd::syscall::sys_recv(0, buf) {
           ostd::syscall::SyscallResult::Ok(_) => {
               buf.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0)
           }
           _ => 0,
       }
   }
   ```

4. **main.rs** — insert the script-file branch between the `-e` branch (ends :70)
   and the REPL fallback (:72):
   ```rust
   // `lua /path/to/script.lua` — read file from VFS and execute.
   // Triggered when args is non-empty and does not start with `-e`.
   if !args.is_empty() {
       let path = args.trim();
       let mut file_buf = alloc::vec![0u8; 4096];
       let n = vfs_read_to_buf(path, &mut file_buf);
       if n == 0 {
           ostd::io::print("lua: cannot open '");
           ostd::io::print(path);
           ostd::io::println("'");
       } else {
           // Derive a NUL-terminated chunk name like "@script.lua".
           let mut chunk_name = alloc::vec![b'@'; 1 + path.len() + 1];
           chunk_name[1..1 + path.len()].copy_from_slice(path.as_bytes());
           *chunk_name.last_mut().unwrap() = 0; // NUL terminator
           // SAFETY: L is valid; file_buf[..n] is valid Lua source bytes;
           // chunk_name is NUL-terminated and outlives the pcall.
           let rc = unsafe {
               ffi::luaL_loadbuffer(
                   L,
                   file_buf.as_ptr() as *const core::ffi::c_char,
                   n,
                   chunk_name.as_ptr() as *const core::ffi::c_char,
               )
           };
           if rc == ffi::LUA_OK {
               let _ = unsafe {
                   ffi::lua_pcallk(L, 0, ffi::LUA_MULTRET, 0, 0, core::ptr::null_mut())
               };
           } else {
               // Print compile error and pop it.
               let mut len = 0usize;
               let ptr = unsafe { ffi::lua_tolstring(L, -1, &mut len as *mut _) };
               if !ptr.is_null() {
                   let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
                   if let Ok(s) = core::str::from_utf8(bytes) {
                       ostd::io::println(s);
                   }
               }
               unsafe { ffi::lua_settop(L, -2) }; // pop error
           }
       }
       loop { ostd::task::yield_now(); }
   }
   ```

5. Run `cargo check -p lua`; resolve any unused-import / warning to reach 0 warnings.

6. **boot.rs** — add the integration test after `lua_vnet_resolve_dns` (:339):
   ```rust
   /// Phase F.1: `lua /data/script.lua` — reads and executes a Lua script from VFS.
   #[test]
   fn lua_script_file() {
       if !prerequisites_ok() { return; }
       let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
       qemu.wait_for("ViCell >", BOOT_TIMEOUT)
           .unwrap_or_else(|e| panic!("prompt: {e}\n{}", qemu.dump()));
       assert!(qemu.output_contains("FAT16 /data volume mounted"), "FAT16 not mounted\n{}", qemu.dump());
       std::thread::sleep(std::time::Duration::from_millis(500));
       // Write the script to /data/ via the existing vwrite built-in.
       qemu.send_line("vwrite /data/hello.lua print('SCRIPT_OK')");
       qemu.wait_for("ViCell >", CMD_TIMEOUT)
           .unwrap_or_else(|e| panic!("vwrite: {e}\n{}", qemu.dump()));
       // Run the script.
       qemu.send_line("lua /data/hello.lua");
       qemu.wait_for("SCRIPT_OK", 15)
           .unwrap_or_else(|e| panic!("script did not run: {e}\n{}", qemu.dump()));
   }
   ```

## Todo List

- [ ] ffi.rs: add `luaL_loadbuffer` to `extern "C"` block
- [ ] main.rs: add `extern crate alloc;`
- [ ] main.rs: add `vfs_read_to_buf` helper
- [ ] main.rs: insert script-file branch (with park loop)
- [ ] `cargo check -p lua` → 0 warnings
- [ ] boot.rs: add `lua_script_file` test
- [ ] Run `lua_script_file` → `SCRIPT_OK`

## Success Criteria

- `cargo check -p lua` → 0 warnings.
- `lua_script_file` passes (prints `SCRIPT_OK`).
- Missing file path prints `lua: cannot open '<path>'`.
- REPL (no args) and `-e` (with args) paths unchanged — regression-free.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Forgetting `extern crate alloc` → `alloc::vec!` fails to compile | Med | High | Step 2 explicit; `cargo check` catches before test |
| Script branch reached on empty args, breaking REPL | Low | High | `if !args.is_empty()` guard; `-e` branch parks before it; REPL only on empty args |
| Returning instead of parking corrupts later spawns | Med | High | Branch ends in `loop { yield_now() }` (same as `-e` main.rs:67-69) |
| 4096-byte cap truncates large scripts | Low | Med | Matches net RECV cap; documented; IoT scripts are small |
| Non-ASCII / NUL bytes in source lost to zero-scan | Low | Low | Documented limitation; Lua source is ASCII |

## Backwards Compatibility

No ABI change (Law 1 untouched — `libs/api`/`libs/types` not modified).
`luaL_loadbuffer` is additive to the FFI block. REPL and `-e` behavior unchanged.
No migration needed.

## Rollback Plan

Revert the three edits (ffi.rs, main.rs, boot.rs). No persisted state, no schema,
no protocol change — clean revert with zero cascade. `vwrite`/VFS unaffected.

## Security Considerations

- VFS service enforces `/data/`/`/tmp/` authorization server-side (cmd_fs.rs:282
  note) — the Lua cell cannot read outside sanctioned paths.
- Path length capped at 253 (`min(253)`), request buffer fixed 256 — no overflow.
- Cell remains `#![forbid(unsafe_code)]`? No — lua cell uses `unsafe` FFI; each new
  `unsafe` block carries a `// SAFETY:` note (Law 4).

## Next Steps

Blocks **Phase F.2** — shares `main.rs` (adds `extern crate alloc;` that F.2 reuses).
Must complete before F.2 starts (file ownership: sequential).
