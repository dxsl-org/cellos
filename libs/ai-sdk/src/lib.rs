//! `AiClient` — the Spec 24 §6 public programming surface for the Cellos AI inference service.
//!
//! Normative source: [`docs/specs/24-ai-inference-architecture.md`] §6. The wire records live in
//! `ai-proto`; this crate is what a Cell imports to talk to `/bin/ai`.
//!
//! # Shape
//!
//! [`AiTransport`] is the only target-specific piece: on a Cell it is a typed-IPC round trip
//! through `ostd` ([`ostd_transport::OstdTransport`], compiled only for bare-metal targets so host
//! tests link without `ostd`'s global allocator). Everything else — request ids, reply matching,
//! streaming, bounds — is target-independent and host-tested.
//!
//! # Deliberate deviations from the ratified signature
//!
//! * `prompt` keeps the async shape but the returned [`TokenStream`] is a synchronous iterator: the
//!   async reactor (Spec 20 §5) is not landed, and pretending otherwise would hide a blocking IPC
//!   round trip behind `await`. Streaming itself is real — the service holds sessions and the
//!   caller polls them.
//! * Errors are [`AiClientError`], not `ViError`, so a caller can distinguish "transport failed"
//!   from "the service refused with `Busy`". `From<AiClientError> for ViError` is provided for
//!   callers that want the collapsed form.
//!
//! # Capacity
//!
//! One call is one 4 KiB IPC message both ways: prompts up to [`ai_proto::MAX_PROMPT_BYTES`],
//! replies up to [`ai_proto::MAX_TOKENS_PER_POLL`] token ids / [`ai_proto::MAX_REPLY_TEXT_BYTES`]
//! text bytes. Larger context/messages belong on the VFS/grant path (Spec 24 §6).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use ai_proto::{AiError, AiRequest, AiResponse, Describe, FinishReason, Quant, MAX_SESSIONS};

#[cfg(all(feature = "ostd-transport", target_os = "none"))]
pub mod ostd_transport;
#[cfg(test)]
mod testing;
pub mod transport;

pub use transport::AiTransport;

/// Failure from the service boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiClientError {
    /// The transport itself failed (IPC send/receive, no registered service).
    Transport,
    /// The service is not registered yet; the caller may retry after the supervisor spawns it.
    NoService,
    /// The reply did not decode, or did not match the request that was sent.
    Protocol,
    /// The service answered with a typed refusal.
    Service(AiError),
    /// A synchronous drain exceeded the caller's poll budget.
    PollLimitExceeded,
}

/// Result alias for this crate's boundaries.
pub type AiResult<T> = Result<T, AiClientError>;

impl From<AiClientError> for crate::types::ViError {
    fn from(error: AiClientError) -> Self {
        match error {
            AiClientError::Transport | AiClientError::PollLimitExceeded => {
                crate::types::ViError::IO
            }
            AiClientError::NoService => crate::types::ViError::NotFound,
            AiClientError::Protocol => crate::types::ViError::InvalidInput,
            AiClientError::Service(error) => match error {
                AiError::BadRequest(_) => crate::types::ViError::InvalidArgument,
                AiError::Busy => crate::types::ViError::WouldBlock,
                AiError::NoModel => crate::types::ViError::NotFound,
                AiError::NotSupported => crate::types::ViError::NotSupported,
                AiError::UnknownRequest | AiError::NotRunning => {
                    crate::types::ViError::InvalidArgument
                }
                AiError::Internal => crate::types::ViError::IO,
            },
        }
    }
}

mod types {
    pub use api::ViError;
}

/// Parameters for one inference request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferParams {
    /// Prompt text.
    pub prompt: String,
    /// Hard cap on generated tokens.
    pub max_tokens: u16,
    /// Temperature × 1000; `0` selects greedy decoding.
    pub temperature_milli: u16,
    /// Top-k cutoff; `0` means no cutoff.
    pub top_k: u16,
    /// Deterministic sampling seed.
    pub seed: u32,
}

impl InferParams {
    /// Greedy parameters for `prompt`.
    pub fn greedy(prompt: &str, max_tokens: u16) -> Self {
        Self {
            prompt: String::from(prompt),
            max_tokens,
            temperature_milli: 0,
            top_k: 0,
            seed: 0,
        }
    }
}

/// One token with its decoded text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Token id in the model vocabulary.
    pub id: u32,
    /// Text produced by this token (may be empty for a partial UTF-8 sequence).
    pub text: String,
}

/// One `INFER_STREAM_POLL` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Token ids produced since the previous poll.
    pub ids: Vec<u32>,
    /// Complete UTF-8 text for `ids`.
    pub text: String,
    /// True when the session finished and every id has been drained.
    pub done: bool,
    /// Why the session finished; `Some` exactly when `done`.
    pub finish: Option<FinishReason>,
}

/// A drained generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// Every generated token id, in order.
    pub ids: Vec<u32>,
    /// The decoded text.
    pub text: String,
    /// Why generation ended.
    pub finish: FinishReason,
    /// Number of poll round trips it took.
    pub polls: usize,
}

/// Owned capability description (the wire record borrows; callers usually want to keep it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    /// Wire-contract version.
    pub proto_version: u16,
    /// Backend serving requests.
    pub active_backend: u32,
    /// Backends this build implements.
    pub backends: u32,
    /// Model name.
    pub model: String,
    /// Model architecture.
    pub arch: String,
    /// Weight quantization.
    pub quant: Quant,
    /// Context window in tokens.
    pub context_tokens: u32,
    /// Vocabulary size.
    pub vocab_size: u32,
    /// Sessions currently live.
    pub live_sessions: u8,
    /// Maximum concurrent sessions.
    pub max_sessions: u8,
    /// Resident bytes held by the service.
    pub resident_bytes: u32,
}

impl From<Describe<'_>> for ServiceInfo {
    fn from(describe: Describe<'_>) -> Self {
        Self {
            proto_version: describe.proto_version,
            active_backend: describe.active_backend,
            backends: describe.backends,
            model: String::from(describe.model),
            arch: String::from(describe.arch),
            quant: describe.quant,
            context_tokens: describe.context_tokens,
            vocab_size: describe.vocab_size,
            live_sessions: describe.live_sessions,
            max_sessions: describe.max_sessions,
            resident_bytes: describe.resident_bytes,
        }
    }
}

/// Client for one AI inference service provider.
pub struct AiClient<T: AiTransport> {
    transport: T,
    device: ai_proto::DeviceTarget,
}

impl<T: AiTransport> AiClient<T> {
    /// Connect through `transport`, letting the service choose the backend.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            device: ai_proto::DeviceTarget::Auto,
        }
    }

    /// Connect through `transport` with an explicit device preference.
    pub fn with_device(transport: T, device: ai_proto::DeviceTarget) -> Self {
        Self { transport, device }
    }

    /// Current device preference.
    pub fn device(&self) -> ai_proto::DeviceTarget {
        self.device
    }

    /// Capability probe.
    pub fn describe(&mut self) -> AiResult<ServiceInfo> {
        let request = AiRequest::Describe;
        let mut reply = [0u8; ai_proto::AI_IPC_BUF_SIZE];
        let response = self.exchange(&request, &mut reply)?;
        match response {
            AiResponse::Description(describe) => Ok(describe.into()),
            AiResponse::Failed { error, .. } => Err(AiClientError::Service(error)),
            _ => Err(AiClientError::Protocol),
        }
    }

    /// Start a session; returns the service-assigned id.
    pub fn submit(&mut self, params: &InferParams) -> AiResult<u32> {
        if params.prompt.len() > ai_proto::MAX_PROMPT_BYTES {
            return Err(AiClientError::Service(AiError::BadRequest(
                ai_proto::limit::Violation::PromptTooLong,
            )));
        }
        let request = AiRequest::InferSubmit(ai_proto::InferSubmit {
            prompt: params.prompt.as_str(),
            max_tokens: params.max_tokens,
            temperature_milli: params.temperature_milli,
            top_k: params.top_k,
            seed: params.seed,
            device: self.device,
        });
        let mut reply = [0u8; ai_proto::AI_IPC_BUF_SIZE];
        match self.exchange(&request, &mut reply)? {
            AiResponse::Accepted { request_id, .. } => Ok(request_id),
            AiResponse::Failed { error, .. } => Err(AiClientError::Service(error)),
            _ => Err(AiClientError::Protocol),
        }
    }

    /// Take up to `max_tokens` ready tokens from a session.
    pub fn poll(&mut self, request_id: u32, max_tokens: u8) -> AiResult<Chunk> {
        if max_tokens == 0 || max_tokens > ai_proto::MAX_TOKENS_PER_POLL {
            return Err(AiClientError::Service(AiError::BadRequest(
                ai_proto::limit::Violation::PollTooLarge,
            )));
        }
        let request = AiRequest::InferStreamPoll {
            request_id,
            max_tokens,
        };
        let mut reply = [0u8; ai_proto::AI_IPC_BUF_SIZE];
        match self.exchange(&request, &mut reply)? {
            AiResponse::TokenChunk {
                tokens,
                text,
                done,
                finish,
                ..
            } => Ok(Chunk {
                ids: ai_proto::token_ids(tokens).collect(),
                text: String::from(text),
                done,
                finish: if done { Some(finish) } else { None },
            }),
            AiResponse::Failed { error, .. } => Err(AiClientError::Service(error)),
            _ => Err(AiClientError::Protocol),
        }
    }

    /// Abandon a session.
    pub fn cancel(&mut self, request_id: u32) -> AiResult<()> {
        let request = AiRequest::InferCancel { request_id };
        let mut reply = [0u8; ai_proto::AI_IPC_BUF_SIZE];
        match self.exchange(&request, &mut reply)? {
            // The service answers a cancel with the final chunk state, an empty chunk, or a
            // refusal; treating "finished" as success keeps cancel idempotent for callers.
            AiResponse::TokenChunk { .. } => Ok(()),
            AiResponse::Failed {
                error: AiError::UnknownRequest,
                ..
            } => Ok(()),
            AiResponse::Failed { error, .. } => Err(AiClientError::Service(error)),
            _ => Err(AiClientError::Protocol),
        }
    }

    /// Embedding of `text`.
    pub fn embed(&mut self, text: &str) -> AiResult<Vec<f32>> {
        let request = AiRequest::InferEmbed {
            text,
            device: self.device,
        };
        let mut reply = [0u8; ai_proto::AI_IPC_BUF_SIZE];
        match self.exchange(&request, &mut reply)? {
            AiResponse::Embedding { dim, values } => {
                let collected: Vec<f32> = ai_proto::embedding_values(values).collect();
                if collected.len() != usize::from(dim) {
                    return Err(AiClientError::Protocol);
                }
                Ok(collected)
            }
            AiResponse::Failed { error, .. } => Err(AiClientError::Service(error)),
            _ => Err(AiClientError::Protocol),
        }
    }

    /// Submit and drain a generation bounded by `max_polls` round trips.
    pub fn generate(&mut self, params: &InferParams, max_polls: usize) -> AiResult<Generation> {
        let request_id = self.submit(params)?;
        let mut ids = Vec::new();
        let mut text = String::new();
        let mut polls = 0usize;

        while polls < max_polls {
            let chunk = match self.poll(request_id, ai_proto::MAX_TOKENS_PER_POLL) {
                Ok(chunk) => chunk,
                Err(error) => {
                    // Do not leave a half-consumed session behind on a late failure.
                    let _ = self.cancel(request_id);
                    return Err(error);
                }
            };
            polls += 1;
            ids.extend(chunk.ids.iter().copied());
            text.push_str(&chunk.text);
            if chunk.done {
                return Ok(Generation {
                    ids,
                    text,
                    finish: chunk.finish.unwrap_or(FinishReason::Length),
                    polls,
                });
            }
        }

        let _ = self.cancel(request_id);
        Err(AiClientError::PollLimitExceeded)
    }

    /// Spec 24 §6 `prompt`: submit and drive the session as a stream of tokens.
    ///
    /// The future resolves as soon as the service accepts the submission; the stream then performs
    /// one blocking poll per `next()`. It is `async` to keep the ratified call shape.
    pub async fn prompt(&mut self, params: &InferParams) -> AiResult<TokenStream<'_, T>> {
        let request_id = self.submit(params)?;
        Ok(TokenStream {
            client: self,
            request_id,
            finished: false,
            pending: Vec::new(),
            text: String::new(),
        })
    }

    /// Send `request` and decode its reply from `reply`.
    ///
    /// The reply buffer is caller-owned so a decoded [`AiResponse`] can borrow from it without
    /// copying: every public method keeps its buffer on the stack and consumes the response in the
    /// same scope.
    fn exchange<'r>(
        &mut self,
        request: &AiRequest<'_>,
        reply: &'r mut [u8],
    ) -> AiResult<AiResponse<'r>> {
        let response = self.transport.round_trip(request, reply)?;
        if !response.matches(request) {
            return Err(AiClientError::Protocol);
        }
        Ok(response)
    }
}

/// Token stream returned by [`AiClient::prompt`].
///
/// Yields each generated token; `None` when the session finishes. Dropping the stream without
/// cancelling leaves the session live on the service until the client's process exits or the
/// service reclaims it, matching the service's bounded session table (call [`TokenStream::cancel`]
/// to release early).
pub struct TokenStream<'a, T: AiTransport> {
    client: &'a mut AiClient<T>,
    request_id: u32,
    finished: bool,
    pending: Vec<u32>,
    text: String,
}

impl<T: AiTransport> TokenStream<'_, T> {
    /// Session id assigned by the service.
    pub fn request_id(&self) -> u32 {
        self.request_id
    }

    /// Text decoded so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Release the session early.
    pub fn cancel(&mut self) -> AiResult<()> {
        self.finished = true;
        self.client.cancel(self.request_id)
    }
}

impl<T: AiTransport> Iterator for TokenStream<'_, T> {
    type Item = AiResult<Token>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(id) = self.pending.pop() {
            return Some(Ok(Token {
                id,
                text: String::new(),
            }));
        }
        if self.finished {
            return None;
        }
        match self
            .client
            .poll(self.request_id, ai_proto::MAX_TOKENS_PER_POLL)
        {
            Ok(chunk) => {
                self.text.push_str(&chunk.text);
                if chunk.done {
                    self.finished = true;
                }
                let mut ids = chunk.ids;
                ids.reverse();
                self.pending = ids;
                // Text for the drained ids was already appended; yield ids first, then the text is
                // available through `text()`. Each token's own text needs its own decode, so the
                // iterator surfaces ids and the caller reads `text()` for the decoded run.
                if let Some(id) = self.pending.pop() {
                    return Some(Ok(Token {
                        id,
                        text: String::new(),
                    }));
                }
                if self.finished {
                    None
                } else {
                    self.next()
                }
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

/// True when the service reports at least one live session slot for this client.
///
/// A caller-side hint only: the service is the authority on admission.
pub fn has_free_session(info: &ServiceInfo) -> bool {
    info.live_sessions < info.max_sessions.min(MAX_SESSIONS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Reply, ScriptedTransport};
    use alloc::vec;

    fn params(prompt: &str) -> InferParams {
        InferParams::greedy(prompt, 4)
    }

    #[test]
    fn drives_a_full_generation_from_accepted_to_done() {
        let transport = ScriptedTransport::new(vec![
            (
                ai_proto::opcode::INFER_SUBMIT,
                Reply::Accepted { request_id: 3 },
            ),
            (
                ai_proto::opcode::INFER_STREAM_POLL,
                Reply::Chunk {
                    ids: vec![10, 11],
                    text: "ab",
                    done: false,
                },
            ),
            (
                ai_proto::opcode::INFER_STREAM_POLL,
                Reply::Chunk {
                    ids: vec![12],
                    text: "c",
                    done: true,
                },
            ),
        ]);
        let mut client = AiClient::new(transport);
        let generation = client.generate(&params("hello"), 8).expect("generation");
        assert_eq!(generation.ids, vec![10, 11, 12]);
        assert_eq!(generation.text, "abc");
        assert_eq!(generation.finish, FinishReason::Length);
        assert_eq!(generation.polls, 2);
    }

    #[test]
    fn surfaces_service_refusals_instead_of_inventing_success() {
        let transport = ScriptedTransport::new(vec![(
            ai_proto::opcode::INFER_SUBMIT,
            Reply::Failed(AiError::Busy),
        )]);
        let mut client = AiClient::new(transport);
        assert_eq!(
            client.submit(&params("hello")).unwrap_err(),
            AiClientError::Service(AiError::Busy)
        );
        // Busy collapses to WouldBlock for ViError callers.
        let vi: crate::types::ViError = AiClientError::Service(AiError::Busy).into();
        assert_eq!(vi, crate::types::ViError::WouldBlock);
    }

    #[test]
    fn describe_and_embed_round_trip_typed_payloads() {
        let transport = ScriptedTransport::new(vec![
            (ai_proto::opcode::DESCRIBE, Reply::Description),
            (
                ai_proto::opcode::INFER_EMBED,
                Reply::Embedding(vec![0.5, -0.25]),
            ),
        ]);
        let mut client = AiClient::new(transport);
        let info = client.describe().expect("describe");
        assert_eq!(info.model, "tiny-llama-64");
        assert!(has_free_session(&info));
        let values = client.embed("xyz").expect("embed");
        assert_eq!(values, vec![0.5, -0.25]);
    }

    #[test]
    fn rejects_a_reply_that_does_not_match_the_request() {
        // The scripted service answers a cancel with a description: protocol mismatch.
        let transport =
            ScriptedTransport::new(vec![(ai_proto::opcode::INFER_CANCEL, Reply::Description)]);
        let mut client = AiClient::new(transport);
        assert_eq!(client.cancel(1).unwrap_err(), AiClientError::Protocol);
    }

    #[test]
    fn cancel_is_idempotent_for_an_unknown_session() {
        let transport = ScriptedTransport::new(vec![(
            ai_proto::opcode::INFER_CANCEL,
            Reply::Failed(AiError::UnknownRequest),
        )]);
        let mut client = AiClient::new(transport);
        assert_eq!(client.cancel(42), Ok(()));
    }

    #[test]
    fn generate_cancels_on_a_late_failure_and_reports_the_poll_limit() {
        let transport = ScriptedTransport::new(vec![
            (
                ai_proto::opcode::INFER_SUBMIT,
                Reply::Accepted { request_id: 1 },
            ),
            (
                ai_proto::opcode::INFER_STREAM_POLL,
                Reply::Failed(AiError::Internal),
            ),
            (
                ai_proto::opcode::INFER_CANCEL,
                Reply::Failed(AiError::UnknownRequest),
            ),
        ]);
        let mut client = AiClient::new(transport);
        assert_eq!(
            client.generate(&params("hello"), 4).unwrap_err(),
            AiClientError::Service(AiError::Internal)
        );

        let transport = ScriptedTransport::new(vec![
            (
                ai_proto::opcode::INFER_SUBMIT,
                Reply::Accepted { request_id: 1 },
            ),
            (
                ai_proto::opcode::INFER_STREAM_POLL,
                Reply::Chunk {
                    ids: vec![],
                    text: "",
                    done: false,
                },
            ),
            (
                ai_proto::opcode::INFER_STREAM_POLL,
                Reply::Chunk {
                    ids: vec![],
                    text: "",
                    done: false,
                },
            ),
            (
                ai_proto::opcode::INFER_CANCEL,
                Reply::Failed(AiError::UnknownRequest),
            ),
        ]);
        let mut client = AiClient::new(transport);
        assert_eq!(
            client.generate(&params("hello"), 2).unwrap_err(),
            AiClientError::PollLimitExceeded
        );
    }

    #[test]
    fn bounds_are_checked_before_the_transport_is_touched() {
        let transport = ScriptedTransport::new(vec![]);
        let calls = transport.calls();
        let mut client = AiClient::new(transport);
        let long = InferParams::greedy(&"x".repeat(ai_proto::MAX_PROMPT_BYTES + 1), 4);
        assert!(matches!(
            client.submit(&long).unwrap_err(),
            AiClientError::Service(AiError::BadRequest(_))
        ));
        assert!(matches!(
            client.poll(1, 0).unwrap_err(),
            AiClientError::Service(AiError::BadRequest(_))
        ));
        assert_eq!(
            calls.borrow().len(),
            0,
            "no transport traffic for invalid calls"
        );
    }

    #[test]
    fn token_stream_yields_ids_then_text_and_can_be_cancelled() {
        let transport = ScriptedTransport::new(vec![
            (
                ai_proto::opcode::INFER_SUBMIT,
                Reply::Accepted { request_id: 5 },
            ),
            (
                ai_proto::opcode::INFER_STREAM_POLL,
                Reply::Chunk {
                    ids: vec![7, 8],
                    text: "hi",
                    done: false,
                },
            ),
            (
                ai_proto::opcode::INFER_CANCEL,
                Reply::Failed(AiError::UnknownRequest),
            ),
        ]);
        let mut client = AiClient::new(transport);
        let mut stream = block_on(client.prompt(&params("hello"))).expect("prompt");
        assert_eq!(stream.request_id(), 5);
        let first = stream.next().expect("first").expect("ok");
        let second = stream.next().expect("second").expect("ok");
        assert_eq!((first.id, second.id), (7, 8));
        assert_eq!(stream.text(), "hi");
        stream.cancel().expect("cancel");
        assert!(stream.next().is_none());
    }

    /// Minimal executor: every future here is immediately ready, so a single poll suffices.
    fn block_on<F: core::future::Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let waker = core::task::Waker::noop();
        let mut context = core::task::Context::from_waker(waker);
        match future.as_mut().poll(&mut context) {
            core::task::Poll::Ready(value) => value,
            core::task::Poll::Pending => panic!("AI client futures never park"),
        }
    }
}
