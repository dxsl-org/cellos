use super::*;

const VALID: &[u8] = b"relay_ip=127.0.0.1\nrelay_port=443\nrelay_hostname=relay.example\n";

#[test]
fn absent_or_complete_endpoint_is_deterministic() {
    assert_eq!(
        parse_relay_endpoint_bytes(b"# local only\npeer_count=0\n"),
        Ok(None)
    );

    let endpoint = parse_relay_endpoint_bytes(
        b" # relay\npeer_0_relay_ip=10.0.0.1\n relay_ip = 127.0.0.1 \nrelay_port=443\nrelay_hostname=relay.example\n",
    )
    .expect("valid config")
    .expect("configured endpoint");
    assert_eq!(endpoint.ip(), [127, 0, 0, 1]);
    assert_eq!(endpoint.port(), 443);
    assert_eq!(endpoint.hostname(), b"relay.example");
    assert!(endpoint.hostname[endpoint.hostname_len as usize..]
        .iter()
        .all(|byte| *byte == 0));
}

#[test]
fn partial_endpoints_report_each_missing_field() {
    assert_eq!(
        parse_relay_endpoint_bytes(b"relay_port=443\nrelay_hostname=relay.example\n"),
        Err(RelayConfigError::MissingIp)
    );
    assert_eq!(
        parse_relay_endpoint_bytes(b"relay_ip=127.0.0.1\nrelay_hostname=relay.example\n"),
        Err(RelayConfigError::MissingPort)
    );
    assert_eq!(
        parse_relay_endpoint_bytes(b"relay_ip=127.0.0.1\nrelay_port=443\n"),
        Err(RelayConfigError::MissingHostname)
    );
}

#[test]
fn duplicate_and_unknown_relay_keys_fail_closed() {
    for duplicate in [
        b"relay_ip=127.0.0.1\n".as_slice(),
        b"relay_port=443\n".as_slice(),
        b"relay_hostname=relay.example\n".as_slice(),
    ] {
        let mut config = VALID.to_vec();
        config.extend_from_slice(duplicate);
        assert_eq!(
            parse_relay_endpoint_bytes(&config),
            Err(RelayConfigError::DuplicateKey)
        );
    }
    assert_eq!(
        parse_relay_endpoint_bytes(b"relay_mode=tls\n"),
        Err(RelayConfigError::UnknownRelayKey)
    );
    assert_eq!(
        parse_relay_endpoint_bytes(b"relay_hostname\n"),
        Err(RelayConfigError::UnknownRelayKey)
    );
}

#[test]
fn invalid_ipv4_and_ports_fail_closed() {
    for ip in [
        b"".as_slice(),
        b"1.2.3",
        b"1.2.3.256",
        b"1..2.3",
        b"+1.2.3.4",
    ] {
        let mut config = b"relay_ip=".to_vec();
        config.extend_from_slice(ip);
        config.extend_from_slice(b"\nrelay_port=443\nrelay_hostname=relay.example\n");
        assert_eq!(
            parse_relay_endpoint_bytes(&config),
            Err(RelayConfigError::InvalidIp)
        );
    }
    for port in [b"".as_slice(), b"0", b"-1", b"tls", b"65536"] {
        let mut config = b"relay_ip=127.0.0.1\nrelay_port=".to_vec();
        config.extend_from_slice(port);
        config.extend_from_slice(b"\nrelay_hostname=relay.example\n");
        assert_eq!(
            parse_relay_endpoint_bytes(&config),
            Err(RelayConfigError::InvalidPort)
        );
    }
}

#[test]
fn invalid_hostnames_fail_closed() {
    for hostname in [
        b"".as_slice(),
        b"Relay.example",
        b"relay example",
        b".relay.example",
        b"relay..example",
        b"relay.example.",
        b"-relay.example",
        b"relay-.example",
        b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let mut config = b"relay_ip=127.0.0.1\nrelay_port=443\nrelay_hostname=".to_vec();
        config.extend_from_slice(hostname);
        config.push(b'\n');
        assert_eq!(
            parse_relay_endpoint_bytes(&config),
            Err(RelayConfigError::InvalidHostname)
        );
    }
}
