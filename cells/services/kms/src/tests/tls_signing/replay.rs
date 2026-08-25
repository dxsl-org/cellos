use super::*;

fn submit(service: &mut KmsService, request_id: u64) -> types::kms::KmsResponseV1 {
    service
        .handle(
            &sign_request(
                FIXTURE_PROFILE_DIGEST,
                FIXTURE_RELAY_GENERATION,
                request_id,
            ),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(7)),
        )
        .unwrap()
}

fn assert_signed(response: types::kms::KmsResponseV1) {
    Tls13ClientCertificateVerifyResponsePayload::decode(response.payload().unwrap()).unwrap();
}

#[test]
fn duplicate_stale_and_zero_ids_are_denied_before_provider_access() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::Relaxed);
    let provider = FixtureRelayProvider {
        access_calls: Some(&CALLS),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    bind_net(&mut service, 7, 4);

    assert_signed(submit(&mut service, 10));
    assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    let unauthorized_duplicate = service
        .handle(
            &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 10),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(8)),
        )
        .unwrap();
    assert_error(unauthorized_duplicate, KmsErrorCode::ServiceBindingStale);
    assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    for request_id in [10, 9, 0] {
        let response = submit(&mut service, request_id);
        assert_error(response, KmsErrorCode::InvalidRequest);
        assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    }
}

#[test]
fn failed_self_verification_does_not_advance_the_accepted_id() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::Relaxed);
    let provider = FixtureRelayProvider {
        behavior: FixtureSignatureBehavior::Corrupt,
        access_calls: Some(&CALLS),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    bind_net(&mut service, 7, 4);

    let first = submit(&mut service, 10);
    assert_error(first, KmsErrorCode::InvalidSignature);
    assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    let retry = submit(&mut service, 10);
    assert_error(retry, KmsErrorCode::InvalidSignature);
    assert_eq!(CALLS.load(Ordering::Relaxed), 4);
}

#[test]
fn successful_service_net_reregistration_resets_the_id_floor() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::Relaxed);
    let provider = FixtureRelayProvider {
        access_calls: Some(&CALLS),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    bind_net(&mut service, 7, 4);

    assert_signed(submit(&mut service, 10));
    let stale = submit(&mut service, 1);
    assert_error(stale, KmsErrorCode::InvalidRequest);
    assert_eq!(CALLS.load(Ordering::Relaxed), 2);

    bind_net(&mut service, 7, 4);
    assert_signed(submit(&mut service, 1));
    assert_eq!(CALLS.load(Ordering::Relaxed), 4);
}
