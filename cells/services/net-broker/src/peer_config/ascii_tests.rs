use super::ascii::{
    eq_slice, parse_addr, parse_hex32, parse_ipv4, parse_u16_ascii, starts_with, trim_ascii,
};

#[test]
fn ascii_helpers_accept_valid_boundaries() {
    assert_eq!(trim_ascii(b"  peer \n"), b"peer");
    assert!(starts_with(b"peer_0_node_id", b"peer_"));
    assert!(eq_slice(b"relay_ip", b"relay_ip"));
    assert_eq!(parse_ipv4(b"255.0.1.2"), Some([255, 0, 1, 2]));
    assert_eq!(parse_u16_ascii(b"65535"), Some(65535));
    assert_eq!(parse_addr(b"10.0.0.1:8080"), Some(([10, 0, 0, 1], 8080)));
    assert_eq!(
        parse_hex32(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        Some([0xaa; 32])
    );
}

#[test]
fn ascii_helpers_reject_invalid_inputs() {
    assert_eq!(trim_ascii(b" \t\r\n"), b"");
    assert!(!starts_with(b"peer", b"peer_"));
    assert!(!eq_slice(b"relay_ip", b"relay_port"));
    assert_eq!(parse_ipv4(b"1.2.3.256"), None);
    assert_eq!(parse_ipv4(b"1.2.x.4"), None);
    assert_eq!(parse_ipv4(b"1..2.3"), None);
    assert_eq!(parse_u16_ascii(b""), None);
    assert_eq!(parse_u16_ascii(b"65536"), None);
    assert_eq!(parse_addr(b"10.0.0.1"), None);
    assert_eq!(parse_hex32(b"abc"), None);
}
