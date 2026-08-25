use super::*;

#[test]
fn valid_and_high_s_provider_outputs_return_the_same_verified_low_s_signature() {
    let mut outputs = [[0u8; 64]; 2];
    for (index, behavior) in [FixtureSignatureBehavior::Valid, FixtureSignatureBehavior::HighS]
        .into_iter()
        .enumerate()
    {
        let provider = FixtureRelayProvider {
            behavior,
            ..FixtureRelayProvider::production()
        };
        let mut service = KmsService::with_provider_fixture(provider);
        bind_net(&mut service, 7, 4);
        let response = service
            .handle(
                &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99),
                7,
                Some(caller(50, 4, 7)),
                net_registry(Some(7)),
            )
            .unwrap();
        let payload = Tls13ClientCertificateVerifyResponsePayload::decode(
            response.payload().unwrap(),
        )
        .unwrap();
        outputs[index] = payload.signature;
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_ne!(outputs[0], [0; 64]);
}

#[test]
fn malformed_provider_scalars_and_bad_signatures_are_rejected() {
    for behavior in [
        FixtureSignatureBehavior::InvalidScalar,
        FixtureSignatureBehavior::Corrupt,
    ] {
        let provider = FixtureRelayProvider {
            behavior,
            ..FixtureRelayProvider::production()
        };
        let mut service = KmsService::with_provider_fixture(provider);
        bind_net(&mut service, 7, 4);
        let response = service
            .handle(
                &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99),
                7,
                Some(caller(50, 4, 7)),
                net_registry(Some(7)),
            )
            .unwrap();
        assert_error(response, KmsErrorCode::InvalidSignature);
    }
}
