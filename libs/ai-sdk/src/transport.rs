//! Transports for [`crate::AiClient`].
//!
//! A transport is the only target-specific piece of the client: it encodes one request into the
//! shared IPC buffer, delivers it to the inference service, and copies the encoded reply back.
//! Keeping it behind a trait is what lets the client's request-id handling, reply matching,
//! streaming, and bounds logic be host-tested without linking a bare-metal `ostd`
//! (`crate::testing::ScriptedTransport`) and lets a Cell use the real IPC path
//! (`crate::ostd_transport::OstdTransport`).

use ai_proto::{AiRequest, AiResponse};

use crate::AiClientError;

/// One request/reply round trip to the inference service.
pub trait AiTransport {
    /// Send `request`, receive its reply into `reply`, and decode it.
    ///
    /// `reply` is caller-owned so the decoded response can borrow message text without copying:
    /// the caller keeps the buffer alive for exactly as long as the returned value.
    ///
    /// A transport must answer with the reply to *this* request or fail — it never fabricates a
    /// success, and it never returns a message that arrived from anyone other than the service.
    fn round_trip<'r>(
        &mut self,
        request: &AiRequest<'_>,
        reply: &'r mut [u8],
    ) -> Result<AiResponse<'r>, AiClientError>;
}
