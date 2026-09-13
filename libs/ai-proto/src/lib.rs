//! Cellos unified AI inference service — typed wire contract.
//!
//! Normative source: [`docs/specs/24-ai-inference-architecture.md`] §6 ("Law 1 Public Interface
//! Contract"). This crate is the *wire* half of that contract: the records a caller Cell and the
//! inference service Cell exchange over typed IPC. The programming surface (`AiClient`) lives in
//! `libs/ai-sdk`.
//!
//! # Wire framing
//!
//! Messages are postcard-encoded into the shared 4 KiB IPC buffer, exactly like the VFS, net,
//! input, and config services (Spec 17 §3): **byte 0 is the postcard variant discriminant** of
//! [`AiRequest`] / [`AiResponse`]. Spec 24 §6 numbers the logical opcodes `0x0600`–`0x0604`; those
//! constants are the registry names of the variants ([`opcode`]), and [`opcode::of_request`] is the
//! single place the mapping is defined. `opcode_map_matches_variant_order` pins it.
//!
//! Because the discriminant is self-delimiting, a request and a response for different opcodes are
//! distinguishable without a length prefix. The service still validates every field
//! ([`validate_request`]) before acting on it: the buffer is caller-controlled.
//!
//! # Capacity
//!
//! One IPC message is [`AI_IPC_BUF_SIZE`] bytes. Prompts, generated token ids, and inline text all
//! share that budget, so a submission is capped at [`MAX_PROMPT_BYTES`] and a poll reply is capped
//! at [`MAX_TOKENS_PER_POLL`] token ids / [`MAX_REPLY_TEXT_BYTES`] text bytes. Callers that need
//! larger context stream it in through the VFS/grant path (Spec 24 §6), not by growing the message.
//!
//! # Non-claims
//!
//! Device targets other than CPU are *declared vocabulary*: the service reports them through
//! [`Describe::backends`] and answers unsupported ones with [`AiError::NotSupported`]. Listing an
//! accelerator here is not evidence that one is implemented or qualified.

#![cfg_attr(not(test), no_std)]

use serde::{Deserialize, Serialize};

/// Size of one AI IPC message.
///
/// Must equal `api::ipc::IPC_BUF_SIZE`; `libs/ai-sdk` asserts that equality so a future change to
/// the system-wide buffer cannot silently desynchronise this contract.
pub const AI_IPC_BUF_SIZE: usize = 4096;

/// Maximum prompt bytes accepted in a single [`AiRequest::InferSubmit`].
///
/// Leaves room for the encoded envelope plus the largest reply the service may send.
pub const MAX_PROMPT_BYTES: usize = 2048;

/// Maximum bytes of generated text carried by one [`AiResponse::TokenChunk`].
pub const MAX_REPLY_TEXT_BYTES: usize = 2048;

/// Bytes per token id on the wire ([`AiResponse::TokenChunk::tokens`]).
pub const TOKEN_ID_BYTES: usize = 4;

/// Bytes per embedding value on the wire ([`AiResponse::Embedding::values`]).
pub const EMBED_VALUE_BYTES: usize = 4;

/// Maximum token ids carried by one [`AiResponse::TokenChunk`].
pub const MAX_TOKENS_PER_POLL: u8 = 16;

/// Maximum generation length a single request may ask for.
pub const MAX_TOKENS_PER_REQUEST: u16 = 1024;

/// Maximum embedding dimensions carried by one [`AiResponse::Embedding`].
///
/// 768 f32 = 3072 bytes, which still fits the IPC buffer next to the envelope.
pub const MAX_EMBED_DIM: u16 = 768;

/// Maximum concurrent generation sessions one inference service instance admits.
pub const MAX_SESSIONS: u8 = 4;

/// Logical opcodes from Spec 24 §6.
///
/// The wire does not carry these values; they are the registry names for the enum variants below
/// (see the crate docs). They are `u16` because the ratified spec names them as 16-bit opcodes.
pub mod opcode {
    /// `DESCRIBE` — service/model capability probe.
    pub const DESCRIBE: u16 = 0x0600;
    /// `INFER_SUBMIT` — submit a prompt, receive a session id.
    pub const INFER_SUBMIT: u16 = 0x0601;
    /// `INFER_STREAM_POLL` — drain generated tokens for a session.
    pub const INFER_STREAM_POLL: u16 = 0x0602;
    /// `INFER_CANCEL` — cancel a session and release it.
    pub const INFER_CANCEL: u16 = 0x0603;
    /// `INFER_EMBED` — embedding request.
    pub const INFER_EMBED: u16 = 0x0604;

    use super::AiRequest;

    /// Registry opcode of a request, by variant order.
    pub const fn of_request(request: &AiRequest<'_>) -> u16 {
        match request {
            AiRequest::Describe => DESCRIBE,
            AiRequest::InferSubmit(_) => INFER_SUBMIT,
            AiRequest::InferStreamPoll { .. } => INFER_STREAM_POLL,
            AiRequest::InferCancel { .. } => INFER_CANCEL,
            AiRequest::InferEmbed { .. } => INFER_EMBED,
        }
    }
}

/// Bit flags for [`Describe::backends`]: backends the *running* service can actually serve.
pub mod backend {
    /// Native CPU engine (always present on a service that has a model loaded).
    pub const CPU: u32 = 1 << 0;
    /// GPU compute backend. Declared only; no implementation ships in Phase 01–03.
    pub const GPU: u32 = 1 << 1;
    /// NPU backend. Declared only; hardware-gated (Spec 24 §3.1).
    pub const NPU: u32 = 1 << 2;
}

/// Target device for a request (Spec 24 §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceTarget {
    /// Probe order: NPU → GPU → CPU. The service falls back to the best *present* backend.
    Auto,
    /// Multi-core CPU engine with SIMD where the target provides it.
    Cpu,
    /// GPU compute backend.
    Gpu(GpuKind),
    /// Vendor NPU backend.
    Npu(NpuKind),
}

/// GPU backends named by Spec 24 §3.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuKind {
    /// Vulkan compute shaders.
    Vulkan,
    /// VirtIO-GPU backed compute.
    VirtioGpu,
}

/// NPU backends named by Spec 24 §3.1 / §7 (CP-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpuKind {
    /// Rockchip RK3588 (6 TOPS).
    Rk3588,
    /// SiFive X390 vector accelerator.
    X390,
    /// Hailo accelerator.
    Hailo,
}

/// Weight quantization of the resident model, for [`Describe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quant {
    /// Unquantized 32-bit floats.
    F32,
    /// 16-bit floats.
    F16,
    /// 8-bit block quantization (GGML `Q8_0`).
    Q8_0,
    /// Model-reported quantization this build does not have a name for.
    Other,
}

/// Why a generation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    /// The model emitted an end-of-generation token.
    Stop,
    /// `max_tokens` was reached.
    Length,
    /// The caller cancelled the session.
    Cancelled,
    /// Generation exceeded the model context window.
    ContextFull,
}

/// Typed failure vocabulary. Every non-success reply carries one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiError {
    /// A field was outside the contract (see [`limit`] codes).
    BadRequest(limit::Violation),
    /// All [`MAX_SESSIONS`] sessions are live; retry after draining one.
    Busy,
    /// The service has no model resident (no model file, or load failed).
    NoModel,
    /// The requested device/feature is not implemented in this build.
    NotSupported,
    /// The request named a session id the service does not own.
    UnknownRequest,
    /// The session was already cancelled or finished.
    NotRunning,
    /// The engine failed internally (model corrupt, arithmetic overflow guard hit).
    Internal,
}

/// Precise reasons a request was rejected, so a caller can fix the call instead of guessing.
pub mod limit {
    use serde::{Deserialize, Serialize};

    /// Which contract bound a rejected request violated.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Violation {
        /// Prompt bytes exceed [`super::MAX_PROMPT_BYTES`].
        PromptTooLong,
        /// `max_tokens` exceeds [`super::MAX_TOKENS_PER_REQUEST`] or is zero.
        MaxTokensOutOfRange,
        /// `top_k` is zero while a temperature was requested, or exceeds the vocabulary.
        TopKOutOfRange,
        /// `temperature_milli` is above the supported ceiling.
        TemperatureOutOfRange,
        /// A poll asked for more tokens than [`super::MAX_TOKENS_PER_POLL`].
        PollTooLarge,
        /// Embedding input is empty or larger than [`super::MAX_PROMPT_BYTES`].
        EmbedInputOutOfRange,
        /// The IPC buffer could not hold the encoded message.
        MessageTooLarge,
    }
}

/// Generation parameters for one submission.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InferSubmit<'a> {
    /// Prompt text, already tokenized by the service (context window is the service's to enforce).
    pub prompt: &'a str,
    /// Hard cap on generated tokens for this request.
    pub max_tokens: u16,
    /// Sampling temperature × 1000. `0` selects greedy decoding.
    pub temperature_milli: u16,
    /// Top-k cutoff applied before sampling. `0` means "no cutoff" (full vocabulary).
    pub top_k: u16,
    /// Deterministic sampling seed. Ignored for greedy decoding.
    pub seed: u32,
    /// Requested device; the reply reports the backend that actually served it.
    pub device: DeviceTarget,
}

/// Highest temperature the engine accepts (× 1000): 4.0.
pub const MAX_TEMPERATURE_MILLI: u16 = 4000;

/// Requests a caller Cell sends to the AI inference service.
#[derive(Debug, Serialize, Deserialize)]
pub enum AiRequest<'a> {
    /// `DESCRIBE` — capability probe; always safe to call, never allocates a session.
    Describe,
    /// `INFER_SUBMIT` — start a generation session; the reply carries its id.
    InferSubmit(InferSubmit<'a>),
    /// `INFER_STREAM_POLL` — take up to `max_tokens` ready tokens from a session.
    InferStreamPoll {
        /// Session id from [`AiResponse::Accepted`].
        request_id: u32,
        /// Tokens the caller wants back now, `1..=MAX_TOKENS_PER_POLL`.
        max_tokens: u8,
    },
    /// `INFER_CANCEL` — abandon a session and release its slot.
    ///
    /// The service answers with the session's final [`AiResponse::TokenChunk`] (possibly empty,
    /// `done: true`, finish [`FinishReason::Cancelled`]) so a caller that was mid-stream sees the
    /// same terminal shape it would have seen from a poll.
    InferCancel {
        /// Session id from [`AiResponse::Accepted`].
        request_id: u32,
    },
    /// `INFER_EMBED` — mean-pooled, L2-normalized hidden state of `text`.
    InferEmbed {
        /// Text to embed.
        text: &'a str,
        /// Requested device; the reply reports the backend that served it.
        device: DeviceTarget,
    },
}

/// Capability description of the running service (reply to [`AiRequest::Describe`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Describe<'a> {
    /// Wire-contract version this service implements.
    pub proto_version: u16,
    /// Backend currently serving requests (an [`backend`] flag).
    pub active_backend: u32,
    /// Backends this build has *implemented* ([`backend`] flags).
    pub backends: u32,
    /// Model identity (`general.name` from the GGUF header), empty when no model is resident.
    pub model: &'a str,
    /// Model architecture string (`llama`, …), empty when no model is resident.
    pub arch: &'a str,
    /// Resident weight quantization.
    pub quant: Quant,
    /// Model context window in tokens.
    pub context_tokens: u32,
    /// Vocabulary size.
    pub vocab_size: u32,
    /// Sessions currently alive.
    pub live_sessions: u8,
    /// Maximum concurrent sessions ([`MAX_SESSIONS`]).
    pub max_sessions: u8,
    /// Resident model bytes held by the service (weights + KV cache).
    pub resident_bytes: u32,
}

/// Replies the service sends to a caller Cell.
#[derive(Debug, Serialize, Deserialize)]
pub enum AiResponse<'a> {
    /// Reply to [`AiRequest::Describe`].
    Description(Describe<'a>),
    /// Reply to [`AiRequest::InferSubmit`]: the session id the service assigned.
    Accepted {
        /// Service-assigned session id, unique among live sessions.
        request_id: u32,
        /// Backend that will serve the session.
        backend: u32,
    },
    /// Reply to [`AiRequest::InferStreamPoll`]: tokens/ids generated since the last poll.
    TokenChunk {
        /// Session id.
        request_id: u32,
        /// Token ids as little-endian `u32`, 4 bytes each — see [`token_ids`]. A borrowed
        /// `&[u8]` is the only slice type postcard can hand back without copying, and it costs
        /// nothing over an id array for the ids a real tokenizer produces.
        tokens: &'a [u8],
        /// Text for `tokens` (UTF-8; a multi-byte character may be split across two chunks'
        /// token ids and is re-joined by the caller's `text` accumulation, never by re-encoding).
        text: &'a str,
        /// True when the session is finished and released.
        done: bool,
        /// Why it finished; only meaningful when `done`.
        finish: FinishReason,
    },
    /// Reply to [`AiRequest::InferEmbed`].
    Embedding {
        /// Number of values in `values`.
        dim: u16,
        /// L2-normalized embedding values as little-endian `f32`, 4 bytes each — see
        /// [`embedding_values`].
        values: &'a [u8],
    },
    /// Any request that could not be served.
    Failed {
        /// Session id the failure belongs to; `0` when the request never got one.
        request_id: u32,
        /// Typed cause.
        error: AiError,
    },
}

impl AiResponse<'_> {
    /// Whether this reply answers `request` in the request/reply sense.
    ///
    /// Used by clients to reject stray or stale replies instead of mis-decoding them.
    pub fn matches(&self, request: &AiRequest<'_>) -> bool {
        matches!(
            (self, request),
            (AiResponse::Description(_), AiRequest::Describe)
                | (AiResponse::Accepted { .. }, AiRequest::InferSubmit(_))
                | (AiResponse::TokenChunk { .. }, AiRequest::InferStreamPoll { .. })
                // A cancel is answered with the session's terminal chunk.
                | (AiResponse::TokenChunk { .. }, AiRequest::InferCancel { .. })
                | (AiResponse::Embedding { .. }, AiRequest::InferEmbed { .. })
                | (AiResponse::Failed { .. }, _)
        )
    }
}

/// Validate a request against the contract **before** encoding it.
///
/// The service re-validates every received request: this function only spares the caller a
/// round-trip, it is not a trust boundary.
pub fn validate_request(request: &AiRequest<'_>) -> Result<(), AiError> {
    match request {
        AiRequest::Describe | AiRequest::InferCancel { .. } => Ok(()),
        AiRequest::InferSubmit(submit) => {
            if submit.prompt.len() > MAX_PROMPT_BYTES {
                return Err(AiError::BadRequest(limit::Violation::PromptTooLong));
            }
            if submit.max_tokens == 0 || submit.max_tokens > MAX_TOKENS_PER_REQUEST {
                return Err(AiError::BadRequest(limit::Violation::MaxTokensOutOfRange));
            }
            if submit.temperature_milli > MAX_TEMPERATURE_MILLI {
                return Err(AiError::BadRequest(limit::Violation::TemperatureOutOfRange));
            }
            if submit.temperature_milli > 0 && submit.top_k == 1 {
                return Err(AiError::BadRequest(limit::Violation::TopKOutOfRange));
            }
            Ok(())
        }
        AiRequest::InferStreamPoll { max_tokens, .. } => {
            if *max_tokens == 0 || *max_tokens > MAX_TOKENS_PER_POLL {
                return Err(AiError::BadRequest(limit::Violation::PollTooLarge));
            }
            Ok(())
        }
        AiRequest::InferEmbed { text, .. } => {
            if text.is_empty() || text.len() > MAX_PROMPT_BYTES {
                return Err(AiError::BadRequest(limit::Violation::EmbedInputOutOfRange));
            }
            Ok(())
        }
    }
}

/// Serialize `msg` into `buf`; returns the written prefix.
///
/// Mirrors `api::ipc::encode` so cells use one calling convention.
pub fn encode<'a, T: Serialize>(msg: &T, buf: &'a mut [u8]) -> postcard::Result<&'a mut [u8]> {
    postcard::to_slice(msg, buf)
}

/// Deserialize a typed message from the start of `buf`, tolerating trailing bytes.
pub fn decode<'de, T: Deserialize<'de>>(buf: &'de [u8]) -> postcard::Result<T> {
    postcard::take_from_bytes(buf).map(|(value, _rest)| value)
}

/// Write token ids into `out` as little-endian `u32`.
///
/// Returns the written prefix, or `None` when `out` is too small. The caller passes the result as
/// [`AiResponse::TokenChunk::tokens`].
pub fn encode_token_ids<'a>(ids: &[u32], out: &'a mut [u8]) -> Option<&'a mut [u8]> {
    let bytes = ids.len().checked_mul(TOKEN_ID_BYTES)?;
    if out.len() < bytes {
        return None;
    }
    for (slot, id) in out.chunks_exact_mut(TOKEN_ID_BYTES).zip(ids) {
        slot.copy_from_slice(&id.to_le_bytes());
    }
    Some(&mut out[..bytes])
}

/// Iterate the token ids carried by [`AiResponse::TokenChunk::tokens`].
///
/// A trailing partial word is ignored rather than panicking: a truncated payload is a malformed
/// message, and silently dropping the tail keeps the reader total.
pub fn token_ids(tokens: &[u8]) -> impl Iterator<Item = u32> + '_ {
    tokens.chunks_exact(TOKEN_ID_BYTES).map(|word| {
        let mut bytes = [0u8; TOKEN_ID_BYTES];
        bytes.copy_from_slice(word);
        u32::from_le_bytes(bytes)
    })
}

/// Write embedding values into `out` as little-endian `f32`; see [`encode_token_ids`].
pub fn encode_embedding_values<'a>(values: &[f32], out: &'a mut [u8]) -> Option<&'a mut [u8]> {
    let bytes = values.len().checked_mul(EMBED_VALUE_BYTES)?;
    if out.len() < bytes {
        return None;
    }
    for (slot, value) in out.chunks_exact_mut(EMBED_VALUE_BYTES).zip(values) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    Some(&mut out[..bytes])
}

/// Iterate the values carried by [`AiResponse::Embedding::values`]; see [`token_ids`].
pub fn embedding_values(values: &[u8]) -> impl Iterator<Item = f32> + '_ {
    values.chunks_exact(EMBED_VALUE_BYTES).map(|word| {
        let mut bytes = [0u8; EMBED_VALUE_BYTES];
        bytes.copy_from_slice(word);
        f32::from_le_bytes(bytes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submit<'a>(prompt: &'a str, max_tokens: u16) -> AiRequest<'a> {
        AiRequest::InferSubmit(InferSubmit {
            prompt,
            max_tokens,
            temperature_milli: 0,
            top_k: 0,
            seed: 1,
            device: DeviceTarget::Auto,
        })
    }

    #[test]
    fn round_trips_every_request_variant() {
        let mut buf = [0u8; AI_IPC_BUF_SIZE];
        let ids: [u32; MAX_TOKENS_PER_POLL as usize] = core::array::from_fn(|i| 40_000 + i as u32);
        let mut token_bytes = [0u8; MAX_TOKENS_PER_POLL as usize * TOKEN_ID_BYTES];
        let tokens = encode_token_ids(&ids, &mut token_bytes).expect("fits");
        let tokens: &[u8] = tokens;

        let requests = [
            submit("hello world", 8),
            AiRequest::InferStreamPoll {
                request_id: 7,
                max_tokens: MAX_TOKENS_PER_POLL,
            },
            AiRequest::InferCancel { request_id: 7 },
            AiRequest::InferEmbed {
                text: "embed me",
                device: DeviceTarget::Cpu,
            },
            AiRequest::Describe,
        ];
        for request in &requests {
            let encoded = encode(request, &mut buf).expect("fits");
            let decoded: AiRequest<'_> = decode(encoded).expect("decodes");
            assert_eq!(opcode::of_request(&decoded), opcode::of_request(request));
            assert_eq!(validate_request(&decoded), Ok(()));
        }

        let chunk = AiResponse::TokenChunk {
            request_id: 7,
            tokens,
            text: "…",
            done: true,
            finish: FinishReason::Stop,
        };
        let encoded = encode(&chunk, &mut buf).expect("fits");
        let decoded: AiResponse<'_> = decode(encoded).expect("decodes");
        match decoded {
            AiResponse::TokenChunk {
                request_id,
                tokens,
                done,
                finish,
                ..
            } => {
                assert_eq!(request_id, 7);
                assert_eq!(token_ids(tokens).collect::<Vec<_>>(), ids.to_vec());
                assert!(done);
                assert_eq!(finish, FinishReason::Stop);
            }
            other => panic!("wrong variant: {other:?}"),
        }

        let mut embed_bytes = [0u8; 3 * EMBED_VALUE_BYTES];
        let values = encode_embedding_values(&[0.5, -1.25, 3.0], &mut embed_bytes).expect("fits");
        let embedding = AiResponse::Embedding {
            dim: 3,
            values,
        };
        let encoded = encode(&embedding, &mut buf).expect("fits");
        let decoded: AiResponse<'_> = decode(encoded).expect("decodes");
        match decoded {
            AiResponse::Embedding { dim, values } => {
                assert_eq!(dim, 3);
                let collected: Vec<f32> = embedding_values(values).collect();
                assert_eq!(collected, vec![0.5, -1.25, 3.0]);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn numeric_helpers_reject_short_buffers_and_ignore_trailing_bytes() {
        let mut too_small = [0u8; 7];
        assert!(encode_token_ids(&[1, 2], &mut too_small).is_none());
        assert!(encode_embedding_values(&[1.0, 2.0], &mut too_small).is_none());

        // 8 bytes = two ids; the trailing 3 bytes are a truncated third id and must be ignored.
        let mut bytes = [0u8; 11];
        let written = encode_token_ids(&[9, 9], &mut bytes).expect("fits");
        assert_eq!(written.len(), 8);
        bytes[8..11].copy_from_slice(&[1, 2, 3]);
        assert_eq!(token_ids(&bytes).collect::<Vec<_>>(), vec![9, 9]);
    }

    #[test]
    fn opcode_map_matches_variant_order() {
        assert_eq!(opcode::of_request(&AiRequest::Describe), opcode::DESCRIBE);
        assert_eq!(opcode::of_request(&submit("x", 1)), opcode::INFER_SUBMIT);
        assert_eq!(
            opcode::of_request(&AiRequest::InferStreamPoll {
                request_id: 0,
                max_tokens: 1
            }),
            opcode::INFER_STREAM_POLL
        );
        assert_eq!(
            opcode::of_request(&AiRequest::InferCancel { request_id: 0 }),
            opcode::INFER_CANCEL
        );
        assert_eq!(
            opcode::of_request(&AiRequest::InferEmbed {
                text: "x",
                device: DeviceTarget::Auto
            }),
            opcode::INFER_EMBED
        );
    }

    #[test]
    fn rejects_requests_that_break_the_bounds() {
        let long_prompt = "a".repeat(MAX_PROMPT_BYTES + 1);
        assert_eq!(
            validate_request(&submit(&long_prompt, 1)),
            Err(AiError::BadRequest(limit::Violation::PromptTooLong))
        );
        assert_eq!(
            validate_request(&submit("x", 0)),
            Err(AiError::BadRequest(limit::Violation::MaxTokensOutOfRange))
        );
        assert_eq!(
            validate_request(&submit("x", MAX_TOKENS_PER_REQUEST + 1)),
            Err(AiError::BadRequest(limit::Violation::MaxTokensOutOfRange))
        );
        assert_eq!(
            validate_request(&AiRequest::InferStreamPoll {
                request_id: 1,
                max_tokens: MAX_TOKENS_PER_POLL + 1
            }),
            Err(AiError::BadRequest(limit::Violation::PollTooLarge))
        );
        assert_eq!(
            validate_request(&AiRequest::InferEmbed {
                text: "",
                device: DeviceTarget::Auto
            }),
            Err(AiError::BadRequest(limit::Violation::EmbedInputOutOfRange))
        );

        let hot = AiRequest::InferSubmit(InferSubmit {
            prompt: "x",
            max_tokens: 1,
            temperature_milli: MAX_TEMPERATURE_MILLI + 1,
            top_k: 0,
            seed: 0,
            device: DeviceTarget::Auto,
        });
        assert_eq!(
            validate_request(&hot),
            Err(AiError::BadRequest(limit::Violation::TemperatureOutOfRange))
        );
    }

    #[test]
    fn oversize_payload_is_reported_as_an_encode_error_not_a_panic() {
        let mut small = [0u8; 8];
        let long_prompt = "b".repeat(MAX_PROMPT_BYTES);
        assert!(encode(&submit(&long_prompt, 4), &mut small).is_err());
    }

    #[test]
    fn replies_only_match_their_requests() {
        let describe = AiRequest::Describe;
        let poll = AiRequest::InferStreamPoll {
            request_id: 3,
            max_tokens: 1,
        };
        let description = AiResponse::Description(Describe {
            proto_version: 1,
            active_backend: backend::CPU,
            backends: backend::CPU,
            model: "tiny",
            arch: "llama",
            quant: Quant::Q8_0,
            context_tokens: 128,
            vocab_size: 256,
            live_sessions: 0,
            max_sessions: MAX_SESSIONS,
            resident_bytes: 1024,
        });
        assert!(description.matches(&describe));
        assert!(!description.matches(&poll));

        // A cancel is answered by the session's terminal chunk, not by a description.
        let cancel = AiRequest::InferCancel { request_id: 3 };
        let terminal = AiResponse::TokenChunk {
            request_id: 3,
            tokens: &[],
            text: "",
            done: true,
            finish: FinishReason::Cancelled,
        };
        assert!(terminal.matches(&cancel));
        assert!(terminal.matches(&poll));
        assert!(!description.matches(&cancel));
        assert!(!terminal.matches(&describe));

        let failed = AiResponse::Failed {
            request_id: 0,
            error: AiError::Busy,
        };
        assert!(failed.matches(&describe));
        assert!(failed.matches(&poll));
    }
}
