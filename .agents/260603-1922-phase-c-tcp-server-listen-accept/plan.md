---
title: "Phase C: TCP server (LISTEN/ACCEPT) + hostname stub"
description: "Close the last TCP socket-API gap — wire LISTEN/ACCEPT in the net cell, add nc -l server mode + hostname resolution, prove it with a guest-as-server integration test."
status: pending
priority: P2
effort: 5h
branch: main
tags: [networking, tcp, smoltcp, net-cell, integration-test]
created: 2026-06-03
---

# Phase C — TCP Server (LISTEN / ACCEPT) + Hostname Stub

Phase A wired TCP CONNECT/SEND/RECV; Phase B added HTTP/1.0 GET. Both pass (23/23
integration tests). Phase C closes the last gap: the **server** side of the TCP
socket API — `LISTEN` (0x17) and `ACCEPT` (0x18), today stubbed to `0xFF` at
`cells/services/net/src/main.rs:339-343`.

After Phase C the guest can act as a TCP server: `nc -l <port>` listens, accepts
one inbound connection, echoes received bytes to serial, and closes.

## Architecture in one paragraph

`nc -l <port>` (guest, ring-3 Cell) sends `LISTEN` to the net service cell
(task 6) over IPC. The net cell calls `smoltcp tcp::Socket::listen(port)` and
flips the cap's `SocketState` to `Listening`. The host test client connects to
the guest through QEMU SLIRP **hostfwd** (`tcp:127.0.0.1:<host_port>-:<guest_port>`).
When the handshake completes, the listening smoltcp socket itself becomes
`Established` (smoltcp has **no separate accept queue**). `ACCEPT` therefore:
(1) mints a new stream cap pointing at the now-established handle, (2) creates a
**fresh** listening socket on the same port, and (3) repoints the listener cap at
that new handle — so the listener stays reusable.

## Data flow (LISTEN → ACCEPT → echo)

```
guest nc          net cell (task 6)                smoltcp SocketSet         host client
  │  LISTEN 0x17 ─────►│ socket.listen(port)              │                       │
  │                    │ state: Created→Listening         │                       │
  │  ◄── [0x00] ───────│                                  │                       │
  │  (println listening)                                  │                       │
  │                    │                          ◄─ SYN ─────────────────────────│ connect()
  │                    │ iface.poll() → Established│                              │
  │  ACCEPT 0x18 ─────►│ mint stream_cap=handle           │                       │
  │                    │ new listen socket on same port   │                       │
  │                    │ listener cap → new_handle        │                       │
  │  ◄ stream_cap:8 ───│                                  │                       │
  │  (println connected)                                  │                       │
  │  RECV 0x14 ───────►│ recv_slice                ◄─ "PING_ViCell\n" ──────────────│ write
  │  ◄── bytes ────────│                                  │                       │
  │  (print to serial; test sees "PING_ViCell")             │                       │
  │  SEND 0x13 ───────►│ send_slice ──────────────────────────────────────────► (echo back)
  │  CLOSE 0x15 (stream)│ table.remove + sockets.remove   │                       │
  │  CLOSE 0x15 (listener)│ table.remove + sockets.remove │                       │
```

## Phases

| # | File | Status | Owns (no overlap) | Depends on |
|---|------|--------|-------------------|------------|
| 1 | [phase-01-listen-accept-net-cell.md](phase-01-listen-accept-net-cell.md) | pending | `cells/services/net/src/socket_table.rs`, `cells/services/net/src/main.rs`, `cells/services/net/src/socket_state.rs` | — |
| 2 | [phase-02-nc-server-mode.md](phase-02-nc-server-mode.md) | pending | `cells/apps/net-tools/src/bin/nc.rs`, `cells/apps/net-tools/src/bin/curl.rs` | Phase 1 (wire protocol) |
| 3 | [phase-03-integration-test.md](phase-03-integration-test.md) | pending | `tests/integration/src/lib.rs`, `tests/integration/tests/boot.rs` | Phase 1 + 2 |

File ownership is disjoint across phases — phases 1 and 2 touch different crates
and can be implemented in parallel against the agreed wire contract, then phase 3
validates the integrated whole.

## Wire contract (frozen — both phases depend on this)

IPC envelope for all net-cell messages: `[opcode:1][cap:8 LE][payload:*]`.

| Op | Code | Request payload | Reply |
|----|------|-----------------|-------|
| LISTEN | 0x17 | `[port:2 LE]` | `[0x00]` ok / `[0x01]` err |
| ACCEPT | 0x18 | (none) | `[stream_cap:8 LE]` or `[0xFF; 8]` if no connection yet |

`ACCEPT` reply of all-`0xFF` (= `u64::MAX`) is the sentinel for "not yet". A
genuine stream cap is a small positive integer (caps start at 1, see
`socket_table.rs:27`), so the sentinel never collides with a real cap.

## Key dependencies / preconditions (already in place — do NOT re-do)

- `SocketState::{Listening, Closed}` exist (`socket_state.rs:13-16`), currently
  under `#[allow(dead_code)]` at `socket_state.rs:4`.
- `cell_opcodes::{LISTEN=0x17, ACCEPT=0x18}` defined (`poll_driver.rs:37-39`).
- `decode_message` already parses the `[opcode][cap:8][payload]` envelope and
  rejects buffers `< 9` bytes (`poll_driver.rs:67`).
- `handle_ipc` polls smoltcp **before and after** every cell syscall
  (`main.rs:171-173`) — so ACCEPT does **not** need its own extra `iface.poll()`.
- `tcp_state_byte`: `Established=0x03`, `CloseWait=0x06`, `Closed=0x00`,
  `Listen=0x0A` (`main.rs:184-198`).
- `next_ephemeral_port()` / `NEXT_PORT` allocator (`main.rs:47-55`) — **not used
  by LISTEN** (server binds the fixed listen port), kept for CONNECT.
- Shell routes unknown commands to `/bin/<prog>` via `spawn_external`, publishing
  argv through `sys_set_spawn_args` (`executor.rs:171,197-198`). `nc -l 9090`
  flows through unchanged — **no shell edit needed**.
- Task IDs: net = 6 (`nc.rs:13`).

## Global acceptance criteria

1. `cargo check -p service-net -p app-net-tools` → 0 warnings (clippy `-D warnings`).
2. `network_tcp_listen_accept` integration test passes (serial shows `PING_ViCell`).
3. All 23 existing integration tests still pass (no regression).
4. `nc -l <port>` prints `listening on <port>` then `connected` after accept.
5. LISTEN returns `[0x00]`; ACCEPT returns a non-sentinel `stream_cap` on connect.
6. CLOSE after accept removes both sockets from smoltcp's `SocketSet`.

## Unresolved questions

See each phase file's "Unresolved Questions" section; the consolidated list is at
the bottom of phase-03.
