use super::*;

const SERVER: [u8; 4] = [10, 0, 2, 3];

#[test]
fn slirp_aliases_answer_without_the_wire() {
    assert_eq!(static_lookup("gateway", SERVER), Some([10, 0, 2, 2]));
    assert_eq!(static_lookup("host", SERVER), Some([10, 0, 2, 2]));
    assert_eq!(static_lookup("localhost", SERVER), Some([127, 0, 0, 1]));
}

#[test]
fn the_dns_alias_follows_the_server_in_force() {
    assert_eq!(static_lookup("dns", SLIRP_DNS_SERVER), Some([10, 0, 2, 3]));
    assert_eq!(
        static_lookup("dns", [192, 168, 1, 1]),
        Some([192, 168, 1, 1])
    );
}

#[test]
fn literals_pass_through_but_names_never_do() {
    assert_eq!(static_lookup("10.0.2.2", SERVER), Some([10, 0, 2, 2]));
    assert_eq!(static_lookup("0.0.0.0", SERVER), Some([0, 0, 0, 0]));
    assert_eq!(
        static_lookup("255.255.255.255", SERVER),
        Some([255, 255, 255, 255])
    );

    // Anything malformed must reach the resolver as a name, never be read as
    // an address: a silent mis-parse here would dial the wrong host.
    for name in [
        "example.com",
        "10.0.2",
        "10.0.2.4.5",
        "256.0.2.1",
        "10.0.2.256",
        "10.0.2.01",
        "10.0.2.",
        "10.0.2.2 ",
        "10.0.2.-1",
        "",
    ] {
        assert_eq!(
            static_lookup(name, SERVER),
            None,
            "parsed {name:?} as an address"
        );
    }
}

#[test]
fn resolver_install_allocates_query_slots_and_admits_wire_query() {
    let mut storage = [smoltcp::iface::SocketStorage::EMPTY; 4];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let resolver = Resolver::install(&mut sockets);
    let mut device = crate::interface::VirtioNetDevice::new();
    let config = smoltcp::iface::Config::new(smoltcp::wire::HardwareAddress::Ethernet(smoltcp::wire::EthernetAddress([0x02, 0, 0, 0, 0, 1])));
    let mut iface = Interface::new(config, &mut device, smoltcp::time::Instant::from_micros(0));
    let handle = resolver.start("tuoitre.vn", &mut iface, &mut sockets);
    assert!(handle.is_some(), "resolver must have free query slots to start query");
    resolver.cancel(handle.unwrap(), &mut sockets);
}
