use super::*;

#[test]
fn san_node_time_and_denylists_fail_closed() {
    let mut fixture = chain(0);
    let dns = fixture
        .profile
        .windows(12)
        .position(|w| w == b"node.example")
        .unwrap();
    fixture.profile[dns] = b'x';
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::InvalidSan)
    );

    let mut fixture = chain(0);
    let node = fixture
        .profile
        .windows(32)
        .position(|w| w == fixture.node)
        .unwrap();
    fixture.profile[node] ^= 1;
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::InvalidNodeId)
    );

    let fixture = chain(0);
    let pending = snapshot(&fixture);
    let mut expired = policy(&fixture, &[], &[]);
    expired.signed_time_unix = 4_102_444_800;
    assert_eq!(
        run(&fixture, pending.clone(), expired, None),
        Err(Error::CertificateExpired)
    );

    let denied = [fixture.node];
    assert_eq!(
        run(
            &fixture,
            pending.clone(),
            policy(&fixture, &denied, &[]),
            None,
        ),
        Err(Error::Denied)
    );
    let serial_bytes = [1u8];
    let serial = [DeniedSerial::new(&serial_bytes).unwrap()];
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &serial), None),
        Err(Error::Denied)
    );
}

#[test]
fn count_byte_and_tpm_size_bounds_fail_closed() {
    let mut fixture = chain(0);
    let original = fixture.profile.clone();
    fixture.profile.extend_from_slice(&original);
    fixture.profile.extend_from_slice(&original);
    fixture.profile.extend_from_slice(&original);
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::ProfileSize)
    );
    let mut fixture = chain(0);
    let pending = snapshot(&fixture);
    fixture.profile = vec![0; MAX_PROFILE_LEN + 1];
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::ProfileSize)
    );
    let fixture = chain(0);
    let pending = snapshot(&fixture);
    assert_eq!(
        run(
            &fixture,
            pending,
            policy(&fixture, &[], &[]),
            Some(vec![0; MAX_TPM2B_PUBLIC + 1])
        ),
        Err(Error::PendingPublicRead)
    );
}
