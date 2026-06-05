//! MQTT 3.1.1 QoS-0 client cell.
//!
//! Usage:
//!   mqtt publish  <host:port> <topic> <payload>
//!   mqtt subscribe <host:port> <topic>
//!
//! Communicates with the net service cell via IPC (same opcodes as nc.rs).
//! Only QoS 0 (fire-and-forget publish, at-most-once subscribe).
#![no_std]
#![no_main]
extern crate ostd;

use ostd::io::{print, println};
use ostd::syscall::{sys_recv, sys_send, sys_spawn_args, sys_yield, SyscallResult};

const NET_ENDPOINT: usize = 6;
const SOCKET_TCP:   u8 = 0x10;
const CONNECT:      u8 = 0x12;
const SEND_OP:      u8 = 0x13;
const RECV_OP:      u8 = 0x14;
const CLOSE:        u8 = 0x15;

#[no_mangle]
pub fn main() {
    let mut arg_buf = [0u8; 128];
    let arg_len = sys_spawn_args(&mut arg_buf);
    if arg_len == 0 {
        println("Usage: mqtt publish  host:port topic payload");
        println("       mqtt subscribe host:port topic");
        return;
    }
    let args_str = match core::str::from_utf8(&arg_buf[..arg_len]) {
        Ok(s) => s,
        Err(_) => { println("mqtt: bad args"); return; }
    };
    let mut parts = args_str.split_whitespace();
    let subcmd   = match parts.next() { Some(s) => s, None => { println("mqtt: missing subcommand"); return; } };
    let hostport = match parts.next() { Some(s) => s, None => { println("mqtt: missing host:port"); return; } };
    let topic    = match parts.next() { Some(s) => s, None => { println("mqtt: missing topic"); return; } };

    // Split host:port on the last ':'.  Default port = 1883.
    let (host, port) = match hostport.rfind(':') {
        Some(i) => (&hostport[..i], parse_u16(&hostport[i + 1..]).unwrap_or(1883)),
        None    => (hostport, 1883u16),
    };
    let addr = match resolve_host(host) {
        Some(a) => a,
        None => { println("mqtt: invalid host"); return; }
    };

    // SOCKET_TCP → cap_id
    sys_send(NET_ENDPOINT, &[SOCKET_TCP, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut cap_reply = [0u8; 8];
    let cap_id = match sys_recv(0, &mut cap_reply) {
        SyscallResult::Ok(_) => u64::from_le_bytes(cap_reply),
        _ => { println("mqtt: socket failed"); return; }
    };
    if cap_id == 0 { println("mqtt: no socket cap"); return; }

    // TCP CONNECT [0x12][cap:8][addr:4][port:2]
    let mut conn = [0u8; 15];
    conn[0] = CONNECT;
    conn[1..9].copy_from_slice(&cap_id.to_le_bytes());
    conn[9..13].copy_from_slice(&addr);
    conn[13..15].copy_from_slice(&port.to_le_bytes());
    sys_send(NET_ENDPOINT, &conn);
    let mut ack = [0u8; 1];
    match sys_recv(0, &mut ack) {
        SyscallResult::Ok(_) if ack[0] == 0x00 => {}
        _ => { println("mqtt: tcp connect failed"); close_socket(cap_id); return; }
    }
    println("connected");

    if !mqtt_handshake(cap_id) {
        println("mqtt: CONNACK rejected");
        close_socket(cap_id);
        return;
    }

    match subcmd {
        "publish" => {
            let payload = parts.next().unwrap_or("");
            do_publish(cap_id, topic, payload);
        }
        "subscribe" => { do_subscribe(cap_id, topic); }
        _ => { println("mqtt: unknown subcommand"); }
    }
    close_socket(cap_id);
}

/// Send MQTT CONNECT and verify CONNACK `[0x20 0x02 0x00 0x00]`.
///
/// Hardcoded for client-id "vios", clean-session, keepalive 60 s.
fn mqtt_handshake(cap: u64) -> bool {
    // CONNECT fixed packet (18 bytes, remaining_len = 16).
    tcp_send(cap, &[
        0x10, 0x10,                           // type CONNECT, remaining = 16
        0x00, 0x04, b'M', b'Q', b'T', b'T',  // proto name "MQTT"
        0x04,                                 // level 3.1.1
        0x02,                                 // flags: clean session
        0x00, 0x3C,                           // keepalive = 60 s
        0x00, 0x04, b'v', b'i', b'o', b's',  // client-id "vios"
    ]);
    let mut buf = [0u8; 256];
    let n = mqtt_recv(cap, &mut buf, 500);
    // CONNACK: type 0x20, return-code 0x00 (accepted).
    n >= 4 && buf[0] == 0x20 && buf[3] == 0x00
}

/// Build and send a PUBLISH packet (QoS 0), then print "published".
fn do_publish(cap: u64, topic: &str, payload: &str) {
    let tb = topic.as_bytes();
    let pb = payload.as_bytes();
    if tb.len() > 64 { println("mqtt: topic too long (max 64 bytes)"); return; }
    if pb.len() > 256 { println("mqtt: payload too long (max 256 bytes)"); return; }
    // remaining = 2(topic_len_field) + topic + payload  (no packet-id at QoS 0).
    let remaining = 2 + tb.len() + pb.len();
    let mut pkt = [0u8; 340]; // 1 + 4(varint) + 2 + 64(topic) + 256(payload) headroom
    let mut rl  = [0u8; 4];
    let rl_len  = encode_remaining_len(remaining, &mut rl);
    pkt[0] = 0x30;
    pkt[1..1 + rl_len].copy_from_slice(&rl[..rl_len]);
    let mut p = 1 + rl_len;
    pkt[p]     = (tb.len() >> 8) as u8;
    pkt[p + 1] = tb.len() as u8;
    p += 2;
    pkt[p..p + tb.len()].copy_from_slice(tb); p += tb.len();
    pkt[p..p + pb.len()].copy_from_slice(pb); p += pb.len();
    tcp_send(cap, &pkt[..p]);
    println("published");
}

/// Send SUBSCRIBE, verify SUBACK, then poll and print incoming PUBLISH payloads.
fn do_subscribe(cap: u64, topic: &str) {
    let tb = topic.as_bytes();
    // remaining = 2(pkt_id) + 2(topic_len_field) + topic + 1(QoS).
    let remaining = 5 + tb.len();
    let mut pkt = [0u8; 96];
    let mut rl  = [0u8; 4];
    let rl_len  = encode_remaining_len(remaining, &mut rl);
    pkt[0] = 0x82;
    pkt[1..1 + rl_len].copy_from_slice(&rl[..rl_len]);
    let mut p = 1 + rl_len;
    pkt[p] = 0x00; pkt[p + 1] = 0x01; p += 2; // packet-id = 1
    pkt[p] = (tb.len() >> 8) as u8; pkt[p + 1] = tb.len() as u8; p += 2;
    pkt[p..p + tb.len()].copy_from_slice(tb); p += tb.len();
    pkt[p] = 0x00; p += 1; // QoS 0
    tcp_send(cap, &pkt[..p]);

    // SUBACK: first byte must be 0x90.
    let mut buf = [0u8; 256];
    let n = mqtt_recv(cap, &mut buf, 500);
    if n == 0 || buf[0] != 0x90 { println("mqtt: SUBACK not received"); return; }
    println("subscribed");

    // Receive PUBLISH packets: one recv per outer iteration, yielding between.
    // 10 000 iterations × ~1 ms yield = up to 10 s patience for the injected PUBLISH.
    for _ in 0..10_000usize {
        let mut data = [0u8; 256];
        let n = mqtt_recv_once(cap, &mut data);
        if n < 4 || data[0] != 0x30 { sys_yield(); continue; }
        // PUBLISH layout (QoS 0): [0x30][remaining][topic_len:2BE][topic][payload]
        let topic_len   = (data[2] as usize) << 8 | data[3] as usize;
        let payload_start = 4 + topic_len;
        let payload_end   = (2 + data[1] as usize).min(n);
        if payload_end <= payload_start { continue; }
        if let Ok(s) = core::str::from_utf8(&data[payload_start..payload_end]) {
            print(s);
            if !s.ends_with('\n') { println(""); }
        }
    }
}

/// Send bytes over TCP using the net IPC SEND opcode; retries until all sent.
fn tcp_send(cap: u64, data: &[u8]) {
    let mut sent = 0usize;
    for _ in 0..500 {
        if sent >= data.len() { break; }
        let chunk = (data.len() - sent).min(256);
        let mut msg = [0u8; 9 + 256];
        msg[0] = SEND_OP;
        msg[1..9].copy_from_slice(&cap.to_le_bytes());
        msg[9..9 + chunk].copy_from_slice(&data[sent..sent + chunk]);
        sys_send(NET_ENDPOINT, &msg[..9 + chunk]);
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

/// Send ONE RECV_OP; return bytes copied (0 = nothing available yet).
///
/// Detects data by checking the first byte is non-zero (all MQTT type
/// bytes are non-zero: types 1-15 map to 0x10-0xF0 upper nibble).
fn mqtt_recv_once(cap: u64, buf: &mut [u8; 256]) -> usize {
    let mut msg = [0u8; 13];
    msg[0] = RECV_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    msg[9..13].copy_from_slice(&256u32.to_le_bytes());
    sys_send(NET_ENDPOINT, &msg);
    let mut data = [0u8; 256];
    match sys_recv(0, &mut data) {
        SyscallResult::Ok(_) if data[0] != 0 => {
            let remaining = data[1] as usize;
            let total = (2 + remaining).min(256);
            buf[..total].copy_from_slice(&data[..total]);
            total
        }
        _ => 0,
    }
}

/// Poll until an MQTT packet arrives; yield between each poll.
///
/// `max_polls` caps the wait time.  Callers should use a value commensurate
/// with the expected response latency (CONNACK/SUBACK: fast; PUBLISH: slower).
fn mqtt_recv(cap: u64, buf: &mut [u8; 256], max_polls: usize) -> usize {
    for _ in 0..max_polls {
        let n = mqtt_recv_once(cap, buf);
        if n > 0 { return n; }
        sys_yield();
    }
    0
}

fn close_socket(cap: u64) {
    let mut msg = [0u8; 9];
    msg[0] = CLOSE;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    sys_send(NET_ENDPOINT, &msg);
    let mut r = [0u8; 1];
    let _ = sys_recv(0, &mut r);
}

/// Encode MQTT variable-length remaining-length (up to 4 bytes, supports > 127).
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

fn resolve_host(s: &str) -> Option<[u8; 4]> {
    match s {
        "gateway" | "host" => Some([10, 0, 2, 2]),
        "dns"              => Some([10, 0, 2, 3]),
        "localhost"        => Some([127, 0, 0, 1]),
        _                  => parse_ipv4(s),
    }
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut it = s.splitn(5, '.');
    let a = parse_octet(it.next()?)?;
    let b = parse_octet(it.next()?)?;
    let c = parse_octet(it.next()?)?;
    let d = parse_octet(it.next()?)?;
    if it.next().is_some() { return None; }
    Some([a, b, c, d])
}

fn parse_octet(s: &str) -> Option<u8> {
    let mut n: u16 = 0;
    if s.is_empty() { return None; }
    for ch in s.bytes() {
        if !(b'0'..=b'9').contains(&ch) { return None; }
        n = n * 10 + (ch - b'0') as u16;
        if n > 255 { return None; }
    }
    Some(n as u8)
}

fn parse_u16(s: &str) -> Option<u16> {
    let mut n: u32 = 0;
    if s.is_empty() { return None; }
    for ch in s.bytes() {
        if !(b'0'..=b'9').contains(&ch) { return None; }
        n = n * 10 + (ch - b'0') as u32;
        if n > 65535 { return None; }
    }
    Some(n as u16)
}
