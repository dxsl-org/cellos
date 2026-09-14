//! Local-inference backend policy for the Hypha LLM gateway (Spec 24 consumer path).
//!
//! The gateway asks the on-device inference service Cell (`/bin/ai`, `service::AI`) before it
//! touches the network. Two rules decide whether that is possible, and both are policy rather
//! than plumbing, so they are pinned by host tests here instead of only by the QEMU gate:
//!
//! * **Wire budget.** One AI request is one IPC message, so a prompt above
//!   [`ai_proto::MAX_PROMPT_BYTES`] cannot be submitted locally at all. Such a turn is not a
//!   failure — it is a request for the larger-context backend, which is the network one.
//! * **Absence, not failure.** `NoService`/`NoModel` mean "no local inference here", so the turn
//!   may be retried over the network. A *failed* local attempt — `Busy`, a mid-session transport
//!   error, a poll limit — is reported instead: silently re-running the same prompt against a
//!   remote endpoint would hide contention and double the latency of a turn.

use ai_proto::AiError;
use ai_sdk::AiClientError;

/// True when `prompt_len` fits one AI IPC message, so the local backend can carry it.
pub fn prompt_fits_wire(prompt_len: usize) -> bool {
    prompt_len <= ai_proto::MAX_PROMPT_BYTES
}

/// True when `error` means "no local inference is available", so the network backend may answer.
pub fn network_fallback_allowed(error: &AiClientError) -> bool {
    matches!(
        error,
        AiClientError::NoService | AiClientError::Service(AiError::NoModel)
    )
}

#[cfg(test)]
#[path = "local-tests.rs"]
mod tests;
