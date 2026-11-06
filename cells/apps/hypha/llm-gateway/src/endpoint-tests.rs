use super::{select_transport, Transport, PLAINTEXT_PORT, QEMU_SLIRP_HOST, TLS_PORT};

#[test]
fn tls_is_the_default_for_every_endpoint() {
    for ip in [
        QEMU_SLIRP_HOST,
        [192, 168, 1, 10],
        [10, 0, 2, 3],
        [127, 0, 0, 1],
    ] {
        assert_eq!(select_transport(ip, false), Transport::Tls, "{ip:?}");
    }
    assert_eq!(select_transport(QEMU_SLIRP_HOST, false).port(), TLS_PORT);
}

#[test]
fn plaintext_needs_the_opt_in_and_the_qemu_host() {
    assert_eq!(
        select_transport(QEMU_SLIRP_HOST, true),
        Transport::Plaintext
    );
    assert_eq!(
        select_transport(QEMU_SLIRP_HOST, true).port(),
        PLAINTEXT_PORT
    );
    for ip in [
        [10, 0, 2, 3],
        [10, 0, 3, 2],
        [8, 8, 8, 8],
        [192, 168, 1, 10],
    ] {
        assert_eq!(select_transport(ip, true), Transport::Tls, "{ip:?}");
    }
}
