# Phase 05 — MQTT Client Cell (X-5)

**Priority:** P3 | **Effort:** ~6h | **Status:** pending | **Files:** 3

## Context Links
- IPC reference impl: `cells/apps/net-tools/src/bin/nc.rs` (full opcode usage)
- Net endpoint + opcodes (nc.rs:13-23): `NET_ENDPOINT=6`, `SOCKET_TCP=0x10`,
  `CONNECT=0x12`, `SEND_OP=0x13`, `RECV_OP=0x14`, `CLOSE=0x15`, `STATE_OP=0x19`
- argv via `sys_spawn_args(&mut buf)` (nc.rs:36)
- `Cargo.toml` `[[bin]]` pattern (net-tools/Cargo.toml:11-29)
- Disk bundling: `gen_disk.ps1:53` (path var) + `:135` (table entry) — **brief
  missed this; both required for the binary to appear at `/bin/mqtt`**

## Overview
New `mqtt` cell implementing MQTT 3.1.1 publish & subscribe over TCP/1883 using
the existing net IPC. Reuses nc.rs's connect/send/recv scaffolding verbatim;
adds MQTT packet framing.

## Key Insights
- All transport already works via net IPC — no kernel/driver changes.
- MQTT QoS 0 only (KISS): no PUBACK/retransmit. CONNECT→CONNACK, then
  PUBLISH (fire-and-forget) or SUBSCRIBE→SUBACK→PUBLISH-recv loop.
- Reuse `resolve_host`/`parse_ipv4`/`parse_u16`/`close_socket` patterns from nc.rs
  (copy — these are per-binary helpers, not shared lib; DRY within reason).

## Architecture / Data Flow
**publish:** argv → connect TCP → send CONNECT(0x10) → recv CONNACK(0x20) →
send PUBLISH(0x30, topic+payload, QoS0) → close.
**subscribe:** connect → CONNECT → CONNACK → send SUBSCRIBE(0x82, pktid, topic,
QoS0) → recv SUBACK(0x90) → loop recv PUBLISH(0x30), print payload.

## MQTT Packet Framing (3.1.1, QoS 0)
- CONNECT: fixed `0x10` + remaining-len; var header = proto name `"MQTT"`
  (len-prefixed) + level `0x04` + flags `0x02` (clean session) + keepalive `60`;
  payload = client-id (len-prefixed, e.g. `"ViCell"`).
- CONNACK expected: `0x20 0x02 0x00 0x00`.
- PUBLISH: `0x30` + remaining-len + topic(len-prefixed) + payload (no packet id at QoS0).
- SUBSCRIBE: `0x82` + remaining-len + packet-id(2B) + topic(len-prefixed) + QoS(0x00).
- SUBACK expected: `0x90 ...`.
- Remaining-length: MQTT varint encoder (1 byte sufficient for <128-byte packets).

## Related Code Files
- Create: `cells/apps/net-tools/src/bin/mqtt.rs` (<200 lines per file rule)
- Modify: `cells/apps/net-tools/Cargo.toml` — add `[[bin]] name="mqtt" path="src/bin/mqtt.rs"`
- Modify: `gen_disk.ps1` — add `$mqtt_bin = "$rel_dir\mqtt"` near :53 and
  `if (Test-Path $mqtt_bin) { $table_args += "/bin/mqtt=$mqtt_bin" }` near :135
- Modify: `tests/integration/tests/boot.rs` — add `mqtt_publish` integration test

## Implementation Steps
1. Scaffold `mqtt.rs` from nc.rs: `#![no_std] #![no_main]`, `extern crate ostd`,
   import opcodes, copy `resolve_host`/`parse_ipv4`/`parse_u16`/`close_socket`.
2. Parse argv: `mqtt publish host:port topic message` /
   `mqtt subscribe host:port topic`. Split `host:port` on `:`.
3. Connect: SOCKET_TCP → cap, CONNECT [0x12]+cap+addr+port (port=1883 default).
4. `encode_remaining_len(n) -> ([u8;4], usize)` varint helper.
5. `mqtt_connect()`: build CONNECT, SEND (retry like nc.rs:112 loop), RECV, check
   CONNACK `0x20`.
6. `publish()`: build PUBLISH, SEND, close.
7. `subscribe()`: build SUBSCRIBE, SEND, RECV SUBACK, then poll RECV loop printing
   PUBLISH payloads (bounded iterations like nc.rs:209).
8. Add `[[bin]]`, edit gen_disk.ps1, rebuild, regenerate disk.
9. Integration test: spawn a host MQTT broker OR a mock TCP echo that returns a
   canned CONNACK; assert `mqtt publish ...` prints `published`.

## Todo List
- [ ] mqtt.rs scaffold + argv parse (host:port split)
- [ ] varint remaining-length encoder
- [ ] CONNECT + CONNACK check
- [ ] PUBLISH path
- [ ] SUBSCRIBE + SUBACK + recv loop
- [ ] Cargo.toml [[bin]]
- [ ] gen_disk.ps1 path var + table entry
- [ ] boot.rs integration test + broker/mock harness

## Success Criteria
- `/bin/mqtt` present after `gen_disk.ps1` (observable in cell table).
- `mqtt publish <broker>:1883 test/topic hi` → prints `connected` then
  `published`; broker receives the message (verified via test harness/mock).
- `mqtt subscribe <broker>:1883 test/topic` → prints `subscribed` then payloads.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| No host broker in CI | Med×High | Add a minimal mock TCP server in harness returning canned CONNACK/SUBACK; or gate test behind a `MQTT_BROKER` env like other net tests |
| Remaining-length >127 (multi-byte varint) | Low×Med | Implement full varint encoder up front; cap payload at 256 B for QoS0 demo |
| SEND partial-write duplication | Med×Med | Reuse nc.rs `sent_bytes` suffix-retry loop (nc.rs:111-129) verbatim |
| File exceeds 200-line rule | Med×Low | Keep helpers terse; split framing into a sibling `mqtt_proto.rs` mod if >200 |
| smoltcp state not READY on first send | Med×Med | Poll/yield retry like nc.rs; check STATE_OP if needed |

## Rollback
Delete `mqtt.rs`, remove `[[bin]]`, revert gen_disk.ps1 lines and the test.
Adds a new binary only — no existing path changes, zero migration risk.

## Security Considerations
No auth in QoS0 demo (matches nc/curl trust model). Bound recv buffers (256 B
like nc.rs) to prevent overflow. Topic/payload from argv — already trusted.

## Next Steps
Independent of 01-04. Shares `boot.rs` with phase 01's test edit — coordinate
that file if both run in parallel (different test fns, low conflict risk).
