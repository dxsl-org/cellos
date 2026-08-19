use super::*;

#[test]
fn parse_peer_config_bytes_populates_valid_peer_with_direct_address() {
    let (peers, len) = parse_peer_config_bytes(
        b"# comment\n\
          peer_0_node_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
          peer_0_relay_ip=1.2.3.4\n\
          peer_0_relay_port=8765\n\
          peer_0_direct=10.0.0.1:4521\n",
    );

    let peer = peers[0].as_ref().expect("peer");
    assert_eq!(len, 1);
    assert_eq!(peer.node_id, CellNetId::from_bytes([0xaa; 32]));
    assert_eq!(peer.relay_ip, [1, 2, 3, 4]);
    assert_eq!(peer.relay_port, 8765);
    assert_eq!(peer.addrs_len, 1);
    assert_eq!(peer.addrs[0], ([10, 0, 0, 1], 4521));
}

#[test]
fn parse_peer_config_bytes_ignores_empty_numeric_fields() {
    let (peers, len) = parse_peer_config_bytes(
        b"peer_0_node_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
          peer_0_relay_ip=1.2.3.4\n\
          peer_0_relay_port=\n",
    );

    assert_eq!(len, 0);
    assert!(peers[0].is_none());
}

#[test]
fn parse_peer_config_bytes_ignores_invalid_lines_and_partial_peers() {
    let (peers, len) = parse_peer_config_bytes(
        b"peer_x_node_id=bad\n\
          peer_0_node_id=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\
          peer_0_relay_ip=1.2.3\n\
          peer_0_relay_port=8765\n\
          peer_0_direct=bad\n\
          broken_line\n",
    );

    assert_eq!(len, 0);
    assert!(peers.iter().all(Option::is_none));
}

#[test]
fn parse_peer_config_bytes_ignores_out_of_range_peer_index() {
    let (peers, len) = parse_peer_config_bytes(
        b"peer_8_node_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
          peer_8_relay_ip=1.2.3.4\n\
          peer_8_relay_port=8765\n",
    );

    assert_eq!(len, 0);
    assert!(peers.iter().all(Option::is_none));
}
