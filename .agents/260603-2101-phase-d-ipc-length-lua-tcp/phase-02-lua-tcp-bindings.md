# Phase 02 — Lua TCP Bindings (`vnet.*`)

## Context Links
- Reference IPC pattern: `cells/apps/net-tools/src/bin/nc.rs` (client mode `:76`–`:152`)
- Lua FFI: `cells/runtimes/lua/src/ffi.rs`
- Lua binding pattern: `cells/runtimes/lua/src/bindings_io.rs` (`#[no_mangle] pub unsafe extern "C" fn ViCell_*`)
- Lua entry: `cells/runtimes/lua/src/main.rs:25` (`luaL_openlibs`)
- Test template: `tests/integration/tests/boot.rs::network_curl_http_get` (`:270`)
- Depends on: Phase 01 (SEND correctness)

## Overview
- **Priority:** P1
- **Status:** pending
- **Description:** Expose a `vnet` global table in Lua with `connect`, `send`, `recv`, `close`. Each function talks to the net service (endpoint 6) over IPC using the exact wire format `nc.rs` uses. Pure Rust — no new C glue, no `build.rs` change.

## Lua API Surface
```
vnet.connect(ip_str, port_int)        → cap_id_int  |  nil, errmsg
vnet.send(cap_id_int, data_str)       → bytes_written_int
vnet.recv(cap_id_int [, buf_len_int]) → data_str  |  nil
vnet.close(cap_id_int)                → nil
```

## Key Insights
- Lua cell has `extern crate alloc` (8 MB static heap via `__wrap__sbrk`) → `alloc::vec!` is available.
- Module-level `unsafe` is allowed (cell is NOT `#![forbid(unsafe_code)]`). All Lua C functions are `unsafe extern "C" fn f(L: *mut LuaState) -> c_int`.
- `lua_pushcfunction` is a macro in lua.h (not a real symbol). Bind `lua_pushcclosure(L, f, 0)` directly instead — the macro is just `lua_pushcclosure(L,f,0)`.
- `sys_recv`/`sys_try_recv` return **sender_id, not byte count** — same constraint as the net cell. The Lua bindings must size replies by the buffer they pass, and (for `recv`) trim trailing zeros, exactly as `nc.rs` does (`nc.rs:142` uses `position(|&b| b == 0)`).
- `lua_to_str` pattern for reading a Lua string arg is in `bindings_io.rs:30`. `vnet.send` data may be long (HTTP request) so use `lua_tolstring` directly to get a length + pointer rather than a fixed stack buffer where possible — but a bounded copy buffer is simpler and KISS for Phase D. Cap at a reasonable MAX (e.g. 512) matching the net cell's receive buffer.

## IPC Wire Format (mirror nc.rs exactly)
Opcodes: `SOCKET_TCP=0x10`, `CONNECT=0x12`, `SEND=0x13`, `RECV=0x14`, `CLOSE=0x15`. Endpoint `NET_ENDPOINT=6`.
- All messages: `[opcode:1][cap:8 LE][payload:*]`
- CONNECT payload: `[addr:4][port:2 LE]`, reply `[0x00]`=ok / `[0x01]`=err
- SEND payload: raw bytes, reply `[n:4 LE]` bytes written
- RECV payload: `[buf_len:4 LE]`, reply = raw bytes (0-length = no data yet)
- SOCKET_TCP reply: `[cap:8 LE]` (0 = error)
- CLOSE reply: `[0x01]` one byte

**Important send-buffer cap:** Phase 01 fixed the net cell to scan for true length. Lua `vnet.send` must size its IPC buffer to exactly `9 + data.len()` and send `&msg[..9 + data.len()]` (like `nc.rs:119`), never a padded fixed array — otherwise the data is fine but you waste stack.

## Architecture / Data Flow
```
Lua: vnet.connect("10.0.2.2", 8080)
  └ vnet_connect(L):
       parse ip_str → [u8;4]; port → u16
       SOCKET_TCP → recv cap (sys_recv)
       CONNECT[cap][addr][port] → recv ack
       ack==0x00 → push cap as integer (return 1)
       else      → push nil, errstr   (return 2)

Lua: vnet.send(cap, "GET / HTTP/1.0\r\n\r\n")
  └ vnet_send(L):
       cap = tointeger(1); data = tolstring(2)
       retry loop (mirror nc.rs:111): send unsent suffix, accumulate n
       push total bytes written (return 1)

Lua: vnet.recv(cap, 256)
  └ vnet_recv(L):
       cap = tointeger(1); buf_len = tointeger(2) or 512
       poll loop (mirror nc.rs:137): RECV[cap][buf_len], recv reply
       on first non-empty → push trimmed string (return 1)
       timeout → push nil (return 1)

Lua: vnet.close(cap)
  └ vnet_close(L): CLOSE[cap], drain reply, push nothing (return 0)
```

## Related Code Files
**Create:**
- `cells/runtimes/lua/src/bindings_net.rs` — `vnet_connect`, `vnet_send`, `vnet_recv`, `vnet_close` + IPC helpers

**Modify:**
- `cells/runtimes/lua/src/ffi.rs` — add 4 missing FFI declarations
- `cells/runtimes/lua/src/main.rs` — `mod bindings_net;` + register `vnet` table after `luaL_openlibs`
- `tests/integration/tests/boot.rs` — new `lua_tcp_http_get` test

**No build.rs change** — pure Rust, no new C files.

## Implementation Steps

### Step 1 — Add missing FFI declarations to `ffi.rs`
Inside the existing `extern "C" { ... }` block (after `lua_touserdata`, before the closing brace at `:90`):

```rust
    // ── Function registration / tables ────────────────────────────────────────

    /// Push a C closure with `n` upvalues. `lua_pushcfunction(L,f)` in lua.h is
    /// the macro `lua_pushcclosure(L,f,0)`; we bind the real symbol and pass 0.
    pub fn lua_pushcclosure(
        L: *mut LuaState,
        f: unsafe extern "C" fn(*mut LuaState) -> c_int,
        n: c_int,
    );

    /// Pop a value from the stack and set it as global `name`.
    pub fn lua_setglobal(L: *mut LuaState, name: *const c_char);

    /// Create a new empty table and push it onto the stack.
    pub fn lua_createtable(L: *mut LuaState, narr: c_int, nrec: c_int);

    /// `t[k] = v`: t at stack `idx`, v at top (popped). `k` is a C string.
    pub fn lua_setfield(L: *mut LuaState, idx: c_int, k: *const c_char);
```
**Notes:**
- `lua_newtable(L)` is a macro = `lua_createtable(L,0,0)`. Bind `lua_createtable` (real symbol) and call it with `(L, 0, 0)`. Do NOT declare `lua_newtable` — it is not an exported symbol and the link will fail.
- `lua_tointegerx` and `lua_tolstring` already exist (`ffi.rs:87`, `:57`) — do not re-declare.

### Step 2 — Create `bindings_net.rs`
```rust
//! Rust-side TCP socket bindings exposed to Lua via C FFI (`vnet.*`).
// `L` is the universal Lua C API convention for `lua_State*`.
#![allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
//!
//! Mirrors the verified IPC wire format used by `nc.rs`: every message is
//! `[opcode:1][cap:8 LE][payload:*]` sent to the net service (endpoint 6).
//! Replies are read with `sys_recv`, which returns the SENDER id, not a byte
//! count — reply length is bounded by the buffer we pass.

extern crate alloc;

use core::ffi::{c_char, c_int};
use crate::ffi::LuaState;
use ostd::syscall::{sys_recv, sys_send, sys_yield, SyscallResult};

/// Net service cell task ID (init spawn order: vfs=3 … net=6).
const NET_ENDPOINT: usize = 6;

const SOCKET_TCP: u8 = 0x10;
const CONNECT:    u8 = 0x12;
const SEND_OP:    u8 = 0x13;
const RECV_OP:    u8 = 0x14;
const CLOSE_OP:   u8 = 0x15;

/// Upper bound for a single SEND payload copied off the Lua stack.
const MAX_SEND: usize = 512;
/// Upper bound for a RECV request (matches net cell's 4096 recv cap).
const MAX_RECV: usize = 4096;

/// Read the string arg at stack `idx` as a byte slice borrowed from Lua.
///
/// # Safety
/// `L` must be valid; the returned slice lives only while the value stays on
/// the Lua stack (caller must not pop before use).
unsafe fn lua_arg_bytes<'a>(L: *mut LuaState, idx: c_int) -> Option<&'a [u8]> {
    let mut len: usize = 0;
    // SAFETY: L valid; idx is a checked stack position.
    let ptr = unsafe { crate::ffi::lua_tolstring(L, idx, &mut len as *mut _) };
    if ptr.is_null() { return None; }
    // SAFETY: Lua guarantees `len` valid bytes at `ptr`.
    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

/// Parse "a.b.c.d" into 4 octets.
fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let s = core::str::from_utf8(s).ok()?;
    let mut it = s.splitn(5, '.');
    let mut out = [0u8; 4];
    for slot in out.iter_mut() {
        let part = it.next()?;
        let mut n: u16 = 0;
        if part.is_empty() { return None; }
        for ch in part.bytes() {
            if !(b'0'..=b'9').contains(&ch) { return None; }
            n = n * 10 + (ch - b'0') as u16;
            if n > 255 { return None; }
        }
        *slot = n as u8;
    }
    if it.next().is_some() { return None; }
    Some(out)
}

/// `vnet.connect(ip_str, port_int)` → cap_id | nil, errmsg
#[no_mangle]
pub unsafe extern "C" fn vnet_connect(L: *mut LuaState) -> c_int {
    // SAFETY: L valid; arg 1 is the ip string, arg 2 the port integer.
    let ip = match unsafe { lua_arg_bytes(L, 1) }.and_then(parse_ipv4) {
        Some(a) => a,
        None => {
            unsafe { crate::ffi::lua_pushnil(L) };
            unsafe { crate::ffi::lua_pushstring(L, c"invalid ip".as_ptr()) };
            return 2;
        }
    };
    let port = unsafe { crate::ffi::lua_tointegerx(L, 2, core::ptr::null_mut()) } as u16;

    // SOCKET_TCP → cap
    let socket_msg = [SOCKET_TCP, 0, 0, 0, 0, 0, 0, 0, 0];
    sys_send(NET_ENDPOINT, &socket_msg);
    let mut cap_reply = [0u8; 8];
    let cap = match sys_recv(0, &mut cap_reply) {
        SyscallResult::Ok(_) => u64::from_le_bytes(cap_reply),
        _ => 0,
    };
    if cap == 0 {
        unsafe { crate::ffi::lua_pushnil(L) };
        unsafe { crate::ffi::lua_pushstring(L, c"socket failed".as_ptr()) };
        return 2;
    }

    // CONNECT [0x12][cap:8][addr:4][port:2]
    let mut conn = [0u8; 15];
    conn[0] = CONNECT;
    conn[1..9].copy_from_slice(&cap.to_le_bytes());
    conn[9..13].copy_from_slice(&ip);
    conn[13..15].copy_from_slice(&port.to_le_bytes());
    sys_send(NET_ENDPOINT, &conn);
    let mut ack = [0u8; 1];
    match sys_recv(0, &mut ack) {
        SyscallResult::Ok(_) if ack[0] == 0x00 => {
            // SAFETY: L valid; cap fits in i64.
            unsafe { crate::ffi::lua_pushinteger(L, cap as i64) };
            1
        }
        _ => {
            unsafe { crate::ffi::lua_pushnil(L) };
            unsafe { crate::ffi::lua_pushstring(L, c"connect failed".as_ptr()) };
            2
        }
    }
}

/// `vnet.send(cap_id, data_str)` → bytes_written
#[no_mangle]
pub unsafe extern "C" fn vnet_send(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    // SAFETY: L valid; arg 2 is the data string.
    let data = unsafe { lua_arg_bytes(L, 2) }.unwrap_or(&[]);
    let data = &data[..data.len().min(MAX_SEND)];

    // Retry until all bytes buffered (mirror nc.rs:111). Each retry forwards
    // only the unsent suffix so a partial write never duplicates a prefix.
    let mut sent = 0usize;
    for _ in 0..500 {
        if sent >= data.len() { break; }
        let rem = &data[sent..];
        let mut msg = alloc::vec![0u8; 9 + rem.len()];
        msg[0] = SEND_OP;
        msg[1..9].copy_from_slice(&cap.to_le_bytes());
        msg[9..9 + rem.len()].copy_from_slice(rem);
        sys_send(NET_ENDPOINT, &msg);
        let mut cnt = [0u8; 4];
        match sys_recv(0, &mut cnt) {
            SyscallResult::Ok(_) => {
                let n = u32::from_le_bytes(cnt) as usize;
                sent += n;
                if n == 0 { sys_yield(); }
            }
            _ => break,
        }
    }
    // SAFETY: L valid.
    unsafe { crate::ffi::lua_pushinteger(L, sent as i64) };
    1
}

/// `vnet.recv(cap_id [, buf_len])` → data_str | nil
#[no_mangle]
pub unsafe extern "C" fn vnet_recv(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    let mut isnum: c_int = 0;
    let req = unsafe { crate::ffi::lua_tointegerx(L, 2, &mut isnum as *mut _) };
    let buf_len = if isnum != 0 { (req as usize).min(MAX_RECV) } else { 512 };

    let mut recv_msg = [0u8; 13];
    recv_msg[0] = RECV_OP;
    recv_msg[1..9].copy_from_slice(&cap.to_le_bytes());
    recv_msg[9..13].copy_from_slice(&(buf_len as u32).to_le_bytes());

    let mut data = alloc::vec![0u8; buf_len];
    // Poll until data arrives (mirror nc.rs:137).
    for _ in 0..500 {
        // Zero before each receive so a short reply leaves no stale tail.
        for b in data.iter_mut() { *b = 0; }
        sys_send(NET_ENDPOINT, &recv_msg);
        match sys_recv(0, &mut data) {
            SyscallResult::Ok(_) if data[0] != 0 => {
                // Reply length unknown (sys_recv returns sender, not count) —
                // trim at the first NUL, matching nc.rs:142. ASCII-only payload.
                let end = data.iter().position(|&b| b == 0).unwrap_or(buf_len);
                // SAFETY: L valid; data[..end] is initialised.
                unsafe {
                    crate::ffi::lua_pushlstring(L, data.as_ptr() as *const c_char, end);
                }
                return 1;
            }
            _ => sys_yield(),
        }
    }
    // SAFETY: L valid.
    unsafe { crate::ffi::lua_pushnil(L) };
    1
}

/// `vnet.close(cap_id)` → nil (no return value)
#[no_mangle]
pub unsafe extern "C" fn vnet_close(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    let mut msg = [0u8; 9];
    msg[0] = CLOSE_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    sys_send(NET_ENDPOINT, &msg);
    let mut r = [0u8; 1];
    let _ = sys_recv(0, &mut r);
    let _ = L; // no values pushed
    0
}
```

**RECV trim caveat:** like the net cell, this trims at the first NUL byte — a binary payload
containing an interior `0x00` would be truncated. Acceptable for the ASCII HTTP use case. Documented inline.

### Step 3 — Register the `vnet` table in `main.rs`
Add `mod bindings_net;` to the module list (after `mod bindings_io;` at `:7`). Then, immediately after `luaL_openlibs(L)` at `:25`, insert:

```rust
    // Register the `vnet` TCP table. lua_newtable(L) is the macro
    // lua_createtable(L,0,0); we call the real symbol. Each field is a C
    // closure with 0 upvalues (lua_pushcfunction == lua_pushcclosure(L,f,0)).
    // SAFETY: L is non-null; the binding fns uphold the lua_CFunction contract.
    unsafe {
        ffi::lua_createtable(L, 0, 4);
        ffi::lua_pushcclosure(L, bindings_net::vnet_connect, 0);
        ffi::lua_setfield(L, -2, c"connect".as_ptr());
        ffi::lua_pushcclosure(L, bindings_net::vnet_send, 0);
        ffi::lua_setfield(L, -2, c"send".as_ptr());
        ffi::lua_pushcclosure(L, bindings_net::vnet_recv, 0);
        ffi::lua_setfield(L, -2, c"recv".as_ptr());
        ffi::lua_pushcclosure(L, bindings_net::vnet_close, 0);
        ffi::lua_setfield(L, -2, c"close".as_ptr());
        ffi::lua_setglobal(L, c"vnet".as_ptr());
    }
```
**Stack discipline:** `lua_createtable` pushes the table (top = -1). Each
`pushcclosure` pushes the fn (top = -1), so the table is now at -2 — that's why
`lua_setfield(L, -2, ...)` is correct; `setfield` pops the value, leaving the table
back at -1. Finally `lua_setglobal` pops the table. Net stack delta = 0. Confirm with
`lua_gettop` during bring-up if unsure.

### Step 4 — Build & lint
```bash
cargo build --release -p lua    # or the lua cell package name
cargo clippy -p lua -- -D warnings
```

### Step 5 — Add integration test `lua_tcp_http_get`
In `tests/integration/tests/boot.rs`, after `network_curl_http_get` (`:298`), add — mirroring its structure:

```rust
/// Phase D.2: HTTP/1.0 GET from Lua via the `vnet.*` TCP bindings.
///
/// A host HTTP server is started before QEMU boots. SLIRP routes guest
/// `10.0.2.2:<port>` → host `127.0.0.1:<port>`. Lua connects out, sends a
/// minimal GET, and prints the response body — proving the vnet bindings and
/// the Phase D.1 SEND-length fix work end-to-end. No hostfwd: Lua dials OUT.
#[test]
fn lua_tcp_http_get() {
    if !prerequisites_ok() {
        return;
    }

    // Keep `_server` alive — dropping early can race the accept() thread.
    let (port, _server) = spawn_http_server();

    let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());

    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.wait_for("DHCP acquired", 40)
        .unwrap_or_else(|e| panic!("DHCP failed: {e}\n--- output ---\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));

    // The Lua cell whitespace-joins argv, so the -e expression must be space-free
    // OR the shell must preserve it. nc/curl tests use a single token; here the
    // chunk contains spaces inside string literals. Send it as one line; the
    // shell forwards the full tail after `lua -e`.
    qemu.send_line(&format!(
        "lua -e local c=vnet.connect('10.0.2.2',{port});vnet.send(c,'GET / HTTP/1.0\\r\\n\\r\\n');print(vnet.recv(c,256));vnet.close(c)"
    ));

    // The server replies "HTTP/1.0 200 OK\r\n...\r\n\r\nHELLO".
    qemu.wait_for("200", 20)
        .unwrap_or_else(|e| panic!("no HTTP 200 status: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.wait_for("HELLO", 10)
        .unwrap_or_else(|e| panic!("no response body: {e}\n--- output ---\n{}", qemu.dump()));
}
```

**Park behavior:** after `-e` evaluation the Lua cell parks in `loop { yield_now() }`
(`lua/src/main.rs:43`). `print()` output flushes to serial *before* the park, so
`wait_for("HELLO", ...)` still matches.

## Todo List
- [ ] Step 1: add `lua_pushcclosure`, `lua_setglobal`, `lua_createtable`, `lua_setfield` to `ffi.rs`
- [ ] Step 2: create `bindings_net.rs` with `vnet_connect/send/recv/close` + helpers
- [ ] Step 3: `mod bindings_net;` + register `vnet` table after `luaL_openlibs` in `main.rs`
- [ ] Step 4: `cargo build --release` + `cargo clippy -- -D warnings` clean
- [ ] Step 5: add `lua_tcp_http_get` integration test
- [ ] Step 6: run test — expect "200" then "HELLO" in serial

## Success Criteria
- Lua cell builds clean, clippy clean.
- `lua_tcp_http_get` passes: serial shows "200" and "HELLO".
- `network_curl_http_get` and `network_tcp_send_recv` still pass (no regression in the shared net cell).

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `lua_newtable`/`lua_pushcfunction` declared as symbols → link error | Med | High | Bind real symbols `lua_createtable` / `lua_pushcclosure` instead (documented) |
| Stack imbalance in registration corrupts Lua state | Med | High | Step 3 documents exact stack deltas; verify with `lua_gettop` during bring-up |
| Shell mangles the `-e` chunk (spaces inside string literals) | Med | Med | Single `send_line`; if shell splits on space, fall back to a NUL/`%20`-free expression or a script file. Test will surface this immediately |
| RECV interior-NUL truncation | Low | Low | ASCII HTTP only; documented inline (same limitation as net cell) |
| SEND corruption if Phase 01 not landed | Med | High | Land Phase 01 first (stated dependency) |
| Lua cell heap exhaustion from `alloc::vec!` in send/recv | Low | Med | 8 MB static heap; buffers ≤4 KB. Negligible |

## Rollback Plan
- `bindings_net.rs` is new — delete it.
- Revert the `mod bindings_net;` line and the registration block in `main.rs`.
- Revert the 4 FFI additions in `ffi.rs` (additive only — safe to remove).
- Remove `lua_tcp_http_get` from `boot.rs`.
No ABI change, no shared-state change. Net cell is untouched by this phase. Reverting cannot cascade.

## Backwards Compatibility
- Purely additive: a new `vnet` global. Existing Lua scripts and `io.*`/`os.*` bindings unaffected.
- No change to the net service wire protocol — `vnet` is a third consumer alongside `nc` and `curl`.

## Security Considerations
- `vnet` gives Lua scripts raw outbound TCP. No new privilege escalation: the net cell already mediates all socket access via cap IDs, and Lua can only act on caps the net cell minted for it.
- Bounded buffers (`MAX_SEND=512`, `MAX_RECV=4096`) prevent unbounded allocation from a malicious script.

## Next Steps / Unresolved Questions
- **Q1:** Does the ViCell shell preserve spaces in the `lua -e` tail, or whitespace-join argv such that `vnet.connect('10.0.2.2', 8080)` (with a space after the comma) breaks? The test expression is written space-free inside the call to be safe — confirm during Step 6. If the shell splits, consider a heredoc/script-file path.
- **Q2:** Confirm the exact cargo package name for the lua cell (`-p lua` vs `-p lua-runtime`) before running build commands.
