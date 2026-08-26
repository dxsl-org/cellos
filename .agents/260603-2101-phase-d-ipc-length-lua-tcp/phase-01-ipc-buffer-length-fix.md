# Phase 01 — IPC Buffer Length Fix (Net Cell)

## Context Links
- Bug source: Phase C code review (HIGH severity)
- File: `cells/services/net/src/main.rs`
- Verified-no-change: `cells/services/net/src/poll_driver.rs` (`decode_message` already `&[u8]`)
- Regression guard test: `tests/integration/tests/boot.rs::network_curl_http_get` (`:270`)

## Overview
- **Priority:** P1 (HIGH — corrupts every TCP SEND with >0 trailing buffer)
- **Status:** pending
- **Description:** The net cell reuses a single `[0u8; 512]` receive buffer across loop iterations. `decode_message` returns `payload = &buf[9..]` — the full 503-byte tail. The SEND handler forwards all 503 bytes to `socket.send_slice`, appending stale bytes from the previous message to the real payload.

## Key Insights
- `sys_try_recv` returns `Ok(sender_id)`, **NOT a byte count** — so the cell never learns the real message length from the syscall. This is why length must be recovered another way.
- `decode_message` at `poll_driver.rs:58` **already accepts `&[u8]`** (a slice). The fix is purely in `main.rs`: zero the buffer, compute `msg_len`, and pass `&buf[..msg_len]`.
- The same `buf` carries kernel RxFrame messages too. For RxFrame, smoltcp reads the IP total-length field from the frame and ignores the buffer tail — trailing zeros are harmless. So a single zero-fill strategy is correct for both message types.
- `handle_ipc` signature is `buf: &[u8; 512]` (`main.rs:155`). Change to `buf: &[u8]` so a slice can be passed.

## The Zero-Scan Limitation (DOCUMENT EXPLICITLY)
Recovering length by scanning for the last non-zero byte:
```rust
buf.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0)
```
**fails for binary payloads whose last byte is `0x00`** — the trailing NUL would be truncated.
- Current callers (`nc`, `curl`, and the new Lua `vnet`) send ASCII / text HTTP that never ends in NUL, so this is **acceptable for Phase D**.
- A robust fix (length-prefixed IPC framing) is deferred. Add a `// LIMITATION:` comment at the scan site so the constraint is discoverable.
- RxFrame is unaffected: smoltcp uses the IP length field, not the scanned length.

## Architecture / Data Flow
```
loop iteration:
  buf.fill(0)                      ← NEW: clear stale bytes
  sys_try_recv(0, &mut buf) → Ok(sender)
  msg_len = rposition(non-zero)+1  ← NEW: recover true length
  handle_ipc(&buf[..msg_len], ...) ← CHANGED: pass slice, not &[u8;512]
     └ decode_message(&buf[..msg_len])  (unchanged — already &[u8])
         ├ RxFrame(&buf[1..msg_len])    → push_rx (tail-safe, IP len used)
         └ CellRequest{ payload: &buf[9..msg_len] }
              └ SEND: socket.send_slice(payload)  ← now exactly the real bytes
```

## Related Code Files
**Modify:**
- `cells/services/net/src/main.rs` — `main()` loop (`:90`–`:150`) + `handle_ipc` signature (`:154`–`:155`)

**No change (verified):**
- `cells/services/net/src/poll_driver.rs` — `decode_message` already takes `&[u8]`

**Delete:** none.

## Implementation Steps

### Step 1 — Zero the buffer + compute length in the receive arm
Replace the `match sys_try_recv(...)` block at `main.rs:133`–`:149`:

```rust
        // ── Receive one IPC message (non-blocking) ────────────────────────────
        // Pre-zero the reused buffer so stale bytes from the previous message
        // cannot leak into this one. sys_try_recv returns the sender id, NOT a
        // byte count, so the true payload length is recovered by scanning for
        // the last non-zero byte after the receive.
        buf.fill(0);
        match sys_try_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                // LIMITATION: zero-scan truncates a payload whose final byte is
                // 0x00. All current senders (nc/curl/lua vnet) transmit ASCII
                // text that never ends in NUL, so this is acceptable for now.
                // A length-prefixed IPC frame is the proper long-term fix.
                let msg_len = buf
                    .iter()
                    .rposition(|&b| b != 0)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                handle_ipc(
                    &buf[..msg_len],
                    sender,
                    &mut device,
                    &mut iface,
                    &mut sockets,
                    &mut table,
                    &local_ip,
                );
            }
            _ => {
                ostd::task::yield_now();
            }
        }
```

### Step 2 — Widen `handle_ipc` to accept a slice
At `main.rs:154`–`:155`, change the parameter type:

```rust
/// Dispatch one IPC message.
fn handle_ipc(
    buf: &[u8],
    sender: usize,
```
(everything else in `handle_ipc` is unchanged — `decode_message(buf)` already accepts `&[u8]`).

### Step 3 — Verify the empty-message guard still holds
`decode_message` (`poll_driver.rs:59`) already returns `NetMessage::Unknown` for an empty buffer. When `msg_len == 0` (an all-zero or empty message), `&buf[..0]` is empty → `Unknown` → no reply sent. Confirm this is intended: a spurious zero-length wake (`sender > 0` but no bytes) should be silently ignored. It is — matches prior `_ =>` no-op behavior.

### Step 4 — Build & lint
```bash
cargo build --release -p net    # or the net cell's package name
cargo clippy -p net -- -D warnings
```
No new `match` arms are introduced — the no-wildcard invariant on `tcp_state_byte` / opcodes is untouched.

## Todo List
- [ ] Step 1: add `buf.fill(0)` + `msg_len` scan in receive arm
- [ ] Step 2: change `handle_ipc(buf: &[u8; 512])` → `handle_ipc(buf: &[u8])`
- [ ] Step 3: confirm empty-message guard (`msg_len == 0` → `Unknown`)
- [ ] Step 4: `cargo build --release` + `cargo clippy -- -D warnings` clean
- [ ] Step 5: run `network_curl_http_get` integration test — must still pass

## Success Criteria
- Net cell builds clean, clippy clean (no new warnings).
- `network_curl_http_get` passes (proves SEND no longer corrupts the GET request).
- A SEND payload of N bytes results in exactly N bytes on the wire (verifiable via the HTTP server which checks for `\r\n\r\n` terminator — extra garbage after the headers would break Content-Length framing on a real server; the test server is lenient but the curl path exercises the same handler).

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Zero-scan truncates a binary payload ending in NUL | Low (no such caller today) | Med | Documented `// LIMITATION:`; defer length-prefix framing |
| `buf.fill(0)` per-iteration cost on the hot path | Low | Low | 512 bytes memset; negligible vs smoltcp poll. Single-core, no contention |
| RxFrame regression from passing a slice | Low | High | smoltcp uses IP total-length, ignores tail; trailing zeros safe. Guarded by existing DHCP/echo tests |
| `msg_len` mis-scan if a valid message has interior+trailing zeros | Low | Med | Interior zeros preserved (scan finds LAST non-zero); only a NUL-terminated payload is affected (see limitation) |

## Rollback Plan
Single-file, self-contained change. Revert `main.rs` to restore `buf: &[u8; 512]`, drop `buf.fill(0)` and the `msg_len` scan, and pass `&buf` again. No data migration, no ABI change (IPC wire format unchanged — this is a receiver-side parsing fix only). Reverting cannot cascade: `poll_driver.rs` was never touched.

## Backwards Compatibility
- **Wire format unchanged.** Senders still emit `[opcode:1][cap:8][payload:*]`. This is purely a receiver-side length-recovery fix.
- Existing senders (`nc.rs`, `curl`) require zero changes and continue to work.

## Security Considerations
- Fix *reduces* an information-leak/corruption surface: previously, bytes from a prior cell's message could be transmitted on a TCP socket belonging to a different request. Zero-fill eliminates this cross-message bleed.

## Next Steps
- Unblocks Phase 02 validation (Lua `vnet.send` uses this same handler).
