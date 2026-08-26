# Phase 01: Net Typed IPC Migration

**Status**: 📋 Planned  
**Priority**: P1  
**Effort**: ~2 days  
**Stage**: G1

---

## Overview

The net service (`/bin/net`) still uses raw byte opcodes for its consumer IPC path —
`poll_driver::cell_opcodes` defines hex constants (0x10–0x24) and `decode_message()` does
manual byte parsing.  All six net-tool consumers (curl, nc, httpd, ping, mqtt, wget) write
into those opcodes directly.

This phase migrates the consumer-to-net IPC path to the typed `api::ipc::NetRequest /
NetResponse` postcard protocol, matching what VFS already does.

**Kernel-to-net path stays raw** — the kernel sends raw Ethernet frames as IPC (opcode 0x00)
and those cannot be postcard-encoded; `kernel_opcodes` stays untouched.

---

## ⚠️ Law 1 Warning

This phase adds four variants to `NetRequest` in `libs/api/src/ipc.rs`:
`UdpBind`, `GetLocalIp`, `MulticastJoin`, `MulticastLeave`.

**Requires 2× user confirmation before implementation starts.**

---

## Key Insights

- `decode_message()` in `poll_driver.rs` deserializes: `[opcode:1][cap_id:8 LE][payload:*]`.
  After migration, consumers will call `api::ipc::encode(&NetRequest, &mut buf)` and the net
  service will call `api::ipc::decode::<NetRequest>(&buf)`.
- The `cap_id` field (currently 8 bytes manually extracted at bytes 1–8) maps to the
  `cap_id: u32` field in the relevant `NetRequest` variant. Note the size change: raw uses
  u64, typed uses u32 — use u32 throughout (no currently-issued cap exceeds u32 range).
- `NetResponse::CapId(u32)` already encodes the new socket handle.  Consumers that currently
  read the first 4 bytes of the reply buffer must instead call `api::ipc::decode::<NetResponse>`.

---

## Requirements

### Functional
1. `api::ipc::NetRequest` gains four new variants:
   - `UdpBind { cap_id: u32, port: u16 }` — bind a UDP socket to a local port
   - `GetLocalIp` — return the DHCP-assigned IPv4 address
   - `MulticastJoin { cap_id: u32, group: [u8; 4] }` — join an IPv4 multicast group
   - `MulticastLeave { cap_id: u32, group: [u8; 4] }` — leave an IPv4 multicast group
2. Net service decodes incoming consumer IPC with `api::ipc::decode::<NetRequest>(&buf)`.
3. Net service encodes replies with `api::ipc::encode(&NetResponse::..., &mut resp_buf)`.
4. All net-tool consumers encode requests with `api::ipc::encode`.
5. `poll_driver::cell_opcodes` module is removed (no callers remain).
6. `poll_driver::decode_message()` is removed or narrowed to kernel-only frames.

### Non-Functional
- `cargo check` clean on net service and all net-tool cells.
- No change to the kernel-to-net raw frame path (`kernel_opcodes` stays).

---

## Architecture

```
Consumer (curl, nc, …)
  └─ api::ipc::encode(&NetRequest::TcpConnect{…}, &mut buf)
  └─ sys_send(net_tid, &buf)
  └─ sys_recv(net_tid, &mut resp_buf)
  └─ api::ipc::decode::<NetResponse>(&resp_buf)

Net service recv loop
  └─ api::ipc::decode::<NetRequest>(&buf)  ← replaces decode_message() + opcode match
  └─ handle dispatch
  └─ api::ipc::encode(&resp, &mut resp_buf)
  └─ sys_send(sender, &resp_buf)
```

Kernel-to-net path unchanged:
```
Kernel VirtIO net ISR
  └─ sys_send(net_tid, raw_frame_bytes)   ← opcode 0x00 prefix stays
```

---

## Related Code Files

**Modify:**
- `libs/api/src/ipc.rs` — add 4 new `NetRequest` variants (**Law 1**)
- `cells/services/net/src/main.rs` — replace opcode dispatch with typed decode
- `cells/services/net/src/poll_driver.rs` — remove `cell_opcodes`; keep `kernel_opcodes` + raw frame path
- `cells/apps/net-tools/src/bin/curl.rs`
- `cells/apps/net-tools/src/bin/nc.rs`
- `cells/apps/net-tools/src/bin/httpd.rs`
- `cells/apps/net-tools/src/bin/ping.rs`
- `cells/apps/net-tools/src/bin/mqtt.rs`
- `cells/apps/net-tools/src/bin/wget.rs`

---

## Implementation Steps

1. **Law 1 confirmation** — present the four new `NetRequest` variants to the user; wait for
   2× explicit approval before any code changes.
2. Add `UdpBind`, `GetLocalIp`, `MulticastJoin`, `MulticastLeave` to `NetRequest` in
   `libs/api/src/ipc.rs`.
3. Rewrite net service `main.rs` recv loop:
   - Retain the `if buf[0] == kernel_opcodes::RX_FRAME` branch (raw frame → smoltcp).
   - For all other messages: `api::ipc::decode::<NetRequest>(&buf[..])` → typed dispatch.
   - Each handler ends with `api::ipc::encode(&resp, &mut resp_buf)` → `sys_send(sender, ...)`.
4. Update `poll_driver.rs` — remove `cell_opcodes` module entirely; keep `kernel_opcodes` and
   `NetMessage::RxFrame` path.
5. Migrate net consumers one file at a time (curl → nc → httpd → ping → mqtt → wget).
   Each consumer:
   a. Replace manual `buf[0] = opcode; buf[1..9] = cap_id.to_le_bytes()` construction with
      `api::ipc::encode(&NetRequest::…, &mut buf)`.
   b. Replace raw reply parsing with `api::ipc::decode::<NetResponse>(&resp_buf)`.
6. `cargo check` on net service + each net-tool binary.

---

## Success Criteria

- [ ] `poll_driver::cell_opcodes` module removed; no callers remain.
- [ ] `decode_message()` either removed or only handles `kernel_opcodes::RX_FRAME`.
- [ ] Net service recv loop uses `api::ipc::decode::<NetRequest>`.
- [ ] All six net-tool consumers use `api::ipc::encode` / `decode::<NetResponse>`.
- [ ] `cargo check` clean on all affected crates.

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| cap_id u64→u32 range issue | Low | All issued cap IDs fit in u32 (counter starts at 1, no wrap) |
| Missed consumer (7th file) | Low | `grep -r cell_opcodes` before declaring done |
| Kernel RX path accidentally broken | Medium | Keep `buf[0] == 0x00` branch first in match; add comment |
| postcard overhead per call | Negligible | 1–2 bytes varint overhead per message; well under 512B IPC budget |

---

## Security Considerations

Removing raw byte parsing eliminates a class of deserialization bugs (off-by-one on cap_id
extraction, unchecked opcode ranges).  Postcard rejects unknown discriminants and malformed
lengths with a `postcard::Error` that the service maps to `NetResponse::Err(0xFF)`.
