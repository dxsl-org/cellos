use super::{network_fallback_allowed, prompt_fits_wire};
use ai_proto::{limit, AiError};
use ai_sdk::AiClientError;

#[test]
fn wire_budget_boundary_is_the_ai_message_limit() {
    assert!(prompt_fits_wire(0));
    assert!(prompt_fits_wire(ai_proto::MAX_PROMPT_BYTES));
    assert!(!prompt_fits_wire(ai_proto::MAX_PROMPT_BYTES + 1));
}

#[test]
fn only_missing_local_inference_falls_back_to_the_network() {
    for error in [
        AiClientError::NoService,
        AiClientError::Service(AiError::NoModel),
    ] {
        assert!(network_fallback_allowed(&error), "{error:?}");
    }

    for error in [
        AiClientError::Service(AiError::Busy),
        AiClientError::Service(AiError::NotSupported),
        AiClientError::Service(AiError::Internal),
        AiClientError::Service(AiError::BadRequest(limit::Violation::PromptTooLong)),
        AiClientError::Transport,
        AiClientError::Protocol,
        AiClientError::PollLimitExceeded,
    ] {
        assert!(!network_fallback_allowed(&error), "{error:?}");
    }
}
