// SPDX-License-Identifier: MIT
//! Caller/token-isolated Tier 2 JS bridge. Missing engines fail explicitly;
//! the line matcher is not a substitute for a document-backed JavaScript DOM.
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use dom_arena::{DocumentArena, DomEvent, DomMutation, JsContext, JsEngine, JsError,
    OcelJsCommand, OcelJsReply, OcelJsRequest, OcelJsResponse,
    OCEL_JS_CHUNK_SIZE, OCEL_JS_IPC_BUF_SIZE, OCEL_JS_MAX_TRANSFER};
use crate::lease::{KernelTransport, Lease, Transport};
use ocel_service_proto::Engine;

static NEXT_CONTEXT: AtomicUsize = AtomicUsize::new(1);

pub struct Tier2JsBridge<T: Transport = KernelTransport> {
    lease: Option<Lease<T>>,
    context: u64,
    opened: bool,
    document_ready: bool,
    mutations: Vec<DomMutation>,
    backend_named: bool,
}

impl<T: Transport + Default> Default for Tier2JsBridge<T> { fn default() -> Self { Self::new() } }

impl<T: Transport + Default> Tier2JsBridge<T> {
    pub fn new() -> Self {
        Self { lease: None, context: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed) as u64,
            opened: false, document_ready: false, mutations: Vec::new(), backend_named: false }
    }

    /// Bind this token to the current document/tab. A reset invalidates old tokens.
    /// Never sync a different document into an existing token: reset it first.
    pub fn context_id(&self) -> u64 { self.context }

    /// Call before scripts, and after native edits only once JS mutations have
    /// been drained AND applied. Same-document wrapper identity/listeners survive.
    pub fn sync_document(&mut self, arena: &DocumentArena, title: &str) -> Result<(), JsError> {
        let result = self.sync_active_document(arena, title);
        if result.is_err() { self.invalidate(); }
        result
    }

    /// The native load path calls this gate for every format. An HTML arena by
    /// itself is not JavaScript demand: only supported executable scripts are.
    pub fn sync_script_document(&mut self, arena: Option<&DocumentArena>, has_scripts: bool, title: &str) -> Result<bool, JsError> {
        self.reset();
        if !has_scripts { return Ok(false); }
        let Some(arena) = arena else { return Ok(false); };
        self.sync_document(arena, title).map(|_| true)
    }

    fn sync_active_document(&mut self, arena: &DocumentArena, title: &str) -> Result<(), JsError> {
        if !arena.validate() || !self.mutations.is_empty() {
            return Err(Self::error("Invalid DOM snapshot or undrained JavaScript mutations"));
        }
        if self.lease.is_none() {
            self.lease = Some(Lease::acquire_with(Engine::JavaScript, T::default())
                .map_err(|message| JsError { message, line: 0 })?);
        }
        if !self.opened {
            self.expect_ack(&OcelJsRequest::OpenContext { context: self.context })?;
            self.opened = true;
        }
        self.execute(OcelJsCommand::SyncDocument { arena: arena.clone(), title: String::from(title) })?;
        self.document_ready = true;
        Ok(())
    }

    fn invalidate(&mut self) {
        self.opened = false;
        self.document_ready = false;
        self.lease = None;
    }

    fn error(message: &str) -> JsError { JsError { message: String::from(message), line: 0 } }

    fn active_lease(&mut self) -> Result<&mut Lease<T>, JsError> {
        self.lease.as_mut().ok_or_else(|| Self::error("No active JavaScript engine lease; synchronize a script-bearing document first"))
    }

    fn exchange(&mut self, request: &OcelJsRequest) -> Result<OcelJsReply, JsError> {
        if self.lease.is_none() { return Err(Self::error("No active JavaScript engine lease")); }
        let mut send_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];
        let mut recv_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];
        let encoded = postcard::to_slice(request, &mut send_buf)
            .map_err(|_| Self::error("JavaScript IPC frame exceeds 4 KiB"))?;
        let reply = self.active_lease()?.exchange(encoded, &mut recv_buf)
            .map_err(|message| JsError { message, line: 0 })
            .and_then(|len| postcard::from_bytes::<OcelJsReply>(&recv_buf[..len])
                .map_err(|_| Self::error("Malformed JavaScript service response")));
        match reply {
            Ok(OcelJsReply::Error { message }) => {
                self.invalidate();
                Err(JsError { message, line: 0 })
            }
            Err(error) => {
                self.invalidate();
                Err(error)
            }
            Ok(reply) => Ok(reply),
        }
    }

    fn expect_ack(&mut self, request: &OcelJsRequest) -> Result<(), JsError> {
        match self.exchange(request)? {
            OcelJsReply::Ack => Ok(()),
            _ => {
                self.invalidate();
                Err(Self::error("Unexpected JavaScript IPC acknowledgement"))
            }
        }
    }

    fn execute(&mut self, command: OcelJsCommand) -> Result<String, JsError> {
        let payload = postcard::to_allocvec(&command).map_err(|_| Self::error("Unable to encode JavaScript command"))?;
        if payload.len() > OCEL_JS_MAX_TRANSFER { return Err(Self::error("JavaScript request exceeds 512 KiB transfer limit")); }
        self.expect_ack(&OcelJsRequest::Begin { context: self.context, total: payload.len() })?;
        for (index, chunk) in payload.chunks(OCEL_JS_CHUNK_SIZE).enumerate() {
            self.expect_ack(&OcelJsRequest::Chunk { context: self.context,
                offset: index * OCEL_JS_CHUNK_SIZE, data: chunk.to_vec() })?;
        }
        let mut reply = self.exchange(&OcelJsRequest::Commit { context: self.context })?;
        let mut bytes = Vec::new();
        let mut expected_total = None;
        loop {
            let OcelJsReply::ResponseChunk { offset, total, data } = reply else {
                self.invalidate();
                return Err(Self::error("Unexpected JavaScript IPC response"));
            };
            if total == 0 || total > OCEL_JS_MAX_TRANSFER || offset != bytes.len()
                || data.is_empty() || data.len() > OCEL_JS_CHUNK_SIZE
                || offset.saturating_add(data.len()) > total
                || expected_total.is_some_and(|expected| expected != total) {
                self.invalidate();
                return Err(Self::error("Invalid JavaScript response chunk"));
            }
            expected_total = Some(total);
            bytes.extend(data);
            if bytes.len() == total { break; }
            reply = self.exchange(&OcelJsRequest::ReadResponse { context: self.context, offset: bytes.len() })?;
        }
        let response = postcard::from_bytes::<OcelJsResponse>(&bytes)
            .map_err(|_| { self.invalidate(); Self::error("Malformed JavaScript mutation response") })?;
        if !self.backend_named {
            ostd::io::println("[ocel] js backend: Tier 2 domain service (document-backed DOM required)");
            self.backend_named = true;
        }
        match response {
            OcelJsResponse::Success { mutations, result_repr } => { self.mutations.extend(mutations); Ok(result_repr) }
            OcelJsResponse::Error { message, line, mutations } => { self.mutations.extend(mutations); Err(JsError { message, line }) }
        }
    }
}

impl<T: Transport + Default> JsEngine for Tier2JsBridge<T> {
    type Context = Tier2JsBridge<T>;
    fn create_context(&mut self) -> Self::Context { Self::new() }
}

impl<T: Transport + Default> JsContext for Tier2JsBridge<T> {
    fn sync_document(&mut self, arena: &DocumentArena, title: &str) -> Result<(), JsError> {
        Tier2JsBridge::sync_document(self, arena, title)
    }
    fn eval(&mut self, script: &str) -> Result<String, JsError> {
        if !self.document_ready { return Err(Self::error("Synchronize the active document before executing JavaScript")); }
        self.execute(OcelJsCommand::Eval { script: String::from(script) })
    }
    fn dispatch_event(&mut self, event: &DomEvent) -> Result<(), JsError> {
        if !self.document_ready { return Err(Self::error("No synchronized JavaScript document context")); }
        self.execute(OcelJsCommand::DispatchEvent { event: event.clone() }).map(|_| ())
    }
    fn take_mutations(&mut self) -> Vec<DomMutation> { core::mem::take(&mut self.mutations) }
    fn reset(&mut self) {
        self.context = NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed) as u64;
        self.invalidate();
        self.mutations.clear();
    }
}

#[cfg(test)]
mod tests;
