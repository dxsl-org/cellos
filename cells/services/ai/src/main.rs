//! Unified AI inference service Cell (`/bin/ai`) — Spec 24 §2.
//!
//! One event loop serves every caller:
//!
//! * `Describe` answers from the resident engine.
//! * `InferSubmit` seats a session in a bounded table (owner = the sender tid that asked for it).
//! * `InferStreamPoll` advances *its own* session by a bounded number of model steps and returns
//!   the tokens produced since the previous poll, so one request can never monopolise the loop and
//!   several callers make progress concurrently.
//! * `InferCancel` releases a session; a session abandoned by a dead client is reclaimed after
//!   [`SESSION_IDLE_TICKS`] of inactivity, because a service without lifecycle authority cannot
//!   watch another Cell's exit.
//!
//! Truthfulness rules:
//!
//! * No model, an unsupported architecture, or an over-budget model is reported through
//!   [`AiError`] — the service never answers with fabricated tokens.
//! * A session belongs to the Cell that submitted it: another Cell polling or cancelling that id
//!   gets `UnknownRequest`, the same answer it would get for an id that never existed.
//! * Non-CPU device targets are refused with `NotSupported` rather than silently downgraded.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use ai_engine::{Engine, EngineError, SamplingParams};
use ai_proto::{
    self, backend, limit, AiError, AiRequest, AiResponse, DeviceTarget, FinishReason, MAX_EMBED_DIM,
    MAX_SESSIONS, MAX_TOKENS_PER_POLL,
};
use ostd::io::{print, print_usize, println};
use ostd::syscall::SyscallResult;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, TryRecv, Log, LookupService, GetTime, Yield];

// Cell heap. Holds the model bytes, the engine's weights/scratch, and the session KV caches.
ostd::declare_custom_heap!(8 * 1024 * 1024);

/// Model path. The P6 FAT cell-store is mounted at `/bin`, so this is the FAT root entry
/// `ai-model.gguf` (deployed by `gen_disk.ps1`).
const MODEL_PATH: &str = "/bin/ai-model.gguf";

/// Ceiling handed to the engine. Must stay below the arena above, leaving room for the read
/// buffer and the session table.
const ENGINE_LIMIT: usize = 6 * 1024 * 1024;

/// Model steps advanced per poll. Bounds one session's share of the event loop; a longer
/// generation simply takes more polls.
const STEPS_PER_POLL: usize = 4;

/// Sessions idle for this many 10 ms ticks are reclaimed.
///
/// A service Cell cannot watch another Cell's exit (`NotifyOnExit` needs lifecycle authority), so
/// an abandoned session is released by deadline instead: 30 s of silence is far longer than any
/// poll interval a live client produces.
const SESSION_IDLE_TICKS: u64 = 3_000;

/// One live session and the Cell that owns it.
struct Session {
    request_id: u32,
    owner: usize,
    last_activity: u64,
}

/// The service state.
struct AiService {
    engine: Option<Engine>,
    sessions: [Option<Session>; MAX_SESSIONS as usize],
    /// Why inference is unavailable when `engine` is `None`, so refusals stay specific.
    unavailable: AiError,
}

ostd::cell_main!(cell_main);

fn cell_main() {
    init_custom_heap();
    let mut service = AiService::load();

    let mut buf = [0u8; ai_proto::AI_IPC_BUF_SIZE];
    let mut reply = [0u8; ai_proto::AI_IPC_BUF_SIZE];

    loop {
        match ostd::syscall::sys_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                service.reap_idle_sessions();
                if let Some(encoded) = service.handle(sender, &buf, &mut reply) {
                    let _ = ostd::syscall::sys_send(sender, encoded);
                }
            }
            _ => ostd::task::yield_now(),
        }
    }
}

impl AiService {
    /// Load the model (if present) and build the service.
    fn load() -> Self {
        println("[ai] inference service starting");
        let mut service = Self {
            engine: None,
            sessions: [const { None }; MAX_SESSIONS as usize],
            unavailable: AiError::NoModel,
        };

        let mut vfs = ostd::clients::VfsClient::new();
        let bytes = match vfs.read_file_bounded(MODEL_PATH, ENGINE_LIMIT) {
            Ok(bytes) => bytes,
            Err(_) => {
                print("[ai] no model at ");
        print_usize(MODEL_PATH.len());
                println(" — inference will be refused");
                return service;
            }
        };

        print("[ai] model bytes: ");
        print_usize(bytes.len());
        match Engine::load(&bytes, ENGINE_LIMIT) {
            Ok(engine) => {
                let describe = engine.describe();
                print("[ai] model ready: ");
        print_usize(describe.vocab_size as usize);
                print(" vocab, context ");
        print_usize(describe.context_tokens as usize);
                print(", resident bytes ");
        print_usize(describe.resident_bytes as usize);
                println("");
                service.engine = Some(engine);
            }
            Err(error) => {
                service.unavailable = match error {
                    EngineError::UnsupportedArchitecture
                    | EngineError::UnsupportedDType(_)
                    | EngineError::TooLarge { .. } => AiError::NotSupported,
                    _ => AiError::NoModel,
                };
                println("[ai] model rejected — inference will be refused");
            }
        }
        service
    }

    /// Release sessions whose owner has gone quiet.
    ///
    /// Dead client Cells cannot be watched without lifecycle authority, so silence is the signal:
    /// a live client polls or cancels well inside [`SESSION_IDLE_TICKS`].
    fn reap_idle_sessions(&mut self) {
        let now = ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0);
        let mut expired = [0u32; MAX_SESSIONS as usize];
        let mut expired_count = 0usize;

        for slot in self.sessions.iter_mut() {
            let Some(session) = slot.as_ref() else {
                continue;
            };
            if now.wrapping_sub(session.last_activity) <= SESSION_IDLE_TICKS {
                continue;
            }
            expired[expired_count] = session.request_id;
            expired_count += 1;
            *slot = None;
        }

        if let Some(engine) = self.engine.as_mut() {
            for request_id in expired.iter().take(expired_count) {
                let _ = engine.cancel(*request_id);
            }
        }
    }

    /// Answer one request. `None` when the request must be ignored (nothing can be replied to it).
    fn handle<'r>(
        &mut self,
        sender: usize,
        buf: &[u8],
        reply: &'r mut [u8],
    ) -> Option<&'r [u8]> {
        let request = match ai_proto::decode::<AiRequest<'_>>(buf) {
            Ok(request) => request,
            Err(_) => {
                let error = AiError::BadRequest(limit::Violation::MessageTooLarge);
                log_refusal("decode", error);
                return Some(encode_failure(reply, 0, error));
            }
        };
        if let Err(error) = ai_proto::validate_request(&request) {
            log_refusal("validate", error);
            return Some(encode_failure(reply, 0, error));
        }

        match request {
            AiRequest::Describe => match self.engine.as_ref() {
                Some(engine) => ai_proto::encode(&AiResponse::Description(engine.describe()), reply)
                    .ok()
                    .map(|encoded| &*encoded),
                None => Some(encode_failure(reply, 0, self.unavailable)),
            },
            AiRequest::InferSubmit(submit) => {
                if !matches!(submit.device, DeviceTarget::Auto | DeviceTarget::Cpu) {
                    return Some(encode_failure(reply, 0, AiError::NotSupported));
                }
                let unavailable = self.unavailable;
                let Some(engine) = self.engine.as_mut() else {
                    return Some(encode_failure(reply, 0, unavailable));
                };
                let params = SamplingParams {
                    max_tokens: submit.max_tokens,
                    temperature_milli: submit.temperature_milli,
                    top_k: submit.top_k,
                    seed: submit.seed,
                };
                match engine.submit(submit.prompt, params) {
                    Ok(request_id) => {
                        self.seat(sender, request_id);
                        ai_proto::encode(
                            &AiResponse::Accepted {
                                request_id,
                                backend: backend::CPU,
                            },
                            reply,
                        )
                        .ok()
                        .map(|encoded| &*encoded)
                    }
                    Err(error) => {
                        log_refusal("submit", error);
                        Some(encode_failure(reply, 0, error))
                    }
                }
            }
            AiRequest::InferStreamPoll {
                request_id,
                max_tokens,
            } => self.poll(sender, request_id, max_tokens, reply),
            AiRequest::InferCancel { request_id } => {
                if !self.owned_by(sender, request_id) {
                    return Some(encode_failure(reply, request_id, AiError::UnknownRequest));
                }
                if let Some(engine) = self.engine.as_mut() {
                    let _ = engine.cancel(request_id);
                }
                self.release(request_id);
                ai_proto::encode(
                    &AiResponse::TokenChunk {
                        request_id,
                        tokens: &[],
                        text: "",
                        done: true,
                        finish: FinishReason::Cancelled,
                    },
                    reply,
                )
                .ok()
                .map(|encoded| &*encoded)
            }
            AiRequest::InferEmbed { text, device } => {
                if !matches!(device, DeviceTarget::Auto | DeviceTarget::Cpu) {
                    return Some(encode_failure(reply, 0, AiError::NotSupported));
                }
                let unavailable = self.unavailable;
                let Some(engine) = self.engine.as_mut() else {
                    return Some(encode_failure(reply, 0, unavailable));
                };
                match engine.embed(text) {
                    Ok(values) => {
                        let mut value_bytes =
                            [0u8; MAX_EMBED_DIM as usize * ai_proto::EMBED_VALUE_BYTES];
                        let encoded_values =
                            ai_proto::encode_embedding_values(&values, &mut value_bytes)?;
                        ai_proto::encode(
                            &AiResponse::Embedding {
                                dim: values.len() as u16,
                                values: encoded_values,
                            },
                            reply,
                        )
                        .ok()
                        .map(|encoded| &*encoded)
                    }
                    Err(error) => {
                        log_refusal("embed", error);
                        Some(encode_failure(reply, 0, error))
                    }
                }
            }
        }
    }

    /// Advance a session and report what it produced.
    fn poll<'r>(
        &mut self,
        sender: usize,
        request_id: u32,
        max_tokens: u8,
        reply: &'r mut [u8],
    ) -> Option<&'r [u8]> {
        if !self.owned_by(sender, request_id) {
            return Some(encode_failure(reply, request_id, AiError::UnknownRequest));
        }
        let unavailable = self.unavailable;
        let Some(engine) = self.engine.as_mut() else {
            return Some(encode_failure(reply, request_id, unavailable));
        };

        if let Err(error) = engine.generate(request_id, STEPS_PER_POLL) {
            if error == AiError::UnknownRequest {
                self.release(request_id);
            }
            log_refusal("generate", error);
            return Some(encode_failure(reply, request_id, error));
        }
        let drained = match engine.drain(request_id, max_tokens as usize) {
            Ok(drained) => drained,
            Err(error) => {
                log_refusal("drain", error);
                return Some(encode_failure(reply, request_id, error));
            }
        };

        let mut token_bytes = [0u8; MAX_TOKENS_PER_POLL as usize * ai_proto::TOKEN_ID_BYTES];
        let encoded_tokens = ai_proto::encode_token_ids(&drained.ids, &mut token_bytes)?;
        let finish = drained.finish.unwrap_or(FinishReason::Length);
        let encoded = ai_proto::encode(
            &AiResponse::TokenChunk {
                request_id,
                tokens: encoded_tokens,
                text: drained.text.as_str(),
                done: drained.done,
                finish,
            },
            reply,
        )
        .ok()
        .map(|encoded| &*encoded);

        if drained.done {
            if let Some(engine) = self.engine.as_mut() {
                let _ = engine.release(request_id);
            }
            self.release(request_id);
        } else {
            self.touch(request_id);
        }
        encoded
    }

    /// Take a session slot for `owner`.
    fn seat(&mut self, owner: usize, request_id: u32) {
        let now = ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0);
        if let Some(slot) = self.sessions.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(Session {
                request_id,
                owner,
                last_activity: now,
            });
        }
    }

    /// Whether `request_id` is seated and belongs to `owner`.
    fn owned_by(&self, owner: usize, request_id: u32) -> bool {
        self.sessions
            .iter()
            .flatten()
            .any(|session| session.request_id == request_id && session.owner == owner)
    }

    /// Drop a session slot.
    fn release(&mut self, request_id: u32) {
        for slot in self.sessions.iter_mut() {
            if matches!(slot, Some(session) if session.request_id == request_id) {
                *slot = None;
            }
        }
    }

    /// Mark a session as active so idle reaping leaves it alone.
    fn touch(&mut self, request_id: u32) {
        let now = ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0);
        for session in self.sessions.iter_mut().flatten() {
            if session.request_id == request_id {
                session.last_activity = now;
            }
        }
    }
}

/// Log a refusal with its typed cause, so a failing deployment is diagnosable from the
/// serial console instead of only from the caller's side.
fn log_refusal(stage: &str, error: AiError) {
    ostd::io::print("[ai] refused ");
    ostd::io::println(stage);
    ostd::io::print_fmt(format_args!("[ai] cause: {error:?}\n"));
}

/// Encode a typed refusal.
fn encode_failure(reply: &mut [u8], request_id: u32, error: AiError) -> &[u8] {
    ai_proto::encode(
        &AiResponse::Failed {
            request_id,
            error,
        },
        reply,
    )
    .map(|encoded| &*encoded)
    .unwrap_or(&[])
}
