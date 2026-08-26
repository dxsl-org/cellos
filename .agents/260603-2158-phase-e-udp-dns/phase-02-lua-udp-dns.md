# Phase 02 — Lua UDP Bindings + DNS Resolver

## Context Links

- Plan: [plan.md](plan.md)
- Depends on: [phase-01-udp-net-cell.md](phase-01-udp-net-cell.md)
- Memory: sys_recv returns sender_id (not byte count) → zero-scan; net cell IPC patterns.

## Overview

- **Priority:** P2
- **Status:** pending
- **Blockers:** Phase 01 (SENDTO/RECVFROM opcodes must exist).
- Add `vnet.udp_send`, `vnet.udp_recv`, `vnet.resolve` to the Lua `vnet` table. The
  resolver does the full DNS A-record dance (or short-circuits via a static table / IPv4
  literal) and returns a dotted-decimal string.

## Key Insights (verified against codebase)

- `vnet` table built at lua/main.rs:36 with `lua_createtable(L, 0, 4)` then 4 setfields
  (connect/send/recv/close) at lines 37-44, `setglobal` at 45. Adding 3 fns → bump hint
  to `lua_createtable(L, 0, 7)` and add 3 `pushcclosure`+`setfield` pairs.
- All needed FFI already exported (lua/ffi.rs): `lua_pushlstring` (:80),
  `lua_pushinteger` (:76), `lua_pushnil` (:72), `lua_pushstring` (:78),
  `lua_tointegerx` (:87), `lua_tolstring` (:57). **No ffi.rs change.**
- `parse_ipv4` (bindings_net.rs:45) reusable for the IPv4-literal fast-path.
- `vnet_recv` (bindings_net.rs:153) is the template for the retry+zero-scan recv loop.
- `NET_ENDPOINT = 6` (bindings_net.rs:17) already defined.
- Existing opcode consts (bindings_net.rs:19-23): `SOCKET_TCP/CONNECT/SEND_OP/RECV_OP/CLOSE_OP`.

## sys_recv semantics (critical to the design)

`sys_recv` returns the **sender id**, not a byte count. The Lua cell therefore cannot
know the exact reply length and uses zero-scan, *but only on the variable-length DATA
tail*. The RECVFROM reply has a fixed 6-byte header read by absolute offset:

```
reply layout:  [src_addr:4][src_port:2 LE][data:0..n]
               read by offset (NOT zero-scanned) ──┘ zero-scanned ┘
```

- **Presence/empty detection:** pre-zero the buffer; after recv, `buffer[0] != 0` means a
  reply arrived. A real DNS server (10.0.2.3 → first byte `0x0A`) always has a non-zero
  first octet; an empty net-cell reply leaves the pre-zeroed buffer all-zero. So
  `buffer[0..6] == [0;6]` (equivalently `buffer[0]==0`) ⇒ no datagram yet.
- **Header parse:** `src_addr = buffer[0..4]`, `src_port = u16::from_le(buffer[4..6])` —
  by fixed offset, immune to internal zero bytes (e.g. 10.0.2.3 has `buffer[1]==0`).
- **Data parse:** scan `buffer[6..]` for first NUL to find the end (same known
  ASCII/binary limitation as `vnet_recv`). For DNS the binary response rarely ends in a
  trailing 0x00; truncation risk is acceptable and pre-existing.

## Requirements

### Functional
- `vnet.udp_send(cap, ip_str, port, data_str)` → integer bytes sent.
- `vnet.udp_recv(cap [, buf_len])` → `(src_ip_str, src_port_int, data_str)` or `nil`.
- `vnet.resolve(hostname_str)` → `ip_str` or `nil`.

### Non-functional
- All FFI calls inside `// SAFETY:`-documented `unsafe` blocks (matches existing style).
- `cargo check -p lua` → 0 warnings.
- No alloc in `format_ip` / DNS builders where avoidable (use fixed stack buffers).

## Architecture — Lua API contract

| Lua call | IPC ops issued | Returns |
|----------|----------------|---------|
| `vnet.udp_send(cap,ip,port,data)` | SENDTO (cap pre-bound by caller) | bytes:int |
| `vnet.udp_recv(cap[,len])` | RECVFROM (poll/retry) | ip,port,data \| nil |
| `vnet.resolve(host)` | SOCKET_UDP→BIND→SENDTO→RECVFROM→CLOSE | ip:str \| nil |

`resolve` owns the full socket lifecycle (mint → use → close) so its UDP cap never
escapes to TCP code paths (Phase 01 type-safety contract).

### resolve() flow
1. Static table: `gateway`→`10.0.2.2`, `dns`→`10.0.2.3`, `localhost`→`127.0.0.1`. Return immediately.
2. `parse_ipv4(host)` succeeds → return host unchanged (already an IP literal).
3. SOCKET_UDP → cap (abort → nil if cap==0).
4. BIND(cap, 0) → ephemeral port (abort → nil if `[0xFF,0xFF]`).
5. `build_dns_query(host, &mut q)` → len.
6. SENDTO(cap, 10.0.2.3, 53, &q[..len]).
7. RECVFROM poll loop (≤500 retries, `sys_yield` between) until `buffer[0]!=0`.
8. `parse_dns_a(&buffer[6..])` → first A-record `[u8;4]` or nil.
9. CLOSE(cap) (always, even on parse failure — RAII discipline).
10. `format_ip(ip)` → push dotted-decimal string.

## Related Code Files

**Modify:**
- `cells/runtimes/lua/src/bindings_net.rs` — opcode consts; `vnet_udp_send`, `vnet_udp_recv`, `vnet_resolve`; helpers `build_dns_query`, `parse_dns_a`, `skip_dns_name`, `format_ip`, static-table lookup.
- `cells/runtimes/lua/src/main.rs` — bump `lua_createtable` count to 7; register 3 new fns.
- `tests/integration/tests/boot.rs` — `lua_vnet_resolve`, `lua_vnet_resolve_dns`.

**Not modified:** `cells/runtimes/lua/src/ffi.rs` (all FFI present).

## Implementation Steps

1. **bindings_net.rs opcode consts** — after line 23 add:
   ```rust
   const SOCKET_UDP:  u8 = 0x11;
   const BIND_OP:     u8 = 0x16;
   const SENDTO_OP:   u8 = 0x21;
   const RECVFROM_OP: u8 = 0x22;
   const DNS_SERVER: [u8; 4] = [10, 0, 2, 3]; // QEMU SLIRP DNS
   ```

2. **`vnet_udp_send`** — parse `(cap:int, ip:str, port:int, data:str)`; build
   `[SENDTO_OP][cap:8][addr:4][port:2 LE][data:*]`. UDP is atomic per datagram — no
   stream offset tracking. Retry only on tx-buffer-full (n==0):
   ```rust
   let mut sent = 0usize;
   for _ in 0..500 {
       sys_send(NET_ENDPOINT, &sendto_msg);
       let mut cnt = [0u8; 4];
       match sys_recv(0, &mut cnt) {
           SyscallResult::Ok(_) => {
               let n = u32::from_le_bytes(cnt) as usize;
               if n > 0 { sent = n; break; }
               sys_yield(); // tx full — retry
           }
           _ => break,
       }
   }
   // push sent as integer
   ```

3. **`vnet_udp_recv`** — parse `(cap:int, buf_len?:int)`; build `[RECVFROM_OP][cap:8][buf_len:4]`.
   Reply buffer = 6 + 512. Pre-zero, recv, poll up to 500 with `sys_yield`:
   ```rust
   // on Ok && buffer[0] != 0:
   let src_ip = [buf[0],buf[1],buf[2],buf[3]];
   let src_port = u16::from_le_bytes([buf[4], buf[5]]);
   let data_end = 6 + buf[6..].iter().position(|&b| b==0).unwrap_or(buf.len()-6);
   // format_ip(src_ip) -> push str; push integer src_port; push lstring data
   // return 3
   // else (timeout): push nil; return 1
   ```

4. **`build_dns_query(hostname, buf) -> usize`** (free fn):
   ```rust
   fn build_dns_query(hostname: &str, buf: &mut [u8]) -> usize {
       buf[0..12].copy_from_slice(&[0x12,0x34,0x01,0x00,0x00,0x01,0,0,0,0,0,0]);
       let mut pos = 12;
       for label in hostname.split('.') {
           buf[pos] = label.len() as u8; pos += 1;
           buf[pos..pos+label.len()].copy_from_slice(label.as_bytes());
           pos += label.len();
       }
       buf[pos] = 0; pos += 1;
       buf[pos..pos+4].copy_from_slice(&[0x00,0x01,0x00,0x01]); // QTYPE=A, QCLASS=IN
       pos + 4
   }
   ```
   Guard: caller bounds hostname ≤ 253 chars and buf ≥ 12+host+1+4. Skip empty labels.

5. **`skip_dns_name` + `parse_dns_a`** (free fns) — extract first A record. Handle the
   0xC0 compression pointer in answer names. Return `Option<[u8;4]>`:
   ```rust
   fn skip_dns_name(buf: &[u8], mut pos: usize) -> Option<usize> {
       loop {
           if pos >= buf.len() { return None; }
           let len = buf[pos];
           if len == 0 { return Some(pos + 1); }
           if len & 0xC0 == 0xC0 { return Some(pos + 2); }
           pos += 1 + len as usize;
       }
   }
   fn parse_dns_a(buf: &[u8]) -> Option<[u8; 4]> {
       if buf.len() < 12 || buf[2] & 0x80 == 0 { return None; } // need QR=1
       let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;
       if ancount == 0 { return None; }
       let mut pos = skip_dns_name(buf, 12)?; pos += 4; // skip question QTYPE+QCLASS
       for _ in 0..ancount {
           pos = skip_dns_name(buf, pos)?;
           if pos + 10 > buf.len() { return None; }
           let rtype = u16::from_be_bytes([buf[pos], buf[pos+1]]);
           let rdlen = u16::from_be_bytes([buf[pos+8], buf[pos+9]]) as usize;
           pos += 10;
           if rtype == 1 && rdlen == 4 && pos + 4 <= buf.len() {
               return Some([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]);
           }
           pos += rdlen;
       }
       None
   }
   ```
   NOTE: `parse_dns_a` receives the **full** DNS message (offset 0), not `buffer[6..]`.
   In `vnet_resolve`, the DNS payload starts at reply offset 6 → pass `&buffer[6..]`.
   The compression pointers inside are relative to the DNS message start (offset 6 of
   the reply = offset 0 of the slice), so slicing first is correct.

6. **`format_ip(ip, buf) -> usize`** (free fn, no alloc) — dotted-decimal itoa, max 15
   chars (per the brief's snippet). Returns byte length.

7. **`vnet_resolve`** — orchestrate per the resolve() flow above. Static table + IPv4
   literal short-circuit first (no IPC, makes `lua_vnet_resolve` deterministic). Always
   CLOSE the cap before returning. On any failure → push nil (return 1).

8. **lua/main.rs registration** — change line 36 to `lua_createtable(L, 0, 7);` and after
   the close binding (line 44) add:
   ```rust
   ffi::lua_pushcclosure(L, bindings_net::vnet_udp_send, 0);
   ffi::lua_setfield(L, -2, c"udp_send".as_ptr());
   ffi::lua_pushcclosure(L, bindings_net::vnet_udp_recv, 0);
   ffi::lua_setfield(L, -2, c"udp_recv".as_ptr());
   ffi::lua_pushcclosure(L, bindings_net::vnet_resolve, 0);
   ffi::lua_setfield(L, -2, c"resolve".as_ptr());
   ```
   Update the stack-discipline comment count (4 → 7).

9. **boot.rs tests** — append:
   ```rust
   /// Phase E: vnet.resolve() static-table fast-path (no DNS, fully deterministic).
   #[test]
   fn lua_vnet_resolve() {
       if !prerequisites_ok() { return; }
       let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
       qemu.wait_for("ViCell >", BOOT_TIMEOUT)
           .unwrap_or_else(|e| panic!("prompt not reached: {e}"));
       std::thread::sleep(std::time::Duration::from_millis(500));
       qemu.send_line("lua -e print(vnet.resolve('gateway'))");
       qemu.wait_for("10.0.2.2", 10).unwrap_or_else(|e| {
           panic!("static resolve failed: {e}\n--- output ---\n{}", qemu.dump())
       });
   }

   /// Phase E: vnet.resolve() real DNS query via QEMU SLIRP (10.0.2.3:53).
   #[test]
   fn lua_vnet_resolve_dns() {
       if !prerequisites_ok() { return; }
       let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
       qemu.wait_for("ViCell >", BOOT_TIMEOUT)
           .unwrap_or_else(|e| panic!("prompt not reached: {e}"));
       qemu.wait_for("DHCP acquired", 40).unwrap_or_else(|e| {
           panic!("DHCP did not complete: {e}\n--- output ---\n{}", qemu.dump())
       });
       std::thread::sleep(std::time::Duration::from_millis(500));
       qemu.send_line("lua -e print(vnet.resolve('google.com'))");
       qemu.wait_for(".", 20).unwrap_or_else(|e| {
           panic!("DNS resolve produced no dotted-decimal output: {e}\n--- output ---\n{}", qemu.dump())
       });
   }
   ```

10. `cargo check -p lua` → 0 warnings. Run full integration suite → 25 tests pass.

## Todo List

- [ ] bindings_net.rs: opcode consts + DNS_SERVER
- [ ] bindings_net.rs: vnet_udp_send
- [ ] bindings_net.rs: vnet_udp_recv
- [ ] bindings_net.rs: build_dns_query
- [ ] bindings_net.rs: skip_dns_name + parse_dns_a
- [ ] bindings_net.rs: format_ip
- [ ] bindings_net.rs: vnet_resolve (static table + IPv4 + DNS path)
- [ ] lua/main.rs: createtable 7 + register 3 fns
- [ ] boot.rs: lua_vnet_resolve
- [ ] boot.rs: lua_vnet_resolve_dns
- [ ] `cargo check -p lua` → 0 warnings
- [ ] full suite → 25 tests pass

## Success Criteria

- `lua_vnet_resolve` passes deterministically (`"gateway"` → `"10.0.2.2"`).
- `lua_vnet_resolve_dns` output contains a `.` (a dotted-decimal IP) when host DNS reachable.
- 23 prior tests unaffected; total green = 25.
- `vnet.resolve` always CLOSEs its cap (no socket-table leak across repeated calls,
  MAX_SOCKETS=18).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| DNS reply header byte ends in 0x00 truncated by data zero-scan | Low | Med | Header read by fixed offset (immune); only `data[6..]` zero-scanned — binary A-record rarely ends in NUL. |
| `lua_vnet_resolve_dns` flaky if CI host blocks :53 | Med | Low | Non-blocking; static-table test is the hard gate. Document in plan.md Q2. |
| Socket leak if resolve aborts before CLOSE | Low | Med | CLOSE on every exit path (RAII); only 18 caps — a leak would surface fast in repeated runs. |
| DNS compression pointer mis-parse → wrong/None IP | Low | Low | `skip_dns_name` handles 0xC0; returns None on malformed → caller yields nil, no crash. |
| Hostname > buffer overruns build_dns_query | Low | High (panic) | Bound host ≤ 253; size query buf ≥ 300; guard before copy. |

## Rollback Plan

Revert the 3 bindings + helpers, the 3 `setfield` registrations (restore `createtable
0,4`), and the 2 tests. No ABI/wire change beyond what Phase 01 owns; reverting Phase 02
alone leaves Phase 01 UDP usable from other cells. No persisted state.

## Security Considerations

- DNS response parsed defensively: every offset bounds-checked, `skip_dns_name` cannot
  loop forever (advances ≥1 each label, returns on pointer/null/OOB) — no DoS from a
  crafted reply.
- Fixed reply buffer (518 B) caps remote-controlled copy size.
- No `libs/api`/`libs/types` change → Law 1 not triggered.
- `#![forbid(unsafe_code)]` not applicable to the lua cell's FFI module (it already uses
  documented `unsafe extern "C"` per existing bindings) — new fns follow the same
  `// SAFETY:` convention.

## Next Steps

Phase E complete after both tests green. Follow-ups (out of scope, YAGNI): DNS response
caching, AAAA/IPv6 records, multiple-question queries, length-prefixed IPC to retire
zero-scan.
