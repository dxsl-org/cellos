use super::*;

#[test]
fn pending_snapshot_and_tpm_bindings_fail_closed() {
    let mut fixture = chain(0);
    let pending = snapshot(&fixture);
    fixture.profile[0] ^= 1;
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::ProfileDigestMismatch)
    );

    let fixture = chain(0);
    let pending = snapshot(&fixture);
    let mut stale = policy(&fixture, &[], &[]);
    stale.expected_journal_revision = pending.journal_revision() + 1;
    assert_eq!(
        run(&fixture, pending, stale, None),
        Err(Error::StaleSnapshot)
    );

    let mut fixture = chain(0);
    let pending = snapshot(&fixture);
    fixture.tpm[10] ^= 1;
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::TpmPublicDigestMismatch)
    );

    let fixture = chain(0);
    let pending = snapshot(&fixture);
    let mut raced = fixture.tpm.clone();
    raced[10] ^= 1;
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), Some(raced)),
        Err(Error::PendingPublicRace)
    );

    let mut fixture = chain(0);
    fixture.tpm[1] = fixture.tpm[1].wrapping_add(1);
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::InvalidTpmPublic)
    );

    let mut fixture = chain(0);
    fixture.tpm = fixture::unrelated_tpm_public();
    let pending = snapshot(&fixture);
    assert_eq!(
        run(&fixture, pending, policy(&fixture, &[], &[]), None),
        Err(Error::SpkiMismatch)
    );
}

#[test]
fn tpm_public_algorithm_attribute_and_curve_matrix_is_closed() {
    let fixture = chain(0);
    for (index, mask) in [
        (3usize, 1u8),
        (5, 1),
        (9, 2),
        (13, 1),
        (15, 1),
        (17, 1),
        (19, 1),
        (21, 1),
        (23, 1),
    ] {
        let mut encoded = fixture.tpm.clone();
        encoded[index] ^= mask;
        assert_eq!(tpm::parse(&encoded), Err(Error::InvalidTpmPublic));
    }
    let mut trailing = fixture.tpm;
    trailing.push(0);
    assert_eq!(tpm::parse(&trailing), Err(Error::InvalidTpmPublic));
}
