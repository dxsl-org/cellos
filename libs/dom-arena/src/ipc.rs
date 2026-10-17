// SPDX-License-Identifier: MIT
//! One active, caller-bound JS context and bounded ordered message transport.
use alloc::string::String;
use alloc::vec::Vec;
use crate::{JsContext, OcelJsCommand, OcelJsReply, OcelJsRequest, OcelJsResponse,
    OCEL_JS_CHUNK_SIZE, OCEL_JS_MAX_TRANSFER, OCEL_JS_MAX_MUTATIONS,
    OCEL_JS_MAX_MUTATION_BYTES, OCEL_JS_MAX_RESULT_BYTES, OCEL_JS_IDLE_TICKS};

/// Count before allocating: even a malicious engine cannot make serialization
/// first allocate an oversized response and only afterwards discover its size.
pub fn serialized_len_bounded<T: serde::Serialize + ?Sized>(value: &T, limit: usize) -> postcard::Result<usize> {
    struct Counter { size: usize, limit: usize }
    impl postcard::ser_flavors::Flavor for Counter {
        type Output = usize;
        fn try_push(&mut self, _: u8) -> postcard::Result<()> { self.try_extend(&[0]) }
        fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
            self.size = self.size.checked_add(bytes.len()).filter(|&n| n <= self.limit)
                .ok_or(postcard::Error::SerializeBufferFull)?;
            Ok(())
        }
        fn finalize(self) -> postcard::Result<usize> { Ok(self.size) }
    }
    postcard::serialize_with_flavor(value, Counter { size: 0, limit })
}

pub fn encode_bounded<T: serde::Serialize + ?Sized>(value: &T, limit: usize) -> postcard::Result<Vec<u8>> {
    let length = serialized_len_bounded(value, limit)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| postcard::Error::SerializeBufferFull)?;
    bytes.resize(length, 0);
    postcard::to_slice(value, &mut bytes)?;
    Ok(bytes)
}

#[derive(Default)]
pub struct JsServiceSession {
    owner: Option<(usize, u64)>,
    incoming: Vec<u8>,
    expected: Option<usize>,
    outgoing: Vec<u8>,
    document_ready: bool,
    last_activity: Option<u64>,
}

impl JsServiceSession {
    /// Runtime entrypoint. Clock absence fails closed rather than granting an
    /// immortal session. Expiry is an explicit idle lease, not owner preemption.
    pub fn handle_at<C: JsContext>(&mut self, caller: usize, request: OcelJsRequest, ctx: &mut C, now: Option<u64>) -> OcelJsReply {
        let Some(now) = now else { return Self::error("JS session clock unavailable"); };
        if self.last_activity.is_some_and(|last| now.wrapping_sub(last) >= OCEL_JS_IDLE_TICKS) {
            self.clear(ctx);
        }
        let authenticated = match &request {
            OcelJsRequest::OpenContext { context } => self.owner.is_none() || self.owner == Some((caller, *context)),
            OcelJsRequest::Begin { context, .. } | OcelJsRequest::Chunk { context, .. }
            | OcelJsRequest::Commit { context } | OcelJsRequest::ReadResponse { context, .. }
            | OcelJsRequest::CloseContext { context } => self.owner == Some((caller, *context)),
            OcelJsRequest::Ping => false,
        };
        let reply = self.handle(caller, request, ctx);
        if authenticated && self.owner.is_some() { self.last_activity = Some(now); }
        reply
    }

    /// An exact reply rejected by the kernel means the owner abandoned the
    /// operation (or died). Never reset somebody else's accepted session.
    pub fn abandon<C: JsContext>(&mut self, caller: usize, context: u64, ctx: &mut C) {
        if self.owner == Some((caller, context)) { self.clear(ctx); }
    }

    fn clear<C: JsContext>(&mut self, ctx: &mut C) {
        self.owner = None;
        self.incoming = Vec::new();
        self.expected = None;
        self.outgoing = Vec::new();
        self.document_ready = false;
        self.last_activity = None;
        ctx.reset();
    }
    pub fn handle<C: JsContext>(&mut self, caller: usize, request: OcelJsRequest, ctx: &mut C) -> OcelJsReply {
        if matches!(request, OcelJsRequest::Ping) {
            return OcelJsReply::Ready;
        }
        if let OcelJsRequest::OpenContext { context } = request {
            if self.owner == Some((caller, context)) { return OcelJsReply::Ack; }
            if self.owner.is_some() { return OcelJsReply::Busy; }
            self.clear(ctx);
            self.owner = Some((caller, context));
            return OcelJsReply::Ack;
        }
        let context = match &request {
            OcelJsRequest::Begin { context, .. } | OcelJsRequest::Chunk { context, .. }
            | OcelJsRequest::Commit { context } | OcelJsRequest::ReadResponse { context, .. }
            | OcelJsRequest::CloseContext { context } => *context,
            OcelJsRequest::OpenContext { .. } | OcelJsRequest::Ping => unreachable!(),
        };
        if self.owner != Some((caller, context)) {
            return Self::error("JS context is not active; explicitly open and synchronize it");
        }
        match request {
            OcelJsRequest::CloseContext { .. } => { self.clear(ctx); OcelJsReply::Ack }
            OcelJsRequest::Begin { total, .. } => {
                if total == 0 || total > OCEL_JS_MAX_TRANSFER { return Self::error("JS request exceeds transfer limit"); }
                self.incoming.clear();
                self.expected = Some(total);
                self.outgoing.clear();
                OcelJsReply::Ack
            }
            OcelJsRequest::Chunk { offset, data, .. } => {
                if self.expected.is_none() || offset != self.incoming.len() || data.is_empty()
                    || data.len() > OCEL_JS_CHUNK_SIZE
                    || offset.saturating_add(data.len()) > self.expected.unwrap_or(0) {
                    return Self::error("JS upload chunk is out of order or out of bounds");
                }
                self.incoming.extend(data);
                OcelJsReply::Ack
            }
            OcelJsRequest::Commit { .. } => {
                if self.expected != Some(self.incoming.len()) { return Self::error("JS upload is incomplete"); }
                self.expected = None;
                let command = postcard::from_bytes::<OcelJsCommand>(&self.incoming);
                self.incoming.clear();
                let result = match command {
                    Ok(OcelJsCommand::SyncDocument { arena, title }) => {
                        if !arena.validate() { return Self::error("Invalid DOM snapshot topology"); }
                        let result = ctx.sync_document(&arena, &title).map(|_| String::from("document_synced"));
                        if result.is_ok() { self.document_ready = true; }
                        result
                    }
                    Ok(OcelJsCommand::Eval { script }) if self.document_ready => ctx.eval(&script),
                    Ok(OcelJsCommand::DispatchEvent { event }) if self.document_ready =>
                        ctx.dispatch_event(&event).map(|_| String::from("event_dispatched")),
                    Ok(_) => return Self::error("Synchronize the document before executing JavaScript"),
                    Err(_) => return Self::error("Malformed JS command"),
                };
                let mutations = ctx.take_mutations();
                let text_length = match &result { Ok(text) => text.len(), Err(error) => error.message.len() };
                if !ctx.is_usable() || mutations.len() > OCEL_JS_MAX_MUTATIONS || text_length > OCEL_JS_MAX_RESULT_BYTES
                    || serialized_len_bounded(&mutations, OCEL_JS_MAX_MUTATION_BYTES).is_err() {
                    self.clear(ctx);
                    return Self::error("JS output budget exceeded; context closed, synchronize to recover");
                }
                let response = match result {
                    Ok(result_repr) => OcelJsResponse::Success { mutations, result_repr },
                    Err(e) => OcelJsResponse::Error { message: e.message, line: e.line, mutations },
                };
                self.outgoing = match encode_bounded(&response, OCEL_JS_MAX_TRANSFER) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        self.clear(ctx);
                        return Self::error("JS response exceeds transfer limit; context closed");
                    }
                };
                self.response_chunk(0)
            }
            OcelJsRequest::ReadResponse { offset, .. } => self.response_chunk(offset),
            OcelJsRequest::OpenContext { .. } | OcelJsRequest::Ping => unreachable!(),
        }
    }

    fn response_chunk(&self, offset: usize) -> OcelJsReply {
        if offset >= self.outgoing.len() { return Self::error("JS response offset is out of bounds"); }
        let end = offset.saturating_add(OCEL_JS_CHUNK_SIZE).min(self.outgoing.len());
        OcelJsReply::ResponseChunk { offset, total: self.outgoing.len(), data: self.outgoing[offset..end].to_vec() }
    }

    pub fn error(message: &str) -> OcelJsReply { OcelJsReply::Error { message: String::from(message) } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentArena, DomEvent, DomMutation, JsError};
    #[derive(Default)]
    struct Context { resets: usize, evals: usize, mutation_count: usize, mutation_bytes: usize }
    impl JsContext for Context {
        fn sync_document(&mut self, _: &DocumentArena, _: &str) -> Result<(), JsError> { Ok(()) }
        fn eval(&mut self, script: &str) -> Result<String, JsError> { self.evals += 1; Ok(String::from(script)) }
        fn dispatch_event(&mut self, _: &DomEvent) -> Result<(), JsError> { Ok(()) }
        fn take_mutations(&mut self) -> Vec<DomMutation> {
            let count = core::mem::take(&mut self.mutation_count);
            let bytes = core::mem::take(&mut self.mutation_bytes);
            (0..count).map(|_| DomMutation::SetDocumentTitle { title: "x".repeat(bytes) }).collect()
        }
        fn reset(&mut self) { self.resets += 1; }
    }
    #[test]
    fn readiness_probe_preserves_another_callers_inflight_upload() {
        let mut session = JsServiceSession::default();
        let mut ctx = Context::default();
        session.handle(1, OcelJsRequest::OpenContext { context: 7 }, &mut ctx);
        session.handle(1, OcelJsRequest::Begin { context: 7, total: 2 }, &mut ctx);
        session.handle(1, OcelJsRequest::Chunk { context: 7, offset: 0, data: alloc::vec![0] }, &mut ctx);
        session.handle(2, OcelJsRequest::Ping, &mut ctx);
        assert!(matches!(
            session.handle(1, OcelJsRequest::Chunk { context: 7, offset: 1, data: alloc::vec![1] }, &mut ctx),
            OcelJsReply::Ack
        ));
        assert!(matches!(
            session.handle(2, OcelJsRequest::Commit { context: 7 }, &mut ctx),
            OcelJsReply::Error { .. }
        ));
        assert_eq!(ctx.resets, 1);
    }
    #[test]
    fn context_is_bound_to_caller_and_explicit_token() {
        let mut session = JsServiceSession::default();
        let mut ctx = Context::default();
        assert!(matches!(session.handle(1, OcelJsRequest::OpenContext { context: 7 }, &mut ctx), OcelJsReply::Ack));
        for (caller, context) in [(2, 7), (1, 8)] {
            assert!(matches!(session.handle(caller, OcelJsRequest::Begin { context, total: 1 }, &mut ctx), OcelJsReply::Error { .. }));
        }
        assert!(matches!(session.handle(2, OcelJsRequest::OpenContext { context: 7 }, &mut ctx), OcelJsReply::Busy));
        assert!(matches!(session.handle(1, OcelJsRequest::OpenContext { context: 8 }, &mut ctx), OcelJsReply::Busy));
        assert!(matches!(session.handle(1, OcelJsRequest::OpenContext { context: 7 }, &mut ctx), OcelJsReply::Ack));
        assert_eq!(ctx.resets, 1);
        assert!(matches!(session.handle(2, OcelJsRequest::CloseContext { context: 7 }, &mut ctx), OcelJsReply::Error { .. }));
        assert!(matches!(session.handle(1, OcelJsRequest::CloseContext { context: 7 }, &mut ctx), OcelJsReply::Ack));
        assert!(matches!(session.handle(2, OcelJsRequest::OpenContext { context: 7 }, &mut ctx), OcelJsReply::Ack));
        assert_eq!(ctx.resets, 3);
    }
    #[test]
    fn upload_rejects_gaps_truncation_and_oversize() {
        let mut session = JsServiceSession::default();
        let mut ctx = Context::default();
        session.handle(1, OcelJsRequest::OpenContext { context: 1 }, &mut ctx);
        assert!(matches!(session.handle(1, OcelJsRequest::Begin { context: 1, total: OCEL_JS_MAX_TRANSFER + 1 }, &mut ctx), OcelJsReply::Error { .. }));
        session.handle(1, OcelJsRequest::Begin { context: 1, total: 2 }, &mut ctx);
        assert!(matches!(session.handle(1, OcelJsRequest::Chunk { context: 1, offset: 1, data: alloc::vec![1] }, &mut ctx), OcelJsReply::Error { .. }));
        assert!(matches!(session.handle(1, OcelJsRequest::Commit { context: 1 }, &mut ctx), OcelJsReply::Error { .. }));
        assert_eq!(ctx.evals, 0);
    }
    fn command(session: &mut JsServiceSession, ctx: &mut Context, command: OcelJsCommand) -> OcelJsReply {
        let bytes = encode_bounded(&command, OCEL_JS_MAX_TRANSFER).unwrap();
        assert!(matches!(session.handle(1, OcelJsRequest::Begin { context: 1, total: bytes.len() }, ctx), OcelJsReply::Ack));
        for (index, data) in bytes.chunks(OCEL_JS_CHUNK_SIZE).enumerate() {
            assert!(matches!(session.handle(1, OcelJsRequest::Chunk { context: 1, offset: index * OCEL_JS_CHUNK_SIZE, data: data.to_vec() }, ctx), OcelJsReply::Ack));
        }
        session.handle(1, OcelJsRequest::Commit { context: 1 }, ctx)
    }

    #[test]
    fn mutation_count_bytes_and_result_limits_close_and_recover() {
        for (count, bytes, script) in [
            (OCEL_JS_MAX_MUTATIONS + 1, 0, String::from("normal")),
            (1, OCEL_JS_MAX_MUTATION_BYTES + 1, String::from("normal")),
            (0, 0, "x".repeat(OCEL_JS_MAX_RESULT_BYTES + 1)),
        ] {
            let mut session = JsServiceSession::default();
            let mut ctx = Context::default();
            session.handle(1, OcelJsRequest::OpenContext { context: 1 }, &mut ctx);
            assert!(matches!(command(&mut session, &mut ctx, OcelJsCommand::SyncDocument { arena: DocumentArena::new(), title: String::new() }), OcelJsReply::ResponseChunk { .. }));
            ctx.mutation_count = count; ctx.mutation_bytes = bytes;
            assert!(matches!(command(&mut session, &mut ctx, OcelJsCommand::Eval { script }), OcelJsReply::Error { .. }));
            assert!(session.owner.is_none());
            assert!(session.outgoing.is_empty());
            assert!(matches!(session.handle(1, OcelJsRequest::OpenContext { context: 1 }, &mut ctx), OcelJsReply::Ack));
            assert!(matches!(command(&mut session, &mut ctx, OcelJsCommand::SyncDocument { arena: DocumentArena::new(), title: String::new() }), OcelJsReply::ResponseChunk { .. }));
            assert!(matches!(command(&mut session, &mut ctx, OcelJsCommand::Eval { script: String::from("recovered") }), OcelJsReply::ResponseChunk { .. }));
        }
    }

    #[test]
    fn idle_expiry_and_abandoned_reply_reclaim_only_owner() {
        let mut session = JsServiceSession::default();
        let mut ctx = Context::default();
        assert!(matches!(session.handle_at(1, OcelJsRequest::OpenContext { context: 1 }, &mut ctx, Some(0)), OcelJsReply::Ack));
        assert!(matches!(session.handle_at(2, OcelJsRequest::OpenContext { context: 2 }, &mut ctx, Some(1)), OcelJsReply::Busy));
        session.abandon(2, 2, &mut ctx);
        session.abandon(1, 2, &mut ctx);
        assert_eq!(session.owner, Some((1, 1)));
        session.handle_at(1, OcelJsRequest::Ping, &mut ctx, Some(OCEL_JS_IDLE_TICKS - 1));
        assert!(matches!(session.handle_at(2, OcelJsRequest::OpenContext { context: 2 }, &mut ctx, Some(OCEL_JS_IDLE_TICKS)), OcelJsReply::Ack));
        session.abandon(1, 1, &mut ctx);
        assert_eq!(session.owner, Some((2, 2)));
        session.abandon(2, 2, &mut ctx);
        assert!(session.owner.is_none());
    }

    #[test]
    fn serializer_checks_size_before_allocating_output() {
        let text = "x".repeat(100);
        assert!(encode_bounded(&text, 100).is_err()); // length prefix also counts
        let bytes = encode_bounded(&text, 101).unwrap();
        assert_eq!(bytes.len(), 101);
    }
}
