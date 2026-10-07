// SPDX-License-Identifier: MIT
//! One active, caller-bound JS context and bounded ordered message transport.
use alloc::string::String;
use alloc::vec::Vec;
use crate::{JsContext, OcelJsCommand, OcelJsReply, OcelJsRequest, OcelJsResponse,
    OCEL_JS_CHUNK_SIZE, OCEL_JS_MAX_TRANSFER};

#[derive(Default)]
pub struct JsServiceSession {
    owner: Option<(usize, u64)>,
    incoming: Vec<u8>,
    expected: Option<usize>,
    outgoing: Vec<u8>,
    document_ready: bool,
}

impl JsServiceSession {
    pub fn handle<C: JsContext>(&mut self, caller: usize, request: OcelJsRequest, ctx: &mut C) -> OcelJsReply {
        if matches!(request, OcelJsRequest::Ping) {
            return OcelJsReply::Ready;
        }
        if let OcelJsRequest::OpenContext { context } = request {
            ctx.reset();
            self.owner = Some((caller, context));
            self.incoming.clear();
            self.expected = None;
            self.outgoing.clear();
            self.document_ready = false;
            return OcelJsReply::Ack;
        }
        let context = match &request {
            OcelJsRequest::Begin { context, .. } | OcelJsRequest::Chunk { context, .. }
            | OcelJsRequest::Commit { context } | OcelJsRequest::ReadResponse { context, .. } => *context,
            OcelJsRequest::OpenContext { .. } | OcelJsRequest::Ping => unreachable!(),
        };
        if self.owner != Some((caller, context)) {
            return Self::error("JS context is not active; explicitly open and synchronize it");
        }
        match request {
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
                let response = match result {
                    Ok(result_repr) => OcelJsResponse::Success { mutations, result_repr },
                    Err(e) => OcelJsResponse::Error { message: e.message, line: e.line, mutations },
                };
                self.outgoing = match postcard::to_allocvec(&response) {
                    Ok(bytes) if bytes.len() <= OCEL_JS_MAX_TRANSFER => bytes,
                    _ => {
                        // Never continue with JS/native state diverged after losing mutations.
                        self.owner = None;
                        self.document_ready = false;
                        ctx.reset();
                        return Self::error("JS response exceeds transfer limit; context invalidated");
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
    struct Context { resets: usize, evals: usize }
    impl JsContext for Context {
        fn sync_document(&mut self, _: &DocumentArena, _: &str) -> Result<(), JsError> { Ok(()) }
        fn eval(&mut self, script: &str) -> Result<String, JsError> { self.evals += 1; Ok(String::from(script)) }
        fn dispatch_event(&mut self, _: &DomEvent) -> Result<(), JsError> { Ok(()) }
        fn take_mutations(&mut self) -> Vec<DomMutation> { Vec::new() }
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
        session.handle(2, OcelJsRequest::OpenContext { context: 7 }, &mut ctx);
        assert_eq!(ctx.resets, 2);
        assert!(matches!(session.handle(1, OcelJsRequest::Commit { context: 7 }, &mut ctx), OcelJsReply::Error { .. }));
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
}
