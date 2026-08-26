# Phase 1 — LISTEN / ACCEPT in the net cell

## Context Links

- `cells/services/net/src/main.rs:339-343` — current LISTEN/ACCEPT stub (returns `0xFF`).
- `cells/services/net/src/socket_table.rs:1-67` — `SocketTable` (target of new methods).
- `cells/services/net/src/socket_state.rs:1-17` — `SocketState` enum (drop `dead_code` allow).
- `cells/services/net/src/main.rs:200-227` — `SOCKET_TCP` handler (socket-creation reference).
- `cells/services/net/src/main.rs:228-233` — `CLOSE` handler (already correct for both cap kinds).
- `cells/services/net/src/main.rs:153-177` — `handle_ipc` dispatch (polls before+after syscall).
- `cells/services/net/src/poll_driver.rs:37-39` — `cell_opcodes::{LISTEN, ACCEPT}`.

## Overview

- **Priority:** P2 (blocker for phases 2 & 3).
- **Status:** pending.
- Replace the not-implemented LISTEN/ACCEPT stub with working smoltcp-backed
  handlers, plus the four `SocketTable` methods they need.

## Key Insights

1. **smoltcp has no accept queue.** A socket in `Listen` state that completes a
   handshake transitions to `Established` *in place* — the listener IS the
   connection. To keep serving, ACCEPT must spin up a brand-new listening socket
   on the same port and repoint the listener cap at it.
2. **`handle_ipc` already polls before AND after** the syscall (`main.rs:171,173`).
   The original draft's inline `iface.poll()` inside ACCEPT is therefore redundant.
   Keep ACCEPT free of its own poll to avoid double-polling; the pre-syscall poll
   at `main.rs:171` has already advanced the stack so `state()` is current.
3. **Sentinel choice.** ACCEPT "not yet" replies `[0xFF; 8]` = `u64::MAX`. Real
   caps come from `next_cap` starting at 1 (`socket_table.rs:27`), so no collision.
4. **`listen()` state guard.** smoltcp `tcp::Socket::listen()` returns
   `ListenError` unless the socket is in `Closed` state. A freshly created socket
   (`SOCKET_TCP`) is `Closed`, so LISTEN on a `Created` cap is valid; reject any
   other `SocketState`.

## Requirements

**Functional**
- LISTEN binds the cap's socket to a local port and arms it for inbound SYNs.
- ACCEPT returns a new stream cap once the handshake completes, and renews the
  listener so it can accept again.
- CLOSE on either a listener cap or a stream cap frees the underlying smoltcp
  socket (existing handler already does this — no change required).

**Non-functional**
- 0 clippy warnings under `-D warnings`.
- No `unsafe` (this is a Cell: `#![forbid(unsafe_code)]`).
- Owned buffers only (Law 2) — `SocketBuffer::new(alloc::vec![...])`.

## Architecture / Data flow

```
LISTEN: Created ──listen(port)──► Listening   (set_listen_port records the port)
ACCEPT: Listening + Established ──► mint stream_cap (Connected)
                                └─► new socket.listen(port) ──► listener cap repointed, stays Listening
```

The listener cap's `SocketHandle` is **swapped** during ACCEPT via
`update_handle`. The old handle is NOT removed from the `SocketSet` — it now
belongs to the stream cap (it is the established connection). The new listening
socket is added fresh.

## Related Code Files

**Modify**
- `cells/services/net/src/socket_table.rs` — add `listen_ports` field + 4 methods.
- `cells/services/net/src/main.rs` — replace stub arm with LISTEN + ACCEPT.
- `cells/services/net/src/socket_state.rs` — remove `#[allow(dead_code)]`.

**Create / Delete:** none.

## Implementation Steps

### Step 1 — `socket_table.rs`: add `listen_ports` field

Add to the struct (after `next_cap`, line 22) and update both constructors
(`#[derive(Default)]` at line 18 and `new()` at line 26-28):

```rust
use smoltcp::iface::SocketHandle;            // already imported (line 10)

#[derive(Default)]
pub struct SocketTable {
    entries: BTreeMap<u64, SocketHandle>,
    states:  BTreeMap<u64, SocketState>,
    listen_ports: BTreeMap<u64, u16>,        // NEW: cap → bound listen port
    next_cap: u64,
}

impl SocketTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            states: BTreeMap::new(),
            listen_ports: BTreeMap::new(),   // NEW
            next_cap: 1,
        }
    }
```

### Step 2 — `socket_table.rs`: add four methods

Append inside `impl SocketTable` (before the closing brace, after `remove` at
line 66):

```rust
    /// Record the port a listening socket is bound to, so ACCEPT can renew it.
    pub fn set_listen_port(&mut self, cap: u64, port: u16) {
        if self.entries.contains_key(&cap) {
            self.listen_ports.insert(cap, port);
        }
    }

    /// Read the bound listen port for `cap`, if any.
    pub fn get_listen_port(&self, cap: u64) -> Option<u16> {
        self.listen_ports.get(&cap).copied()
    }

    /// Repoint an existing cap at a new smoltcp handle (used by ACCEPT to renew
    /// the listener socket without changing the cap the consumer already holds).
    pub fn update_handle(&mut self, cap: u64, new_handle: SocketHandle) {
        if self.entries.contains_key(&cap) {
            self.entries.insert(cap, new_handle);
        }
    }

    /// Allocate a new `CapId` for `handle` with an explicit initial state.
    ///
    /// Unlike `insert` (which defaults to `Created`), this sets `state` directly.
    /// ONLY call from ACCEPT, where the new socket is already `Established` and
    /// must be surfaced as `Connected` to the consumer.
    ///
    /// # Errors
    /// Returns `ViError::OutOfMemory` if `MAX_SOCKETS` is already reached.
    pub fn insert_with_state(
        &mut self,
        handle: SocketHandle,
        state: SocketState,
    ) -> Result<u64, ViError> {
        if self.entries.len() >= MAX_SOCKETS {
            return Err(ViError::OutOfMemory);
        }
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries.insert(cap, handle);
        self.states.insert(cap, state);
        Ok(cap)
    }
```

Also update `remove` (line 63-66) to drop the `listen_ports` entry for cleanliness:

```rust
    pub fn remove(&mut self, cap: u64) -> Option<SocketHandle> {
        self.states.remove(&cap);
        self.listen_ports.remove(&cap);      // NEW
        self.entries.remove(&cap)
    }
```

### Step 3 — `main.rs`: replace the stub arm

Replace the arm at `main.rs:339-343`:

```rust
        cell_opcodes::BIND | cell_opcodes::LISTEN | cell_opcodes::ACCEPT
        | cell_opcodes::SOCKET_UDP => {
            let _ = (cap, payload);
            sys_send(sender, &[0xFF]); // not-yet-implemented
        }
```

with the following three arms:

```rust
        cell_opcodes::LISTEN => {
            // [0x17][cap:8][port:2 LE] → [0x00] ok / [0x01] err.
            // Only a freshly created socket (smoltcp Closed) may listen.
            if payload.len() < 2 {
                sys_send(sender, &[0x01]);
                return;
            }
            if table.get_state(cap) != Some(SocketState::Created) {
                sys_send(sender, &[0x01]);
                return;
            }
            let port = u16::from_le_bytes([payload[0], payload[1]]);
            if let Some(handle) = table.get(cap) {
                let socket = sockets.get_mut::<tcp::Socket>(handle);
                match socket.listen(port) {
                    Ok(()) => {
                        table.set_state(cap, SocketState::Listening);
                        table.set_listen_port(cap, port);
                        sys_send(sender, &[0x00]);
                    }
                    Err(_) => sys_send(sender, &[0x01]),
                }
            } else {
                sys_send(sender, &[0x01]);
            }
        }
        cell_opcodes::ACCEPT => {
            // [0x18][cap:8] → [stream_cap:8 LE] or [0xFF;8] if not connected yet.
            // handle_ipc already polled smoltcp before this call (main.rs:171),
            // so socket.state() reflects the current handshake progress.
            if table.get_state(cap) != Some(SocketState::Listening) {
                sys_send(sender, &[0xFF_u8; 8]);
                return;
            }
            let handle = match table.get(cap) {
                Some(h) => h,
                None => { sys_send(sender, &[0xFF_u8; 8]); return; }
            };
            {
                let s = sockets.get_mut::<tcp::Socket>(handle);
                if s.state() != tcp::State::Established {
                    sys_send(sender, &[0xFF_u8; 8]);
                    return;
                }
            }
            // Handshake done. The listener socket IS the connection now.
            let listen_port = table.get_listen_port(cap).unwrap_or(0);
            match table.insert_with_state(handle, SocketState::Connected) {
                Ok(stream_cap) => {
                    // Renew the listener: fresh socket on the same port.
                    let rx = tcp::SocketBuffer::new(alloc::vec![0u8; 4096]);
                    let tx = tcp::SocketBuffer::new(alloc::vec![0u8; 4096]);
                    let mut new_sock = tcp::Socket::new(rx, tx);
                    let _ = new_sock.listen(listen_port);
                    let new_handle = sockets.add(new_sock);
                    table.update_handle(cap, new_handle);
                    table.set_state(cap, SocketState::Listening);
                    table.set_listen_port(cap, listen_port);
                    sys_send(sender, &stream_cap.to_le_bytes());
                }
                Err(_) => {
                    // Table full — cannot mint a stream cap. Leave listener as-is.
                    sys_send(sender, &[0xFF_u8; 8]);
                }
            }
        }
        cell_opcodes::BIND | cell_opcodes::SOCKET_UDP => {
            let _ = (cap, payload);
            sys_send(sender, &[0xFF]); // not-yet-implemented
        }
```

> **Borrow note:** the `s.state()` check is scoped in its own `{ }` block so the
> `&mut Socket` borrow of `sockets` ends before `sockets.add(new_sock)` later
> reborrows the `SocketSet`. Without the block the borrow checker rejects it.

> **Capacity note:** ACCEPT calls `sockets.add()` (one new listener) and
> `insert_with_state()` (one new cap) for the SAME established handle. Net socket
> count grows by 1 per accepted connection. `MAX_SOCKETS = 18` (`socket_table.rs:15`)
> bounds this. For the single-connection test this is well within budget.

### Step 4 — `socket_state.rs`: remove the dead-code allow

Both `Listening` and `Closed` are now referenced (`Listening` actively;
`Closed` remains reserved). Remove line 4:

```rust
#[allow(dead_code)] // reason: Listening + Closed reserved ...   ← DELETE this line
```

If clippy still flags `Closed` as unused after deletion, keep a narrower allow on
just that variant rather than the whole enum, or leave the line if `Closed` has no
constructor yet. Verify with `cargo clippy -p service-net -- -D warnings` and
choose the minimal silencing.

## Todo List

- [ ] socket_table.rs: add `listen_ports` field + update `new()`
- [ ] socket_table.rs: add `set_listen_port`, `get_listen_port`, `update_handle`, `insert_with_state`
- [ ] socket_table.rs: drop `listen_ports` entry in `remove`
- [ ] main.rs: replace stub arm with LISTEN + ACCEPT + residual BIND|SOCKET_UDP
- [ ] socket_state.rs: remove (or narrow) the `#[allow(dead_code)]`
- [ ] `cargo check -p service-net` and `cargo clippy -p service-net -- -D warnings` clean

## Success Criteria

- LISTEN on a `Created` cap returns `[0x00]` and flips state to `Listening`.
- LISTEN on any other state / unknown cap / short payload returns `[0x01]`.
- ACCEPT on a non-`Listening` cap returns `[0xFF; 8]`.
- ACCEPT while not yet `Established` returns `[0xFF; 8]`.
- ACCEPT on an `Established` listener returns a small positive `stream_cap` and
  the listener cap is back in `Listening` with a fresh socket.
- 0 clippy warnings.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `listen()` called on non-Closed socket → `ListenError` | Med | Low | State guard: only `Created`→`Listening`; match `Err(_)` → `[0x01]`. |
| Borrow conflict (`get_mut` then `add`) | High | Compile fail | Scope the `state()` check in its own `{ }` block (Step 3 note). |
| Listener handle leaked after renew | Low | Med (socket exhaustion) | Old handle is reused as the stream cap — not leaked. Each accept nets +1 socket, bounded by MAX_SOCKETS=18. |
| `insert_with_state` misused outside ACCEPT | Low | Med | Doc-comment restricts usage; only one call site. |
| Double-poll if ACCEPT adds its own `iface.poll()` | Med | Low (perf) | Rely on `handle_ipc`'s pre/post polls; ACCEPT does NOT poll. |

## Backwards Compatibility

- Wire protocol is additive: new opcodes 0x17/0x18 were previously `0xFF` stubs;
  no existing client sent them. CONNECT/SEND/RECV/CLOSE/SOCKET_STATE unchanged.
- `SocketTable` gains a field and methods — purely additive; existing call sites
  in `main.rs` (insert/get/get_state/set_state/remove) are untouched.

## Security Considerations

- ACCEPT only acts on caps the consumer already owns (cap is the capability).
- No port-ownership check across cells (single-tenant guest, SAS) — acceptable
  for current threat model; note for future multi-tenant hardening.

## Next Steps

- Unblocks Phase 2 (nc server mode) and Phase 3 (integration test).
- Rollback: revert the three files; opcodes return to `0xFF` stub. No persisted
  state, no migration — clean revert.

## Unresolved Questions

- Does `cargo clippy` still flag `SocketState::Closed` as unused after Step 4? If
  so, the implementer must decide between a narrow per-variant allow vs. leaving
  the enum-level allow until a `Closed` constructor lands. Verify during impl.
