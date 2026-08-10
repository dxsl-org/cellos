extern crate alloc;

use super::*;
use alloc::format;
use alloc::string::String;

fn peer_hex(byte: u8) -> String {
    (0..32).map(|_| format!("{byte:02x}")).collect()
}

#[test]
fn load_config_bytes_ignores_invalid_lines() {
    let mut identity = BrokerIdentity::from_static_pub([7; 32]);
    identity.load_config_bytes(b"peer_x_node_id=bad\npeer_0_direct=bad\n");
    assert_eq!(identity.peer_count(), 0);
}

#[test]
fn load_config_bytes_populates_valid_peer() {
    let cfg = format!(
        "peer_0_node_id={}\npeer_0_relay_ip=1.2.3.4\npeer_0_relay_port=8765\npeer_0_direct=10.0.0.1:4521\n",
        peer_hex(0xab)
    );
    let mut identity = BrokerIdentity::from_static_pub([9; 32]);
    identity.load_config_bytes(cfg.as_bytes());

    let peer = identity.get_peer(0).expect("peer");
    assert_eq!(identity.peer_count(), 1);
    assert_eq!(peer.node_id, CellNetId::from_bytes([0xab; 32]));
    assert_eq!(peer.relay_ip, [1, 2, 3, 4]);
    assert_eq!(peer.relay_port, 8765);
    assert_eq!(peer.addrs_len, 1);
    assert_eq!(peer.addrs[0], ([10, 0, 0, 1], 4521));
}
