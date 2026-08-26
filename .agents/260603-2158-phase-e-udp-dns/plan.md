---
title: "Phase E — UDP socket support + Lua DNS resolver"
description: "Add UDP datagram sockets to the net cell and a Lua DNS A-record resolver on top of them."
status: pending
priority: P2
effort: 5h
branch: main
tags: [networking, udp, dns, lua, smoltcp, net-cell]
created: 2026-06-03
---

# Phase E — UDP Socket Support + Lua DNS Resolver

Extends the net service cell (task 6, smoltcp 0.11) with UDP datagram sockets, then
layers a Lua `vnet.resolve()` DNS A-record resolver on top. TCP (Phases A–D) stays
untouched. 23/23 existing integration tests must continue passing.

## Architecture (data flow)

```
Lua cell                          Net cell (task 6)              QEMU SLIRP
--------                          ------------------             ----------
vnet.resolve("google.com")
  ├─ static table? ──► return immediately (no IPC)
  ├─ IPv4 literal?  ──► return immediately
  └─ DNS path:
     SOCKET_UDP ─────IPC──────►  udp::Socket::new ─► cap
     BIND(0)    ─────IPC──────►  socket.bind(eph)  ─► bound_port
     SENDTO(10.0.2.3:53,query)─► send_slice + poll ──UDP──► :53 DNS
     RECVFROM ──poll/retry────►  recv_slice ◄────────UDP─── A-record
     parse_dns_a ─► [a,b,c,d]
     CLOSE      ─────IPC──────►  sockets.remove
  └─ format_ip ─► "142.250.x.x"
```

## Phases

| # | Phase | Status | Effort | Blockers |
|---|-------|--------|--------|----------|
| 01 | [UDP socket in net cell](phase-01-udp-net-cell.md) | pending | 2.5h | — |
| 02 | [Lua UDP bindings + DNS resolver](phase-02-lua-udp-dns.md) | pending | 2.5h | Phase 01 |

Phase 02 depends on Phase 01: the Lua resolver issues SENDTO/RECVFROM IPC ops that
only exist after Phase 01 lands. Phases are sequential — do **not** parallelise (both
touch shared wire-format contract).

## File ownership (no overlap between phases)

| File | Phase 01 | Phase 02 |
|------|:--------:|:--------:|
| `cells/services/net/src/poll_driver.rs` | write | — |
| `cells/services/net/src/main.rs` | write | — |
| `cells/runtimes/lua/src/bindings_net.rs` | — | write |
| `cells/runtimes/lua/src/main.rs` | — | write |
| `tests/integration/tests/boot.rs` | — | write |

Not modified by either phase: `socket_table.rs`, `socket_state.rs`, `lua/src/ffi.rs`
(all required primitives already exist — verified).

## Acceptance criteria (whole phase)

1. `cargo check -p service-net -p lua` → 0 warnings.
2. `lua_vnet_resolve` passes — static-table fast-path, `"gateway"` → `"10.0.2.2"`.
3. `lua_vnet_resolve_dns` passes — real DNS query, output contains `.`.
4. All 23 existing integration tests still pass.

## Verified facts (re-grepped 2026-06-03)

- `cell_opcodes`: `SOCKET_UDP=0x11`, `BIND=0x16`, `GET_LOCAL_IP=0x20` exist; `0x21`/`0x22` free (poll_driver.rs:21-44).
- Stub arm `BIND | SOCKET_UDP => sys_send(&[0xFF])` at main.rs:446-449.
- Zero-scan floor `match buf[0]` block at main.rs:162-167.
- smoltcp imports at main.rs:26-31 — `socket::tcp` only; `IpAddress`/`IpEndpoint` already imported.
- `next_ephemeral_port()` at main.rs:49; `now_instant()` at main.rs:57.
- `handle_socket_syscall(opcode,cap,payload,sender,iface,device,sockets,table,local_ip)` at main.rs:233 — `iface`/`device`/`sockets` in scope (SENDTO can `iface.poll`).
- `socket_table::remove()` deletes by cap regardless of TCP/UDP (socket_table.rs:116) — works for UDP CLOSE.
- Lua `vnet` table built with `lua_createtable(L,0,4)` + 4 setfields at lua/main.rs:36-45 (must grow to 7).
- FFI present in lua/ffi.rs: `lua_pushlstring`, `lua_pushinteger`, `lua_pushnil`, `lua_pushstring`, `lua_tointegerx`, `lua_tolstring` — no ffi.rs change needed.
- `parse_ipv4` helper at bindings_net.rs:45; `vnet_connect/send/recv/close` at :66/:116/:153/:189.
- Test harness: `prerequisites_ok()` (boot.rs:42), `QemuRunner::boot`, `wait_for`, `send_line`, `dump()`; `BOOT_TIMEOUT=40`; DHCP test pattern at boot.rs:196-208.

## Unresolved questions

1. **smoltcp 0.11 `Ipv4Address` byte accessor** — the brief uses `a.as_bytes()` in the
   RECVFROM handler. Some 0.11 point releases renamed this to `.octets()` (returns
   `[u8;4]`). Implementer MUST verify against the locked `Cargo.lock` smoltcp version
   before committing; `clippy -D warnings` will catch a deprecated call. Fallback that
   works on all 0.11.x: `let o = a.octets(); reply[0..4].copy_from_slice(&o);`.
2. **`lua_vnet_resolve_dns` flakiness** — depends on QEMU SLIRP forwarding to host DNS
   (10.0.2.3:53). If the CI host blocks outbound :53, this test will time out. Marked
   non-blocking; the static-table `lua_vnet_resolve` is the reliable gate.
