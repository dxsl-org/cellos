use super::*;

#[test]
fn framing_algorithm_path_and_extension_fail_closed() {
    let mut fixture = chain(0);
    let pending = snapshot(&fixture);
    fixture.profile.clear();
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::ProfileSize)
    );

    let mut fixture = chain(0);
    fixture.profile.push(0);
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::MalformedDer)
    );

    let mut fixture = chain(0);
    let position = fixture
        .profile
        .windows(8)
        .position(|w| w == [0x2a, 0x86, 0x48, 0xce, 0x3d, 4, 3, 2])
        .unwrap();
    fixture.profile[position + 7] ^= 1;
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::UnsupportedSignatureAlgorithm)
    );

    let mut fixture = chain(0);
    let curve = fixture
        .profile
        .windows(8)
        .position(|w| w == [0x2a, 0x86, 0x48, 0xce, 0x3d, 3, 1, 7])
        .unwrap();
    fixture.profile[curve + 7] ^= 1;
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::UnsupportedPublicKey)
    );

    let mut fixture = chain(0);
    let last = fixture.profile.len() - 1;
    fixture.profile[last] ^= 1;
    let pending = snapshot(&fixture);
    assert!(matches!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::SignatureVerification | Error::InvalidSignatureEncoding)
    ));

    let mut fixture = chain(0);
    fixture.profile.extend_from_slice(&fixture.root);
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::ForbiddenCertificate)
    );

    let mut fixture = chain(0);
    let critical = fixture
        .profile
        .windows(3)
        .position(|w| w == [0x01, 0x01, 0xff])
        .unwrap();
    fixture.profile[critical + 2] = 1;
    let pending = snapshot(&fixture);
    assert!(matches!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::MalformedExtensions)
    ));

    let mut fixture = chain(0);
    let basic = fixture
        .profile
        .windows(3)
        .position(|w| w == [0x55, 0x1d, 0x13])
        .unwrap();
    fixture.profile[basic + 2] = 0x14;
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::UnknownCriticalExtension)
    );

    let mut fixture = chain(0);
    let constraint = fixture
        .root
        .windows(12)
        .position(|w| w == b"node.example")
        .unwrap();
    fixture.root[constraint] = b'x';
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::InvalidNameConstraints)
    );
}

#[test]
fn server_auth_constrained_intermediate_is_rejected() {
    let fixture = fixture::chain_with_server_auth_intermediate();
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::InvalidExtendedKeyUsage)
    );
}

#[test]
fn reencoded_root_signature_cannot_hide_root_spki() {
    let mut fixture = chain(0);
    let last = fixture.root.len() - 1;
    fixture.root[last] ^= 1;
    fixture.profile.extend_from_slice(&fixture.root);
    let pinned = chain(0).root;
    let pending = snapshot(&fixture);
    let mut trusted = policy(&fixture, &[], &[]);
    trusted.trust_anchor_der = &pinned;
    assert_eq!(
        run(&fixture, pending, trusted, None),
        Err(Error::ForbiddenCertificate)
    );
}
