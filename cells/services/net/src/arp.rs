//! Ethernet/IPv4 ARP frames (RFC 826) for RFC 3927 link-local claiming.
//!
//! smoltcp 0.11 has no probe API and silently drops ARP whose sender IP is
//! 0.0.0.0 (every probe), so the link-local engine reads ARP straight off the
//! device RX path and transmits frames built here through the same TX queue.

use alloc::string::String;

/// EtherType of ARP.
const ETHERTYPE_ARP: [u8; 2] = [0x08, 0x06];
/// Ethernet header (14) plus an Ethernet/IPv4 ARP body (28).
pub(crate) const ARP_FRAME_LEN: usize = 42;
/// Fixed ARP header for Ethernet hardware addresses carrying IPv4: htype 1,
/// ptype 0x0800, hlen 6, plen 4.
const ARP_ETH_IPV4: [u8; 6] = [0x00, 0x01, 0x08, 0x00, 6, 4];
const BROADCAST: [u8; 6] = [0xff; 6];

/// ARP operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArpOp {
    /// `who-has target tell sender` (opcode 1).
    Request,
    /// `sender is-at sender_mac` (opcode 2).
    Reply,
}

/// One decoded Ethernet/IPv4 ARP packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArpPacket {
    pub op: ArpOp,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

impl ArpPacket {
    /// RFC 3927 §2.2.1 probe: a request for `candidate` with sender IP 0, so
    /// no host updates its cache with an address we do not own yet.
    pub(crate) fn probe(mac: [u8; 6], candidate: [u8; 4]) -> Self {
        Self::request(mac, [0; 4], candidate)
    }

    /// RFC 3927 §2.4 announcement: a request whose sender and target IP are
    /// both the claimed address.
    pub(crate) fn announce(mac: [u8; 6], ip: [u8; 4]) -> Self {
        Self::request(mac, ip, ip)
    }

    /// `who-has target tell ip`.
    pub(crate) fn request(mac: [u8; 6], ip: [u8; 4], target: [u8; 4]) -> Self {
        Self {
            op: ArpOp::Request,
            sender_mac: mac,
            sender_ip: ip,
            target_mac: [0; 6],
            target_ip: target,
        }
    }

    /// Answer to another host's probe for our claimed address. The prober has
    /// no address yet, so the target IP stays 0 and the frame is broadcast.
    pub(crate) fn reply_to_probe(mac: [u8; 6], ip: [u8; 4], prober: [u8; 6]) -> Self {
        Self {
            op: ArpOp::Reply,
            sender_mac: mac,
            sender_ip: ip,
            target_mac: prober,
            target_ip: [0; 4],
        }
    }

    /// Whether this is a probe: a request from a host with no address yet.
    pub(crate) fn is_probe(&self) -> bool {
        self.op == ArpOp::Request && self.sender_ip == [0; 4]
    }

    /// Whether this is an announcement (gratuitous request for itself).
    pub(crate) fn is_announcement(&self) -> bool {
        self.op == ArpOp::Request && self.sender_ip != [0; 4] && self.sender_ip == self.target_ip
    }

    /// Decode an Ethernet frame; `None` for anything but a well-formed
    /// untagged Ethernet/IPv4 request or reply.
    pub(crate) fn parse_frame(frame: &[u8]) -> Option<Self> {
        if frame.len() < ARP_FRAME_LEN || frame[12..14] != ETHERTYPE_ARP {
            return None;
        }
        let body = &frame[14..ARP_FRAME_LEN];
        if body[..6] != ARP_ETH_IPV4 {
            return None;
        }
        let op = match [body[6], body[7]] {
            [0, 1] => ArpOp::Request,
            [0, 2] => ArpOp::Reply,
            _ => return None,
        };
        let mut packet = Self {
            op,
            sender_mac: [0; 6],
            sender_ip: [0; 4],
            target_mac: [0; 6],
            target_ip: [0; 4],
        };
        packet.sender_mac.copy_from_slice(&body[8..14]);
        packet.sender_ip.copy_from_slice(&body[14..18]);
        packet.target_mac.copy_from_slice(&body[18..24]);
        packet.target_ip.copy_from_slice(&body[24..28]);
        Some(packet)
    }

    /// Encode as a broadcast Ethernet frame from `sender_mac`. RFC 3927 §2.5:
    /// every ARP packet with a link-local sender address, replies included, is
    /// broadcast so the whole link sees a conflict.
    pub(crate) fn encode_frame(&self) -> [u8; ARP_FRAME_LEN] {
        let mut frame = [0u8; ARP_FRAME_LEN];
        frame[..6].copy_from_slice(&BROADCAST);
        frame[6..12].copy_from_slice(&self.sender_mac);
        frame[12..14].copy_from_slice(&ETHERTYPE_ARP);
        frame[14..20].copy_from_slice(&ARP_ETH_IPV4);
        frame[20..22].copy_from_slice(match self.op {
            ArpOp::Request => &[0, 1],
            ArpOp::Reply => &[0, 2],
        });
        frame[22..28].copy_from_slice(&self.sender_mac);
        frame[28..32].copy_from_slice(&self.sender_ip);
        frame[32..38].copy_from_slice(&self.target_mac);
        frame[38..42].copy_from_slice(&self.target_ip);
        frame
    }
}

/// `aa:bb:cc:dd:ee:ff` for log lines.
pub(crate) fn mac_text(mac: [u8; 6]) -> String {
    alloc::format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    )
}

#[cfg(test)]
#[path = "arp-tests.rs"]
mod tests;
