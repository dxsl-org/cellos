# Phase 2 — `nc -l` server mode + hostname stub

## Context Links

- `cells/apps/net-tools/src/bin/nc.rs:1-188` — current client-only nc.
- `cells/apps/net-tools/src/bin/curl.rs:51-62,239-262` — URL parse + `parse_ipv4`.
- `cells/services/net/src/main.rs` (Phase 1) — LISTEN/ACCEPT wire handlers.
- `cells/apps/shell/src/executor.rs:171,197-198` — `spawn_external` publishes argv,
  no shell change needed for `nc -l`.
- `nc.rs:135` — uses `ostd::io::print` fully-qualified (no `print` import); server
  mode must do likewise OR add `print` to the `use` line.

## Overview

- **Priority:** P2.
- **Status:** pending.
- Add `-l <port>` server mode to nc: LISTEN, poll ACCEPT, echo received bytes to
  serial AND back to the peer, exit on remote close. Add a small hostname-resolution
  stub used by both nc (client mode) and curl.

## Key Insights

1. **Argv already flows.** `nc -l 9090` is not a shell builtin, so it routes to
   `spawn_external("nc", ["-l","9090"])` → `sys_set_spawn_args("-l 9090")` →
   nc reads it via `sys_spawn_args` (`nc.rs:33`). No shell edit.
2. **The test detects success via serial.** nc MUST print received bytes (so
   `wait_for("PING_ViCell")` fires). Echoing back to the peer is secondary but keeps
   nc behaving like a real echo server.
3. **ACCEPT is poll-based.** Reply `[0xFF;8]` (=`u64::MAX`) means "retry". A real
   stream cap is small. Loop with `sys_yield()` between retries, bounded.
4. **nc.rs imports only `println`** from `ostd::io` (`nc.rs:5`). Client mode calls
   `ostd::io::print` fully-qualified (`nc.rs:135`). Either add `print` to the use
   import or keep fully-qualifying — pick one for consistency.

## Requirements

**Functional**
- `nc -l <port>`: listen, accept one connection, echo received data to serial and
  back to peer, close when peer closes (state CloseWait/Closed or 0-byte + closed).
- `nc <host> <port>`: unchanged client behavior, but `host` now passes through
  `resolve_host` (hostname stub → IPv4 fallback).
- curl: `host` passes through `resolve_host` too.

**Non-functional**
- No heap / `extern crate alloc` (SAS BSS-overlap hazard — see `curl.rs:23-31`).
  Use fixed stack buffers, as existing nc/curl do.
- 0 clippy warnings.

## Architecture / Data flow (server_mode)

```
SOCKET_TCP [0x10] ──────────► cap
LISTEN     [0x17][cap][port] ► [0x00]   → println("listening on <port>")
loop ACCEPT[0x18][cap]:
   [0xFF;8] → sys_yield, retry (bounded ~5000)
   stream_cap (≠ u64::MAX) → break  → println("connected")
loop RECV  [0x14][stream_cap][256]:
   bytes>0 → print to serial; SEND [0x13][stream_cap][bytes] back to peer
   bytes==0 → query SOCKET_STATE [0x19][stream_cap]:
                CloseWait(0x06)/Closed(0x00) → break
                else → sys_yield, retry (bounded)
CLOSE [0x15][stream_cap]
CLOSE [0x15][cap]   (listener — optional but clean)
```

## Related Code Files

**Modify**
- `cells/apps/net-tools/src/bin/nc.rs` — add `-l` parse, `server_mode`,
  `resolve_host`, LISTEN/ACCEPT/SOCKET_STATE opcode consts.
- `cells/apps/net-tools/src/bin/curl.rs` — add `resolve_host`, route `host` through it.

**Create / Delete:** none.

## Implementation Steps

### Step 1 — nc.rs: add opcode constants

After the existing consts (`nc.rs:16-20`):

```rust
const LISTEN_OP:   u8 = 0x17;
const ACCEPT_OP:   u8 = 0x18;
const STATE_OP:    u8 = 0x19;
```

Add `print` to the io import (`nc.rs:5`) so server mode can stream bytes:

```rust
use ostd::io::{print, println};
```

### Step 2 — nc.rs: branch on `-l` in `main`

Insert at the top of `main`, right after the argv string is parsed
(`nc.rs:38-50`, after `args_str` is in hand) — before the client SOCKET_TCP path:

```rust
    let mut parts = args_str.split_whitespace();
    let first = match parts.next() {
        Some(t) => t,
        None => { println("Usage: nc <host> <port>  |  nc -l <port>"); return; }
    };

    if first == "-l" {
        // Server mode: nc -l <port>
        let port = match parts.next().and_then(parse_u16) {
            Some(p) => p,
            None => { println("Usage: nc -l <port>"); return; }
        };
        server_mode(port);
        return;
    }

    // Client mode: `first` is the host token.
    let host = first;
    let port_str = match parts.next() {
        Some(p) => p,
        None => { println("Usage: nc <host> <port>"); return; }
    };
    let addr = match resolve_host(host) {
        Some(a) => a,
        None => { println("nc: invalid host"); return; }
    };
    let port: u16 = match parse_u16(port_str) {
        Some(p) => p,
        None => { println("nc: invalid port"); return; }
    };
```

> This **replaces** the existing client-arg parsing block at `nc.rs:42-58`
> (the `let mut parts`, `host`, `port_str`, `addr`, `port` lines). The client SEND/
> RECV body below (`nc.rs:60-144`) stays as-is. Note `parse_ipv4(host)` at
> `nc.rs:51` becomes `resolve_host(host)`.

### Step 3 — nc.rs: add `server_mode`

Add as a free function (mirrors the IPC style of client mode; fixed buffers, no alloc):

```rust
/// nc -l <port> — listen, accept one connection, echo bytes to serial and back
/// to the peer, then close when the peer closes.
fn server_mode(port: u16) {
    // SOCKET_TCP → cap
    let socket_msg = [SOCKET_TCP, 0, 0, 0, 0, 0, 0, 0, 0];
    sys_send(NET_ENDPOINT, &socket_msg);
    let mut cap_reply = [0u8; 8];
    let cap = match sys_recv(0, &mut cap_reply) {
        SyscallResult::Ok(_) => u64::from_le_bytes(cap_reply),
        _ => { println("nc: SOCKET_TCP failed"); return; }
    };
    if cap == 0 { println("nc: no socket cap"); return; }

    // LISTEN [0x17][cap:8][port:2 LE] → [0x00] ok
    let mut listen_msg = [0u8; 11];
    listen_msg[0] = LISTEN_OP;
    listen_msg[1..9].copy_from_slice(&cap.to_le_bytes());
    listen_msg[9..11].copy_from_slice(&port.to_le_bytes());
    sys_send(NET_ENDPOINT, &listen_msg);
    let mut ack = [0u8; 1];
    match sys_recv(0, &mut ack) {
        SyscallResult::Ok(_) if ack[0] == 0x00 => {}
        _ => { println("nc: listen failed"); close_socket(cap); return; }
    }
    // Test gate: "listening on <port>". print_usize avoids alloc/format.
    print("listening on ");
    ostd::io::print_usize(port as usize);
    println("");

    // ACCEPT [0x18][cap:8] → stream_cap, or u64::MAX = retry.
    let mut accept_msg = [0u8; 9];
    accept_msg[0] = ACCEPT_OP;
    accept_msg[1..9].copy_from_slice(&cap.to_le_bytes());
    let mut stream_cap = 0u64;
    for _ in 0..5000 {
        sys_send(NET_ENDPOINT, &accept_msg);
        let mut r = [0u8; 8];
        match sys_recv(0, &mut r) {
            SyscallResult::Ok(_) => {
                let c = u64::from_le_bytes(r);
                if c != u64::MAX && c != 0 { stream_cap = c; break; }
                sys_yield();
            }
            _ => { sys_yield(); }
        }
    }
    if stream_cap == 0 {
        println("nc: accept timed out");
        close_socket(cap);
        return;
    }
    println("connected");

    // RECV loop: print to serial AND echo back. Exit on peer close.
    let mut recv_msg = [0u8; 13];
    recv_msg[0] = RECV_OP;
    recv_msg[1..9].copy_from_slice(&stream_cap.to_le_bytes());
    recv_msg[9..13].copy_from_slice(&256u32.to_le_bytes());

    for _ in 0..5000 {
        let mut data = [0u8; 256];
        sys_send(NET_ENDPOINT, &recv_msg);
        match sys_recv(0, &mut data) {
            SyscallResult::Ok(_) if data[0] != 0 => {
                let end = data.iter().position(|&b| b == 0).unwrap_or(256);
                if let Ok(s) = core::str::from_utf8(&data[..end]) {
                    print(s);
                }
                // Echo back: SEND [0x13][stream_cap:8][data].
                let mut send_msg = [0u8; 9 + 256];
                send_msg[0] = SEND_OP;
                send_msg[1..9].copy_from_slice(&stream_cap.to_le_bytes());
                send_msg[9..9 + end].copy_from_slice(&data[..end]);
                sys_send(NET_ENDPOINT, &send_msg[..9 + end]);
                let mut cnt = [0u8; 4];
                let _ = sys_recv(0, &mut cnt);
            }
            SyscallResult::Ok(_) => {
                // 0 bytes: distinguish "no data yet" from "peer closed".
                if query_state(stream_cap) == 0x06 // CloseWait
                    || query_state(stream_cap) == 0x00 // Closed
                {
                    break;
                }
                sys_yield();
            }
            _ => break,
        }
    }

    close_socket(stream_cap);
    close_socket(cap); // listener — optional but clean
}

/// Query SOCKET_STATE (0x19) → 1-byte smoltcp state code.
fn query_state(cap: u64) -> u8 {
    let mut msg = [0u8; 9];
    msg[0] = STATE_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    sys_send(NET_ENDPOINT, &msg);
    let mut st = [0u8; 1];
    match sys_recv(0, &mut st) {
        SyscallResult::Ok(_) => st[0],
        _ => 0x00,
    }
}
```

> **`print_usize` check:** curl/nc use `ostd::io::print_usize` (it exists — used in
> shell `executor.rs` print_jobs). If `print_usize` is NOT exported from `ostd::io`,
> render the port with a tiny stack itoa helper instead. **Verify the symbol before
> relying on it** (`grep "pub fn print_usize" libs/ostd`). The test only needs the
> literal substring "listening" — printing the number is cosmetic, so a fallback
> that prints just "listening on port" is acceptable if print_usize is absent.

### Step 4 — nc.rs: add `resolve_host`

```rust
/// Resolve a hostname stub to an IPv4 address, falling back to literal parsing.
/// Static table only — no DNS. SLIRP gateway/DNS/loopback aliases for tests.
fn resolve_host(s: &str) -> Option<[u8; 4]> {
    match s {
        "gateway" | "host" => Some([10, 0, 2, 2]),
        "dns"              => Some([10, 0, 2, 3]),
        "localhost"        => Some([127, 0, 0, 1]),
        _                  => parse_ipv4(s),
    }
}
```

### Step 5 — curl.rs: route host through `resolve_host`

Add the same `resolve_host` function to curl.rs and replace the `parse_ipv4(host)`
call at `curl.rs:59`:

```rust
    let addr = match resolve_host(host) {
        Some(a) => a,
        None => { println("curl: invalid host"); return; }
    };
```

`resolve_host` and `parse_ipv4` are duplicated across the two binaries. They are
separate `[[bin]]` crates with no shared module, so duplication here is the KISS
choice over introducing a shared lib for ~10 lines. If a shared `net-tools` lib
module already exists, prefer placing `resolve_host` there instead — verify during
impl (`grep "mod " cells/apps/net-tools/src/`).

## Todo List

- [ ] nc.rs: add LISTEN_OP/ACCEPT_OP/STATE_OP consts + `print` import
- [ ] nc.rs: branch on `-l` in main, route client host through `resolve_host`
- [ ] nc.rs: add `server_mode`, `query_state`, `resolve_host`
- [ ] nc.rs: verify `print_usize` exists or add itoa fallback
- [ ] curl.rs: add `resolve_host`, route host through it
- [ ] `cargo check -p app-net-tools` + clippy clean

## Success Criteria

- `nc -l 9090` prints `listening on 9090`.
- After a host client connects, nc prints `connected`.
- Bytes the client sends appear on guest serial and are echoed back to the client.
- nc exits cleanly when the client closes (no hang past the bounded loops).
- `nc gateway 80` resolves to 10.0.2.2 (client path unbroken).
- 0 clippy warnings.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `print_usize` not in `ostd::io` | Med | Low | Verify symbol; fallback prints "listening on port" (literal "listening" suffices for test). |
| ACCEPT poll never resolves (no connection) | Med | Med (hang) | Bounded 5000-iter loop + `sys_yield`; prints "accept timed out" and closes. |
| RECV loop never sees peer close | Low | Med (hang) | Bounded 5000-iter loop; `query_state` CloseWait/Closed exit. |
| Echo SEND back-pressured (n<len) | Low | Low | Test payload is tiny (10B); single SEND fits 4096B tx buffer. Best-effort echo acceptable. |
| Stack frame too large (256+256 buffers) | Low | Med (stack overflow) | Buffers are 256B each, well under typical Cell stack; matches existing curl 512B buffers. |
| Duplicated resolve_host drifts vs curl | Low | Low | Keep identical; note shared-lib option if a module exists. |

## Backwards Compatibility

- Client mode `nc <host> <port>` unchanged except host now accepts aliases
  (superset of prior IPv4-only behavior). Existing `network_tcp_send_recv` test
  passes `10.0.2.2` which `resolve_host` returns via the `parse_ipv4` fallback.
- curl URL parsing unchanged; only the host→addr step gains alias support.

## Security Considerations

- Static hostname table only — no DNS resolver, no network lookups. No attack
  surface beyond literal IPv4 parsing already present.

## Next Steps

- Unblocks Phase 3 integration test.
- Rollback: revert nc.rs/curl.rs. Client mode + curl return to IPv4-literal only.
  No persisted state.

## Unresolved Questions

- Is `ostd::io::print_usize` available to net-tools binaries? (Used by shell —
  verify it is `pub` and reachable from this crate.)
- Does net-tools have a shared lib module where `resolve_host` belongs, or are the
  binaries fully independent? (Affects DRY vs KISS decision in Step 5.)
