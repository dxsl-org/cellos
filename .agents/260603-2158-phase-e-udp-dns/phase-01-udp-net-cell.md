# Phase 01 — UDP Socket in Net Cell

## Context Links

- Plan: [plan.md](plan.md)
- Specs: `docs/specs/07-networking.md`
- Memory: net cell IPC patterns (sys_recv returns sender_id; zero-scan floors); smoltcp State exhaustive in 0.11.

## Overview

- **Priority:** P2
- **Status:** pending
- **Blockers:** none
- Add UDP datagram support to the net service cell. Implement `SOCKET_UDP`, `BIND`,
  `SENDTO`, `RECVFROM` over the existing `[opcode:1][cap:8][payload:*]` IPC envelope.
  Remove the not-implemented stub for BIND/SOCKET_UDP.

## Key Insights (verified against codebase)

- The stub arm `cell_opcodes::BIND | cell_opcodes::SOCKET_UDP => sys_send(&[0xFF])`
  lives at **main.rs:446-449** — must be split into real handlers; the remaining
  `_ => sys_send(&[])` arm (main.rs:450-452) stays as catch-all.
- New opcodes `0x21`/`0x22` do **not** collide — `0x20` is `GET_LOCAL_IP`.
- `handle_socket_syscall` (main.rs:233) already receives `iface`, `device`, `sockets`
  → SENDTO can call `iface.poll(...)` to flush the datagram immediately, same as
  CONNECT does at main.rs:298.
- `sockets.get_mut::<udp::Socket>(handle)` is type-keyed. Calling it with a TCP handle
  (or `get_mut::<tcp::Socket>` with a UDP handle) **panics**. A UDP cap must only ever
  reach SOCKET_UDP/BIND/SENDTO/RECVFROM/CLOSE. CLOSE never type-casts (it only calls
  `sockets.remove(handle)` + `table.remove(cap)`), so it is safe for both.
- `socket_table::remove()` (socket_table.rs:116) is type-agnostic → UDP CLOSE works
  with zero changes.
- smoltcp 0.11 import block at main.rs:26-31 currently brings in `socket::tcp` only.

## Requirements

### Functional
- Create UDP socket, return CapId (or `[0;8]` on table-full).
- Bind to explicit port or auto-assign ephemeral (port 0 input).
- Send a datagram to an arbitrary `(addr,port)`; reply with bytes queued.
- Receive one datagram + source endpoint; reply empty when none pending.

### Non-functional
- `#![forbid(unsafe_code)]` holds (Cell rule — no unsafe in cells).
- `cargo check -p service-net` → 0 warnings, exhaustive matches (no unreachable `_`).
- No change to TCP code paths.

## Architecture — IPC wire format

| Opcode | Payload | Reply |
|--------|---------|-------|
| SOCKET_UDP (0x11) | — (cap bytes zero) | `[cap:8 LE]` or `[0;8]` |
| BIND (0x16) | `[port:2 LE]` (0=auto) | `[bound_port:2 LE]` or `[0xFF,0xFF]` |
| SENDTO (0x21) | `[addr:4][port:2 LE][data:*]` | `[n:4 LE]` queued |
| RECVFROM (0x22) | `[buf_len:4 LE]` | `[addr:4][port:2 LE][data:*]` or empty |
| CLOSE (0x15) | — | `[0x00]` (unchanged) |

State machine: SOCKET_UDP → `Created`; BIND → `Listening`. SENDTO/RECVFROM accepted in
any state with a valid cap (no state guard beyond cap existence — UDP is connectionless).

## Related Code Files

**Modify:**
- `cells/services/net/src/poll_driver.rs` — add `SENDTO=0x21`, `RECVFROM=0x22` to `cell_opcodes`.
- `cells/services/net/src/main.rs` — import `socket::udp`; add 4 handlers; split stub; add zero-scan floors.

**Create / delete:** none.

## Implementation Steps

1. **poll_driver.rs** — in `mod cell_opcodes`, after `GET_LOCAL_IP` (line 43), add:
   ```rust
   /// Send a UDP datagram to (addr4[4], port[2]); reply = bytes queued.
   pub const SENDTO: u8   = 0x21;
   /// Receive a UDP datagram + source endpoint; reply = [addr4][port2][data].
   pub const RECVFROM: u8 = 0x22;
   ```

2. **main.rs import** (line 28) — add `udp` to the socket import:
   ```rust
   socket::{tcp, udp},
   ```

3. **main.rs zero-scan floors** — extend the `match buf[0]` block (lines 162-167):
   ```rust
   let msg_len = match buf[0] {
       0x12 => scan_len.max(15), // CONNECT
       0x14 => scan_len.max(13), // RECV
       0x16 => scan_len.max(11), // BIND:     needs port:2
       0x17 => scan_len.max(11), // LISTEN
       0x21 => scan_len.max(15), // SENDTO:   needs addr:4 + port:2
       0x22 => scan_len.max(13), // RECVFROM: needs buf_len:4
       _    => scan_len,
   };
   ```
   Reason: SENDTO/RECVFROM/BIND carry fixed headers whose trailing byte may be 0x00
   (e.g. port 53 → `[0x35,0x00]`), which zero-scan would otherwise truncate.

4. **main.rs SOCKET_UDP handler** — mirror SOCKET_TCP:
   ```rust
   cell_opcodes::SOCKET_UDP => {
       let rx = udp::PacketBuffer::new(
           alloc::vec![udp::PacketMetadata::EMPTY; 4],
           alloc::vec![0u8; 1024],
       );
       let tx = udp::PacketBuffer::new(
           alloc::vec![udp::PacketMetadata::EMPTY; 4],
           alloc::vec![0u8; 1024],
       );
       let handle = sockets.add(udp::Socket::new(rx, tx));
       match table.insert(handle) {
           Ok(cap_id) => sys_send(sender, &cap_id.to_le_bytes()),
           Err(_)     => sys_send(sender, &[0u8; 8]),
       }
   }
   ```

5. **main.rs BIND handler** — replace the stub:
   ```rust
   cell_opcodes::BIND => {
       if payload.len() < 2 { sys_send(sender, &[0xFF, 0xFF]); return; }
       if table.get_state(cap) != Some(SocketState::Created) {
           sys_send(sender, &[0xFF, 0xFF]); return;
       }
       let requested = u16::from_le_bytes([payload[0], payload[1]]);
       let port = if requested == 0 { next_ephemeral_port() } else { requested };
       if let Some(handle) = table.get(cap) {
           let socket = sockets.get_mut::<udp::Socket>(handle);
           match socket.bind(port) {
               Ok(())  => { table.set_state(cap, SocketState::Listening);
                            sys_send(sender, &port.to_le_bytes()); }
               Err(_)  => sys_send(sender, &[0xFF, 0xFF]),
           }
       } else {
           sys_send(sender, &[0xFF, 0xFF]);
       }
   }
   ```

6. **main.rs SENDTO handler** (new arm):
   ```rust
   cell_opcodes::SENDTO => {
       if payload.len() < 6 { sys_send(sender, &0u32.to_le_bytes()); return; }
       let addr = IpAddress::v4(payload[0], payload[1], payload[2], payload[3]);
       let dst_port = u16::from_le_bytes([payload[4], payload[5]]);
       let data = &payload[6..];
       let endpoint = IpEndpoint::new(addr, dst_port);
       if let Some(handle) = table.get(cap) {
           let socket = sockets.get_mut::<udp::Socket>(handle);
           match socket.send_slice(data, endpoint) {
               Ok(())  => { iface.poll(now_instant(), device, sockets);
                            sys_send(sender, &(data.len() as u32).to_le_bytes()); }
               Err(_)  => sys_send(sender, &0u32.to_le_bytes()),
           }
       } else {
           sys_send(sender, &0u32.to_le_bytes());
       }
   }
   ```

7. **main.rs RECVFROM handler** (new arm):
   ```rust
   cell_opcodes::RECVFROM => {
       let buf_len = if payload.len() >= 4 {
           u32::from_le_bytes(payload[0..4].try_into().unwrap_or([0; 4])) as usize
       } else { 512 };
       let buf_len = buf_len.min(512);
       if let Some(handle) = table.get(cap) {
           let socket = sockets.get_mut::<udp::Socket>(handle);
           if socket.can_recv() {
               let mut data = alloc::vec![0u8; buf_len];
               match socket.recv_slice(&mut data) {
                   Ok((n, meta)) => {
                       let mut reply = alloc::vec![0u8; 6 + n];
                       if let IpAddress::Ipv4(a) = meta.endpoint.addr {
                           // VERIFY accessor: 0.11.x uses .octets() (-> [u8;4]).
                           let o = a.octets();
                           reply[0..4].copy_from_slice(&o);
                       }
                       reply[4..6].copy_from_slice(&meta.endpoint.port.to_le_bytes());
                       reply[6..6 + n].copy_from_slice(&data[..n]);
                       sys_send(sender, &reply);
                   }
                   Err(_) => sys_send(sender, &[]),
               }
           } else {
               sys_send(sender, &[]); // empty = no datagram yet
           }
       } else {
           sys_send(sender, &[]);
       }
   }
   ```

8. **main.rs stub removal** — the old `BIND | SOCKET_UDP` arm (446-449) is fully
   replaced by steps 4-5. Keep the trailing `_ => sys_send(sender, &[])` catch-all.

9. `cargo check -p service-net` → expect 0 warnings. If `IpAddress::Ipv4` pattern or
   `.octets()` errors, see Unresolved Q1 in plan.md.

## Todo List

- [ ] poll_driver.rs: add SENDTO=0x21, RECVFROM=0x22
- [ ] main.rs: add `udp` to smoltcp import
- [ ] main.rs: add 0x16/0x21/0x22 zero-scan floors
- [ ] main.rs: SOCKET_UDP handler
- [ ] main.rs: BIND handler (replaces stub)
- [ ] main.rs: SENDTO handler
- [ ] main.rs: RECVFROM handler
- [ ] main.rs: remove old BIND|SOCKET_UDP stub arm
- [ ] `cargo check -p service-net` → 0 warnings
- [ ] confirm 23 existing tests still build (no TCP path touched)

## Success Criteria

- `cargo check -p service-net` → 0 warnings, no panics introduced.
- A UDP cap routed only through UDP opcodes never hits a `get_mut::<tcp::Socket>` call.
- SENDTO flushes via `iface.poll` so a follow-up RECVFROM can observe a reply within
  the Lua retry budget.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `get_mut::<udp::Socket>` panic if a TCP cap is mis-routed | Low | High (cell crash) | Document UDP-only opcode contract; Lua resolver mints + uses its own cap (Phase 02) — never crosses types. |
| `.as_bytes()` vs `.octets()` API drift in smoltcp 0.11.x | Med | Med (compile fail) | Use `.octets()`; verify against Cargo.lock; clippy catches it pre-merge. |
| Zero-scan truncates SENDTO data ending in 0x00 | Low | Low | Existing known limitation; DNS query/binary OK because floor `.max(15)` only protects the header — payload truncation is unchanged from TCP SEND behaviour. |
| 1024-byte UDP buffers too small for large DNS responses | Low | Low | A-record responses for single hostnames are well under 512B; buf capped at 512 on recv anyway. |

## Rollback Plan

Single-cell change. Revert by restoring the `BIND | SOCKET_UDP => sys_send(&[0xFF])`
stub arm and removing the 4 handlers + 2 opcodes + import. No persisted state, no ABI
change in `libs/api` or `libs/types` → no migration. TCP untouched, so a revert cannot
cascade into Phases A–D.

## Security Considerations

- No new syscall ABI (`libs/api`/`libs/types` untouched) — Law 1 not triggered.
- UDP source endpoint comes from smoltcp's parsed header; we copy only 4+2 bytes into a
  fixed reply — no length controlled by the remote attacker beyond the capped buf_len.
- `recv_slice` bounded by `buf_len.min(512)` — no unbounded allocation from a remote.

## Next Steps

Unblocks Phase 02 (Lua resolver issues SENDTO/RECVFROM). Verify smoltcp accessor name
before starting Phase 02 so the wire contract is final.
