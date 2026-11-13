//! MQTT 3.1.1 QoS-0 telemetry over TCP-IPC.
//!
//! All functions are best-effort: they return without panicking when the net
//! service is absent or the broker is unreachable.

extern crate alloc;

use abi::syscall::service;
use ostd::io::println;
use ostd::ipc::IpcError;
use ostd::service::provider_tid;
use wire::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};

// 200ms per net-service request (system_ticks() runs at 100 Hz = 10ms per tick).
// Each call gets a fresh 20-tick budget covering admission and the reply.
// In test environments without a broker this bounds each MQTT cycle to ~2.4s.
const NET_CALL_TICKS: u64 = 20;

const MQTT_ADDR: [u8; 4] = [10, 0, 2, 2]; // QEMU SLIRP gateway
const MQTT_PORT: u16 = 1883;

/// Publish `payload` to the `vios/robot` topic.
///
/// Opens a fresh TCP connection per call (fire-and-forget QoS-0). Gracefully
/// skips if the net service is not registered or the broker is unreachable.
pub fn publish_telemetry(payload: &str) {
    let net_ep = match provider_tid(service::NET) {
        Some(ep) => ep,
        None => {
            println("[robot-demo] no net service — skipping MQTT");
            return;
        }
    };

    let mut send = [0u8; IPC_BUF_SIZE];
    let mut r = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::TcpConnect {
        addr: MQTT_ADDR,
        port: MQTT_PORT,
    };
    let cap: u32 = match net_call(net_ep, &req, &mut send, &mut r) {
        Ok(NetResponse::CapId(c)) => c,
        Err(IpcError::PreDispatchTimeout | IpcError::Indeterminate) => {
            println("[robot-demo] MQTT connect: net service timeout");
            return;
        }
        Ok(_) | Err(IpcError::Decode) => {
            println("[robot-demo] MQTT broker unreachable");
            return;
        }
        Err(_) => {
            println("[robot-demo] MQTT connect failed");
            return;
        }
    };

    if !mqtt_handshake(net_ep, cap) {
        println("[robot-demo] CONNACK failed — closing");
        close_cap(net_ep, cap);
        return;
    }

    mqtt_publish(net_ep, cap, "vios/robot", payload);
    close_cap(net_ep, cap);
    println("[robot-demo] MQTT telemetry published");
}

/// Send MQTT CONNECT, retry up to 5 times until the TCP handshake completes,
/// then accumulate data until CONNACK arrives. Returns true on `CONNACK RC=0`.
fn mqtt_handshake(net_ep: usize, cap: u32) -> bool {
    const PKTLEN: usize = 18;
    let connect_pkt: [u8; PKTLEN] = [
        0x10, 0x10, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x02, 0x00, 0x3C, 0x00, 0x04, b'v',
        b'i', b'o', b's',
    ];
    let mut connect_sent = false;
    let mut connack = [0u8; 64];
    let mut n_accum = 0usize;

    for _ in 0..5usize {
        if !connect_sent {
            connect_sent = tcp_try_send(net_ep, cap, &connect_pkt) > 0;
        }
        if connect_sent {
            let mut tmp = [0u8; 64];
            let n = tcp_recv_once(net_ep, cap, &mut tmp);
            if n > 0 && n_accum + n <= connack.len() {
                connack[n_accum..n_accum + n].copy_from_slice(&tmp[..n]);
                n_accum += n;
            }
            if n_accum >= 4 {
                return connack[0] == 0x20 && connack[3] == 0x00;
            }
        }
        ostd::task::yield_now();
    }
    false
}

fn mqtt_publish(net_ep: usize, cap: u32, topic: &str, payload: &str) {
    let mut pkt = [0u8; 340];
    let packet_len = match encode_publish_packet(topic, payload, &mut pkt) {
        Ok(len) => len,
        Err(_) => {
            println("[robot-demo] mqtt_publish: packet exceeds 340 bytes");
            return;
        }
    };
    tcp_send_raw(net_ep, cap, &pkt[..packet_len]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublishPacketError {
    Topic,
    Packet,
    RemainingLength,
}

fn encode_publish_packet(
    topic: &str,
    payload: &str,
    out: &mut [u8],
) -> Result<usize, PublishPacketError> {
    let topic = topic.as_bytes();
    let payload = payload.as_bytes();
    if topic.len() > u16::MAX as usize {
        return Err(PublishPacketError::Topic);
    }

    let remaining = 2usize
        .checked_add(topic.len())
        .and_then(|len| len.checked_add(payload.len()))
        .ok_or(PublishPacketError::RemainingLength)?;
    let mut encoded_remaining = [0u8; 4];
    let encoded_remaining_len = encode_remaining_len(remaining, &mut encoded_remaining)
        .ok_or(PublishPacketError::RemainingLength)?;
    let packet_len = 1usize
        .checked_add(encoded_remaining_len)
        .and_then(|len| len.checked_add(remaining))
        .ok_or(PublishPacketError::Packet)?;
    if packet_len > out.len() {
        return Err(PublishPacketError::Packet);
    }

    out[0] = 0x30;
    out[1..1 + encoded_remaining_len].copy_from_slice(&encoded_remaining[..encoded_remaining_len]);
    let mut cursor = 1 + encoded_remaining_len;
    out[cursor..cursor + 2].copy_from_slice(&(topic.len() as u16).to_be_bytes());
    cursor += 2;
    out[cursor..cursor + topic.len()].copy_from_slice(topic);
    cursor += topic.len();
    out[cursor..cursor + payload.len()].copy_from_slice(payload);
    Ok(packet_len)
}

/// One net-service request; the service answers each with exactly one `NetResponse`.
fn net_call<'r>(
    net_ep: usize,
    req: &NetRequest<'_>,
    send: &mut [u8; IPC_BUF_SIZE],
    reply: &'r mut [u8; IPC_BUF_SIZE],
) -> Result<NetResponse<'r>, IpcError> {
    ostd::ipc::service_call_typed_bounded(net_ep, req, send, reply, NET_CALL_TICKS)
}

/// Send data and return bytes accepted (0 = socket not yet Established or timeout).
fn tcp_try_send(net_ep: usize, cap: u32, data: &[u8]) -> usize {
    let mut send = [0u8; IPC_BUF_SIZE];
    let mut r = [0u8; IPC_BUF_SIZE];
    match net_call(
        net_ep,
        &NetRequest::TcpSend { cap_id: cap, data },
        &mut send,
        &mut r,
    ) {
        Ok(NetResponse::Data(b)) if b.len() >= 4 => {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&b[0..4]);
            u32::from_le_bytes(arr) as usize
        }
        _ => 0,
    }
}

/// Fire-and-forget send; used after Established is confirmed.
fn tcp_send_raw(net_ep: usize, cap: u32, data: &[u8]) {
    let mut send = [0u8; IPC_BUF_SIZE];
    let mut r = [0u8; IPC_BUF_SIZE];
    let _ = net_call(
        net_ep,
        &NetRequest::TcpSend { cap_id: cap, data },
        &mut send,
        &mut r,
    );
}

fn tcp_recv_once(net_ep: usize, cap: u32, out: &mut [u8; 64]) -> usize {
    let mut send = [0u8; IPC_BUF_SIZE];
    let mut r = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::TcpRecv {
        cap_id: cap,
        buf_len: 64,
    };
    match net_call(net_ep, &req, &mut send, &mut r) {
        Ok(NetResponse::Data(b)) if !b.is_empty() => {
            let n = b.len().min(64);
            out[..n].copy_from_slice(&b[..n]);
            n
        }
        _ => 0,
    }
}

fn close_cap(net_ep: usize, cap: u32) {
    let mut send = [0u8; IPC_BUF_SIZE];
    let mut r = [0u8; IPC_BUF_SIZE];
    let _ = net_call(
        net_ep,
        &NetRequest::TcpClose { cap_id: cap },
        &mut send,
        &mut r,
    );
}

fn encode_remaining_len(mut value: usize, out: &mut [u8; 4]) -> Option<usize> {
    const MAX_REMAINING_LEN: usize = 268_435_455;

    if value > MAX_REMAINING_LEN {
        return None;
    }

    let mut encoded_len = 0;
    loop {
        let mut byte = (value % 128) as u8;
        value /= 128;
        if value > 0 {
            byte |= 0x80;
        }
        out[encoded_len] = byte;
        encoded_len += 1;
        if value == 0 {
            return Some(encoded_len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_publish_packet, encode_remaining_len, PublishPacketError};

    #[test]
    fn remaining_length_encodes_mqtt_boundaries() {
        let cases: &[(usize, &[u8])] = &[
            (0, &[0x00]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (16_383, &[0xff, 0x7f]),
            (16_384, &[0x80, 0x80, 0x01]),
            (2_097_151, &[0xff, 0xff, 0x7f]),
            (2_097_152, &[0x80, 0x80, 0x80, 0x01]),
            (268_435_455, &[0xff, 0xff, 0xff, 0x7f]),
        ];

        for &(value, expected) in cases {
            let mut encoded = [0u8; 4];
            let encoded_len = encode_remaining_len(value, &mut encoded).unwrap();
            assert_eq!(&encoded[..encoded_len], expected);
        }
    }

    #[test]
    fn remaining_length_rejects_values_above_mqtt_limit() {
        let mut encoded = [0u8; 4];
        assert_eq!(encode_remaining_len(268_435_456, &mut encoded), None);
    }

    #[test]
    fn publish_packet_encodes_two_byte_remaining_length() {
        let payload = "x".repeat(125);
        let mut packet = [0u8; 132];
        let packet_len = encode_publish_packet("t", &payload, &mut packet).unwrap();

        assert_eq!(packet_len, 131);
        assert_eq!(&packet[..5], &[0x30, 0x80, 0x01, 0x00, 0x01]);
        assert_eq!(packet[5], b't');
        assert_eq!(&packet[6..packet_len], payload.as_bytes());
    }

    #[test]
    fn publish_packet_rejects_topic_length_above_u16() {
        let topic = "t".repeat(u16::MAX as usize + 1);
        let mut packet = [0u8; 340];

        assert_eq!(
            encode_publish_packet(&topic, "", &mut packet),
            Err(PublishPacketError::Topic)
        );
    }

    #[test]
    fn publish_packet_rejects_payload_that_exceeds_packet_buffer() {
        let payload = "x".repeat(335);
        let mut packet = [0u8; 340];

        assert_eq!(
            encode_publish_packet("t", &payload, &mut packet),
            Err(PublishPacketError::Packet)
        );
    }
}
