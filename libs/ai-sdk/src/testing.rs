//! Host-test harness: a scripted stand-in for the inference service.
//!
//! Compiled only under `cfg(test)`. It exists so the client's protocol behaviour (reply matching,
//! bounds, cancel-on-failure, streaming) is tested as observed behaviour rather than as byte
//! expectations, which `ai-proto`'s own tests already pin.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use ai_proto::{AiError, AiRequest, AiResponse, Describe, FinishReason, MAX_SESSIONS};

use crate::transport::AiTransport;
use crate::AiClientError;

/// A reply the scripted service can produce.
pub enum Reply {
    /// Submission accepted with this session id.
    Accepted {
        /// Session id the service assigns.
        request_id: u32,
    },
    /// A poll answer.
    Chunk {
        /// Token ids produced since the previous poll.
        ids: Vec<u32>,
        /// Text for those ids.
        text: &'static str,
        /// Whether the session is finished.
        done: bool,
    },
    /// A capability description.
    Description,
    /// An embedding reply.
    Embedding(Vec<f32>),
    /// A typed refusal.
    Failed(AiError),
}

/// Encode `reply` into `out`; returns the encoded length.
pub fn encode_reply(reply: &Reply, out: &mut [u8]) -> usize {
    let mut token_bytes = [0u8; ai_proto::MAX_TOKENS_PER_POLL as usize * 4];
    let mut value_bytes = [0u8; 64];
    let response = match reply {
        Reply::Accepted { request_id } => AiResponse::Accepted {
            request_id: *request_id,
            backend: ai_proto::backend::CPU,
        },
        Reply::Chunk { ids, text, done } => AiResponse::TokenChunk {
            request_id: 0,
            tokens: ai_proto::encode_token_ids(ids, &mut token_bytes).expect("ids fit"),
            text,
            done: *done,
            finish: if *done {
                FinishReason::Length
            } else {
                FinishReason::Stop
            },
        },
        Reply::Description => AiResponse::Description(Describe {
            proto_version: 1,
            active_backend: ai_proto::backend::CPU,
            backends: ai_proto::backend::CPU,
            model: "tiny-llama-64",
            arch: "llama",
            quant: ai_proto::Quant::Q8_0,
            context_tokens: 128,
            vocab_size: 270,
            live_sessions: 0,
            max_sessions: MAX_SESSIONS,
            resident_bytes: 1,
        }),
        Reply::Embedding(values) => AiResponse::Embedding {
            dim: values.len() as u16,
            values: ai_proto::encode_embedding_values(values, &mut value_bytes).expect("fits"),
        },
        Reply::Failed(error) => AiResponse::Failed {
            request_id: 0,
            error: *error,
        },
    };
    ai_proto::encode(&response, out).expect("reply fits").len()
}

/// A transport that replays a scripted reply list and records the opcodes the client sent.
pub struct ScriptedTransport {
    script: Vec<(u16, Reply)>,
    next: usize,
    calls: Rc<RefCell<Vec<u16>>>,
}

impl ScriptedTransport {
    /// Serve the scripted replies in order.
    pub fn new(script: Vec<(u16, Reply)>) -> Self {
        Self {
            script,
            next: 0,
            calls: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Shared log of request opcodes.
    pub fn calls(&self) -> Rc<RefCell<Vec<u16>>> {
        Rc::clone(&self.calls)
    }
}

impl AiTransport for ScriptedTransport {
    fn round_trip<'r>(
        &mut self,
        request: &AiRequest<'_>,
        reply: &'r mut [u8],
    ) -> Result<AiResponse<'r>, AiClientError> {
        let opcode = ai_proto::opcode::of_request(request);
        self.calls.borrow_mut().push(opcode);

        let (expected, response) = self.script.get(self.next).ok_or(AiClientError::Transport)?;
        self.next += 1;
        let reply_source = if *expected == opcode {
            response
        } else {
            // Answering the wrong opcode models a service talking to someone else; the client must
            // see a refusal rather than act on a mismatched reply.
            &Reply::Failed(AiError::Internal)
        };
        encode_reply(reply_source, reply);
        ai_proto::decode(reply).map_err(|_| AiClientError::Protocol)
    }
}
