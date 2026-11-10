use super::*;

const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xc2, 0xc0, 0x01];

#[test]
fn probe_encodes_to_the_rfc3927_wire_layout() {
    let frame = ArpPacket::probe(MAC, [169, 254, 192, 1]).encode_frame();
    #[rustfmt::skip]
    let expected: [u8; ARP_FRAME_LEN] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // broadcast
        0x52, 0x54, 0x00, 0xc2, 0xc0, 0x01, // source
        0x08, 0x06,                         // ARP
        0x00, 0x01, 0x08, 0x00, 6, 4,       // Ethernet / IPv4
        0x00, 0x01,                         // request
        0x52, 0x54, 0x00, 0xc2, 0xc0, 0x01, // sender MAC
        0, 0, 0, 0,                         // sender IP 0: a probe
        0, 0, 0, 0, 0, 0,                   // target MAC unknown
        169, 254, 192, 1,                   // candidate
    ];
    assert_eq!(frame, expected);
}

#[test]
fn every_kind_round_trips() {
    let ip = [169, 254, 192, 1];
    for packet in [
        ArpPacket::probe(MAC, ip),
        ArpPacket::announce(MAC, ip),
        ArpPacket::request(MAC, ip, [169, 254, 192, 2]),
        ArpPacket::reply_to_probe(MAC, ip, [2, 0, 0, 0, 0, 9]),
    ] {
        assert_eq!(ArpPacket::parse_frame(&packet.encode_frame()), Some(packet));
    }
    assert!(ArpPacket::probe(MAC, ip).is_probe());
    assert!(!ArpPacket::probe(MAC, ip).is_announcement());
    assert!(ArpPacket::announce(MAC, ip).is_announcement());
    assert!(!ArpPacket::request(MAC, ip, [169, 254, 192, 2]).is_announcement());
    assert!(!ArpPacket::reply_to_probe(MAC, ip, MAC).is_probe());
}

#[test]
fn padded_frames_parse() {
    // NICs pad to the 60-byte Ethernet minimum.
    let mut frame = [0u8; 60];
    let packet = ArpPacket::announce(MAC, [169, 254, 7, 7]);
    frame[..ARP_FRAME_LEN].copy_from_slice(&packet.encode_frame());
    assert_eq!(ArpPacket::parse_frame(&frame), Some(packet));
}

#[test]
fn non_arp_and_malformed_frames_are_rejected() {
    let good = ArpPacket::announce(MAC, [169, 254, 7, 7]).encode_frame();
    assert!(ArpPacket::parse_frame(&good[..ARP_FRAME_LEN - 1]).is_none());
    for (offset, value) in [
        (12, 0x86), // EtherType 0x8606
        (13, 0x00), // IPv4, not ARP
        (15, 0x06), // hardware type 6
        (16, 0x86), // protocol type IPv6
        (18, 8),    // hardware length
        (19, 16),   // protocol length
        (21, 3),    // RARP request
    ] {
        let mut frame = good;
        frame[offset] = value;
        assert!(ArpPacket::parse_frame(&frame).is_none(), "offset {offset}");
    }
}

#[test]
fn mac_text_is_colon_hex() {
    assert_eq!(mac_text(MAC), "52:54:00:c2:c0:01");
}
