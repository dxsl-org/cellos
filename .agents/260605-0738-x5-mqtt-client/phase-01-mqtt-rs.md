---
title: "X-5 Phase 01 — mqtt.rs implementation"
status: ready
priority: P3
effort: 2.5h
---

# Phase 01 — `cells/apps/net-tools/src/bin/mqtt.rs`

## Context Links

- IPC reference: `cells/apps/net-tools/src/bin/nc.rs` (copy close_socket, resolve_host, parse_*)
- Net opcodes: `NET_ENDPOINT=6`, `SOCKET_TCP=0x10`, `CONNECT=0x12`, `SEND_OP=0x13`,
  `RECV_OP=0x14`, `CLOSE=0x15` (from nc.rs:13-23)
- Argv: `sys_spawn_args(&mut buf)` (nc.rs:36 pattern)
- `cmd_echo_to_vec` not needed — self-contained binary

## Argv Contract

```
mqtt publish <host:port> <topic> <payload>
mqtt subscribe <host:port> <topic>
```

`host:port` split on last `:` — handles `10.0.2.2:1883` and `localhost:1883`.
Default port 1883 if no `:` in first arg.

## MQTT 3.1.1 Packet Bytes (QoS 0 only)

### CONNECT (18 bytes, remaining_len=16)
```
10 10          fixed header (type=CONNECT, remaining=16)
00 04          proto name length
4D 51 54 54  "MQTT"
04             protocol level 3.1.1
02             flags: clean session
00 3C          keepalive 60s
00 04          client-id length
76 69 6F 73  "ViCell"
```

### CONNACK expected (4 bytes)
```
20 02 00 00
```

### PUBLISH — topic + payload (QoS 0, no packet-id)
```
30 <remaining_len>
<topic_len:2BE> <topic_bytes>
<payload_bytes>       // no packet-id at QoS 0
```

### SUBSCRIBE — one topic, QoS 0
```
82 <remaining_len>
00 01              packet-id = 1
<topic_len:2BE> <topic_bytes>
00                 requested QoS = 0
```

### SUBACK expected: first byte = 0x90

### Incoming PUBLISH (from broker) layout
```
30 <remaining_len>
<topic_len:2BE> <topic_bytes>
<payload_bytes>
```
Parse by: skip fixed header (2B), read topic_len, skip topic_bytes, remaining bytes = payload.

## Remaining-Length Encoder (MQTT varint)

```rust
/// Encode MQTT variable-length remaining-length field (up to 4 bytes).
/// Sufficient for packets up to 268 MB; QoS-0 payloads are capped at 256 B.
fn encode_remaining_len(mut n: usize, out: &mut [u8; 4]) -> usize {
    let mut i = 0;
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 { b |= 0x80; }
        out[i] = b;
        i += 1;
        if n == 0 || i == 4 { break; }
    }
    i
}
```

## SEND retry loop (copy from nc.rs pattern)

```rust
fn tcp_send(cap: u64, data: &[u8]) {
    let mut sent = 0;
    for _ in 0..500 {
        if sent >= data.len() { break; }
        let rem = &data[sent..];
        let mut msg = [0u8; 9 + 256];
        msg[0] = SEND_OP;
        msg[1..9].copy_from_slice(&cap.to_le_bytes());
        let chunk = rem.len().min(256);
        msg[9..9+chunk].copy_from_slice(&rem[..chunk]);
        sys_send(NET_ENDPOINT, &msg[..9+chunk]);
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
}
```

## RECV one packet (poll until non-empty)

```rust
fn tcp_recv(cap: u64, buf: &mut [u8; 256]) -> usize {
    let mut msg = [0u8; 13];
    msg[0] = RECV_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    msg[9..13].copy_from_slice(&256u32.to_le_bytes());
    for _ in 0..500 {
        sys_send(NET_ENDPOINT, &msg);
        let mut data = [0u8; 256];
        match sys_recv(0, &mut data) {
            SyscallResult::Ok(_) if data[0] != 0 => {
                let n = data.iter().position(|&b| b == 0).unwrap_or(256);
                buf[..n].copy_from_slice(&data[..n]);
                return n;
            }
            _ => { sys_yield(); }
        }
    }
    0
}
```

## Full mqtt.rs Structure (185 lines target)

```
#![no_std] #![no_main]          // 4
extern crate ostd; use ...      // 6

constants: NET_ENDPOINT, opcodes // 8

fn main() {                     // 50
  parse argv (host:port split, subcommand)
  SOCKET_TCP → cap_id
  CONNECT → addr+port
  mqtt_handshake(cap)
  match subcommand:
    "publish"   → do_publish(cap, topic, payload)
    "subscribe" → do_subscribe(cap, topic)
  close_socket(cap)
}

fn mqtt_handshake(cap) -> bool  // 20
  build CONNECT packet (fixed bytes)
  tcp_send
  recv CONNACK, check [0x20,_,0x00,0x00]

fn do_publish(cap, topic, payload) // 20
  build PUBLISH packet
  tcp_send
  println("published")

fn do_subscribe(cap, topic)     // 25
  build SUBSCRIBE packet
  tcp_send
  recv SUBACK (first byte 0x90)
  println("subscribed")
  loop 200 iters: tcp_recv, parse PUBLISH payload, print

fn tcp_send(cap, data)          // 18
fn tcp_recv(cap, buf) -> usize  // 18
fn close_socket(cap)            // 6
fn encode_remaining_len(n, out) -> usize // 12

fn resolve_host(s) -> Option<[u8;4]> // 7
fn parse_ipv4(s) -> Option<[u8;4]>   // 10
fn parse_octet(s) -> Option<u8>      // 8
fn parse_u16(s) -> Option<u16>       // 8

TOTAL: ~175 lines
```

## Implementation Steps

1. Create `mqtt.rs` with the skeleton above.
2. Argv parsing: split `host:port` on last `:`, default port = 1883.
3. Implement `mqtt_handshake`: fixed CONNECT packet (hardcoded for "ViCell" client-id).
4. Implement `do_publish`: build PUBLISH with length-prefixed topic + raw payload.
5. Implement `do_subscribe`: SUBSCRIBE → SUBACK check → recv loop printing payloads.
6. Copy helpers from nc.rs: `close_socket`, `resolve_host`, `parse_ipv4`, `parse_octet`, `parse_u16`.
7. Build and verify compile.

## Todo

- [ ] Create `mqtt.rs` (175 lines)
- [ ] `mqtt_handshake()` — CONNECT + CONNACK check
- [ ] `do_publish()` — PUBLISH fire-and-forget
- [ ] `do_subscribe()` — SUBSCRIBE + SUBACK + recv loop
- [ ] `tcp_send()` helper
- [ ] `tcp_recv()` helper
- [ ] Helper functions (close, resolve, parse)
- [ ] Compile check

## Success Criteria

- `mqtt publish 10.0.2.2:PORT test/topic hello` → prints `connected\npublished`
- `mqtt subscribe 10.0.2.2:PORT test/topic` → prints `connected\nsubscribed\n<payload>`
- File stays ≤ 200 lines (split helpers to `mqtt_net.rs` mod if needed)
