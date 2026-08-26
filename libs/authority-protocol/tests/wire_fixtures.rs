mod wire_fixture_values;
mod wire_request_vectors;
mod wire_response_vectors;
use authority_protocol::*;
use wire_request_vectors::REQUEST_FRAMES;
use wire_response_vectors::RESPONSE_FRAMES;

use wire_fixture_values::{requests, responses};

struct ResponsePolicy;
impl ResponseAuthenticator for ResponsePolicy {
    fn verify(&self, _: &[u8; RESPONSE_AUTH_INPUT_LEN], signature: &[u8; 64]) -> bool {
        signature == &[8; 64]
    }
}

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap() as u8;
            let low = (pair[1] as char).to_digit(16).unwrap() as u8;
            (high << 4) | low
        })
        .collect()
}

#[test]
fn all_request_frames_match_literal_goldens() {
    for (request, literal) in requests().iter().zip(REQUEST_FRAMES) {
        let expected = hex(literal);
        let header = FrameHeader {
            class: FrameClass::Request,
            operation: request.operation(),
            payload_len: (expected.len() - FRAME_HEADER_LEN) as u16,
            request_id: 6,
            authenticator: [7; 16],
        };
        let mut actual = [0u8; FRAME_HEADER_LEN + FRAME_MAX_PAYLOAD];
        let length = encode_typed_request(header, request, &mut actual)
            .unwrap_or_else(|error| panic!("{:?}: {:?}", request.operation(), error));
        assert_eq!(&actual[..length], expected);
        assert_eq!(decode_typed_request(&expected), Ok((header, *request)));
    }
}

#[test]
fn all_response_frames_match_literal_goldens() {
    for (response, literal) in responses().iter().zip(RESPONSE_FRAMES) {
        let expected = hex(literal);
        let header = FrameHeader {
            class: FrameClass::Response,
            operation: response.operation(),
            payload_len: (expected.len() - FRAME_HEADER_LEN) as u16,
            request_id: 6,
            authenticator: [8; 16],
        };
        let mut actual = [0u8; FRAME_HEADER_LEN + FRAME_MAX_PAYLOAD];
        let length = encode_typed_response(header, response, &mut actual).unwrap();
        assert_eq!(&actual[..length], expected);
        assert_eq!(decode_typed_response(&expected), Ok((header, *response)));
        let expected_binding = ExpectedResponseBinding {
            device_id: [1; 32],
            authority_id: [2; 32],
            boot_epoch: 3,
            request_id: 6,
            operation: response.operation(),
        };
        let validated =
            verify_typed_response(*response, &header, &expected_binding, &ResponsePolicy).unwrap();
        assert_eq!(validated.response(), response);
    }
}

#[test]
fn response_identity_substitution_cannot_create_a_validated_token() {
    let response = responses()[0];
    let header = FrameHeader {
        class: FrameClass::Response,
        operation: response.operation(),
        payload_len: 225,
        request_id: 6,
        authenticator: [8; 16],
    };
    let expected = ExpectedResponseBinding {
        device_id: [9; 32],
        authority_id: [2; 32],
        boot_epoch: 3,
        request_id: 6,
        operation: response.operation(),
    };
    assert_eq!(
        verify_typed_response(response, &header, &expected, &ResponsePolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );

    let wrong_operation = ExpectedResponseBinding {
        device_id: [1; 32],
        operation: Operation::ReadCommittedRelayState,
        ..expected
    };
    assert_eq!(
        verify_typed_response(response, &header, &wrong_operation, &ResponsePolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );
}

#[test]
fn typed_fault_values_are_stable() {
    assert_eq!(
        decode_fault(&[15, 0]),
        Ok(AuthorityFault::ProviderSplitBrain)
    );
    assert_eq!(decode_fault(&[0xff, 0xff]), Err(WireError::UnknownFault));
}

#[test]
fn all_fault_frames_match_literal_goldens() {
    const FRAMES: [&str; 17] = [
        "4155544801010301020000000600000000000000080808080808080808080808080808080100",
        "4155544801010301020000000600000000000000080808080808080808080808080808080200",
        "4155544801010301020000000600000000000000080808080808080808080808080808080300",
        "4155544801010301020000000600000000000000080808080808080808080808080808080400",
        "4155544801010301020000000600000000000000080808080808080808080808080808080500",
        "4155544801010301020000000600000000000000080808080808080808080808080808080600",
        "4155544801010301020000000600000000000000080808080808080808080808080808080700",
        "4155544801010301020000000600000000000000080808080808080808080808080808080800",
        "4155544801010301020000000600000000000000080808080808080808080808080808080900",
        "4155544801010301020000000600000000000000080808080808080808080808080808080a00",
        "4155544801010301020000000600000000000000080808080808080808080808080808080b00",
        "4155544801010301020000000600000000000000080808080808080808080808080808080c00",
        "4155544801010301020000000600000000000000080808080808080808080808080808080d00",
        "4155544801010301020000000600000000000000080808080808080808080808080808080e00",
        "4155544801010301020000000600000000000000080808080808080808080808080808080f00",
        "4155544801010301020000000600000000000000080808080808080808080808080808081000",
        "4155544801010301020000000600000000000000080808080808080808080808080808081100",
    ];
    for (index, literal) in FRAMES.iter().enumerate() {
        let expected = hex(literal);
        let fault = AuthorityFault::try_from(index as u16 + 1).unwrap();
        let header = FrameHeader {
            class: FrameClass::Fault,
            operation: Operation::OpenBoot,
            payload_len: 2,
            request_id: 6,
            authenticator: [8; 16],
        };
        let mut actual = [0u8; FRAME_HEADER_LEN + 2];
        assert_eq!(
            encode_fault_frame(header, fault, &mut actual),
            Ok(actual.len())
        );
        assert_eq!(actual.as_slice(), expected);
        assert_eq!(decode_fault_frame(&expected), Ok((header, fault)));
    }
}
