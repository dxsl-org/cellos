// SPDX-License-Identifier: MIT
//! Caller/token-isolated Tier 2 JS bridge. Missing engines fail explicitly;
//! the line matcher is not a substitute for a document-backed JavaScript DOM.
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use dom_arena::{BrowserEnvironment, BrowserFrame, BrowserReply, BrowserRequest, NodeId,
    DocumentArena, DomEvent, DomMutation, JsContext, JsEngine, JsError,
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
    last_activity: Option<u64>,
    browser_requests: Vec<BrowserRequest>,
    browser_wakeup: Option<u64>,
}

impl<T: Transport + Default> Default for Tier2JsBridge<T> { fn default() -> Self { Self::new() } }

impl<T: Transport + Default> Tier2JsBridge<T> {
    pub fn new() -> Self {
        Self { lease: None, context: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed) as u64,
            opened: false, document_ready: false, mutations: Vec::new(), backend_named: false, last_activity: None,
            browser_requests: Vec::new(), browser_wakeup: None }
    }

    /// Bind this token to the current document/tab. A reset invalidates old tokens.
    /// Never sync a different document into an existing token: reset it first.
    pub fn context_id(&self) -> u64 { self.context }
    /// False after navigation, scripts-off, or any terminal service error.
    pub fn is_active(&self) -> bool {
        self.opened && self.document_ready && !self.last_activity.is_some_and(|last| {
            self.lease.as_ref().and_then(Lease::now).is_some_and(|now| now.wrapping_sub(last) >= dom_arena::OCEL_JS_IDLE_TICKS)
        })
    }

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
        if self.opened && !self.is_active() { self.close_context(); self.document_ready = false; }
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

    pub fn eval_script(&mut self, script: &str, node: Option<NodeId>) -> Result<String, JsError> {
        self.require_active()?;
        self.execute(OcelJsCommand::EvalScript { script: String::from(script), node })
    }

    pub fn configure_browser(&mut self, environment: &BrowserEnvironment) -> Result<(), JsError> {
        self.require_active()?;
        self.execute(OcelJsCommand::ConfigureBrowser { environment: environment.clone() }).map(|_| ())
    }

    pub fn pump_browser(&mut self, frame: &BrowserFrame) -> Result<(), JsError> {
        self.require_active()?;
        self.execute(OcelJsCommand::PumpBrowser { frame: frame.clone() }).map(|_| ())
    }

    pub fn complete_browser(&mut self, reply: &BrowserReply) -> Result<(), JsError> {
        self.require_active()?;
        self.execute(OcelJsCommand::CompleteBrowser { reply: reply.clone() }).map(|_| ())
    }

    pub fn eval_script_with_layout(&mut self, script: &str, node: Option<NodeId>,
        host: &mut impl FnMut(&[DomMutation], NodeId) -> Result<BrowserFrame, JsError>) -> Result<String, JsError> {
        self.require_active()?;
        self.execute_with_layout(OcelJsCommand::EvalScript { script: String::from(script), node }, Some(host))
    }

    pub fn pump_browser_with_layout(&mut self, frame: &BrowserFrame,
        host: &mut impl FnMut(&[DomMutation], NodeId) -> Result<BrowserFrame, JsError>) -> Result<(), JsError> {
        self.require_active()?;
        self.execute_with_layout(OcelJsCommand::PumpBrowser { frame: frame.clone() }, Some(host)).map(|_| ())
    }

    pub fn complete_browser_with_layout(&mut self, reply: &BrowserReply,
        host: &mut impl FnMut(&[DomMutation], NodeId) -> Result<BrowserFrame, JsError>) -> Result<(), JsError> {
        self.require_active()?;
        self.execute_with_layout(OcelJsCommand::CompleteBrowser { reply: reply.clone() }, Some(host)).map(|_| ())
    }

    pub fn dispatch_event_with_layout(&mut self, event: &DomEvent,
        host: &mut impl FnMut(&[DomMutation], NodeId) -> Result<BrowserFrame, JsError>) -> Result<(), JsError> {
        self.require_active()?;
        self.execute_with_layout(OcelJsCommand::DispatchEvent { event: event.clone() }, Some(host)).map(|_| ())
    }

    pub fn take_browser_requests(&mut self) -> Vec<BrowserRequest> {
        core::mem::take(&mut self.browser_requests)
    }

    pub fn next_browser_wakeup(&self) -> Option<u64> { self.browser_wakeup }

    fn require_active(&mut self) -> Result<(), JsError> {
        if !self.is_active() {
            self.invalidate();
            return Err(Self::error("No active synchronized JavaScript document context"));
        }
        Ok(())
    }

    fn invalidate(&mut self) {
        self.close_context();
        self.opened = false;
        self.document_ready = false;
        self.last_activity = None;
        self.lease = None;
        self.browser_requests.clear();
        self.browser_wakeup = None;
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
        let activity = self.lease.as_ref().and_then(Lease::now);
        let reply = self.active_lease()?.exchange(encoded, &mut recv_buf)
            .map_err(|message| JsError { message, line: 0 })
            .and_then(|len| postcard::from_bytes::<OcelJsReply>(&recv_buf[..len])
                .map_err(|_| Self::error("Malformed JavaScript service response")));
        match reply {
            Ok(OcelJsReply::Busy) => {
                self.invalidate();
                Err(Self::error("JavaScript engine busy: another live document owns the context"))
            }
            Ok(OcelJsReply::Error { message }) => {
                self.invalidate();
                Err(JsError { message, line: 0 })
            }
            Err(error) => {
                self.invalidate();
                Err(error)
            }
            Ok(reply) => {
                self.last_activity = activity;
                Ok(reply)
            }
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
        self.execute_with_layout(command, None)
    }

    fn execute_with_layout(&mut self, command: OcelJsCommand,
        mut host: Option<&mut dyn FnMut(&[DomMutation], NodeId) -> Result<BrowserFrame, JsError>>) -> Result<String, JsError> {
        if !self.mutations.is_empty() {
            return Err(Self::error("Apply pending JavaScript mutations before executing another command"));
        }
        let payload = dom_arena::ipc::encode_bounded(&command, OCEL_JS_MAX_TRANSFER)
            .map_err(|_| Self::error("JavaScript request exceeds 512 KiB transfer limit"))?;
        self.expect_ack(&OcelJsRequest::Begin { context: self.context, total: payload.len() })?;
        for (index, chunk) in payload.chunks(OCEL_JS_CHUNK_SIZE).enumerate() {
            self.expect_ack(&OcelJsRequest::Chunk { context: self.context,
                offset: index * OCEL_JS_CHUNK_SIZE, data: chunk.to_vec() })?;
        }
        let mut reply = self.exchange(&OcelJsRequest::Commit { context: self.context })?;
        let mut queries = 0;
        let mut query_mutations = 0usize;
        let mut query_bytes = 0usize;
        let mut bytes = Vec::new();
        let mut expected_total = None;
        loop {
            if matches!(reply, OcelJsReply::LayoutQueryChunk { .. }) {
                if !bytes.is_empty() { self.invalidate(); return Err(Self::error("Layout query interrupted a final response")); }
                queries += 1;
                if queries > 64 { self.invalidate(); return Err(Self::error("Native layout query limit exceeded")); }
                let (query_id, query) = self.receive_layout_query(reply)?;
                if query_id != queries { self.invalidate(); return Err(Self::error("Native layout query sequence mismatch")); }
                query_mutations = query_mutations.saturating_add(query.mutations.len());
                query_bytes = query_bytes.saturating_add(dom_arena::ipc::serialized_len_bounded(&query.mutations, dom_arena::OCEL_JS_MAX_MUTATION_BYTES)
                    .unwrap_or(usize::MAX));
                if query_mutations > dom_arena::OCEL_JS_MAX_MUTATIONS || query_bytes > dom_arena::OCEL_JS_MAX_MUTATION_BYTES {
                    self.invalidate(); return Err(Self::error("Native layout mutation budget exceeded"));
                }
                let result = match host.as_mut() {
                    Some(host) => host(&query.mutations, query.node),
                    None => {
                        self.mutations.extend(query.mutations.iter().cloned());
                        Err(Self::error("Synchronous native layout host is unavailable"))
                    }
                };
                let resume = match result {
                    Ok(frame) => dom_arena::BrowserLayoutReply::Frame(frame),
                    Err(error) => dom_arena::BrowserLayoutReply::Error(error.message),
                };
                let payload = dom_arena::ipc::encode_bounded(&resume, OCEL_JS_MAX_TRANSFER)
                    .map_err(|_| { self.invalidate(); Self::error("Native layout response exceeds transfer budget") })?;
                self.expect_ack(&OcelJsRequest::LayoutResumeBegin { context: self.context, query: query_id, total: payload.len() })?;
                for (index, chunk) in payload.chunks(OCEL_JS_CHUNK_SIZE).enumerate() {
                    self.expect_ack(&OcelJsRequest::LayoutResumeChunk { context: self.context, query: query_id,
                        offset: index * OCEL_JS_CHUNK_SIZE, data: chunk.to_vec() })?;
                }
                reply = self.exchange(&OcelJsRequest::LayoutResumeCommit { context: self.context, query: query_id })?;
                continue;
            }
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
        let (mutations, text, requests, wake_at) = match &response {
            OcelJsResponse::Success { mutations, result_repr, requests, wake_at } => (mutations, result_repr, requests, wake_at),
            OcelJsResponse::Error { mutations, message, requests, wake_at, .. } => (mutations, message, requests, wake_at),
        };
        let request_bytes = dom_arena::ipc::serialized_len_bounded(requests, dom_arena::OCEL_JS_MAX_BROWSER_REQUEST_BYTES)
            .and_then(|incoming| dom_arena::ipc::serialized_len_bounded(&self.browser_requests, dom_arena::OCEL_JS_MAX_BROWSER_REQUEST_BYTES)
                .and_then(|queued| if incoming.saturating_add(queued) <= dom_arena::OCEL_JS_MAX_BROWSER_REQUEST_BYTES {
                    Ok(incoming.saturating_add(queued))
                } else { Err(postcard::Error::SerializeBufferFull) }));
        if mutations.len().saturating_add(query_mutations) > dom_arena::OCEL_JS_MAX_MUTATIONS || text.len() > dom_arena::OCEL_JS_MAX_RESULT_BYTES
            || dom_arena::ipc::serialized_len_bounded(mutations, dom_arena::OCEL_JS_MAX_MUTATION_BYTES)
                .map_or(true, |bytes| bytes.saturating_add(query_bytes) > dom_arena::OCEL_JS_MAX_MUTATION_BYTES)
            || requests.len().saturating_add(self.browser_requests.len()) > dom_arena::OCEL_JS_MAX_BROWSER_REQUESTS
            || request_bytes.is_err() {
            self.invalidate();
            return Err(Self::error("JavaScript service exceeded output budgets; context closed"));
        }
        self.browser_wakeup = *wake_at;
        if !self.backend_named {
            ostd::io::println("[ocel] js backend: Tier 2 domain service (document-backed DOM required)");
            self.backend_named = true;
        }
        match response {
            OcelJsResponse::Success { mutations, result_repr, requests, .. } => {
                self.mutations.extend(mutations); self.browser_requests.extend(requests); Ok(result_repr)
            }
            OcelJsResponse::Error { message, line, mutations, requests, .. } => {
                self.mutations.extend(mutations); self.browser_requests.extend(requests); Err(JsError { message, line })
            }
        }
    }

    fn receive_layout_query(&mut self, mut reply: OcelJsReply) -> Result<(u32, dom_arena::BrowserLayoutQuery), JsError> {
        let mut bytes = Vec::new();
        let mut expected = None;
        let mut expected_query = None;
        loop {
            let OcelJsReply::LayoutQueryChunk { context, query, offset, total, data } = reply else {
                self.invalidate(); return Err(Self::error("Malformed native layout query chunk"));
            };
            if context != self.context || query == 0 || total == 0 || total > OCEL_JS_MAX_TRANSFER || offset != bytes.len()
                || data.is_empty() || data.len() > OCEL_JS_CHUNK_SIZE || offset.saturating_add(data.len()) > total
                || expected.is_some_and(|value| value != total) || expected_query.is_some_and(|value| value != query) {
                self.invalidate(); return Err(Self::error("Invalid native layout query sequence"));
            }
            expected = Some(total); expected_query = Some(query); bytes.extend(data);
            if bytes.len() == total {
                let query = postcard::from_bytes(&bytes).map_err(|_| { self.invalidate(); Self::error("Malformed native layout query") })?;
                return Ok((expected_query.unwrap(), query));
            }
            reply = self.exchange(&OcelJsRequest::ReadLayoutQuery { context: self.context, query, offset: bytes.len() })?;
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
    fn eval_script(&mut self, script: &str, node: Option<NodeId>) -> Result<String, JsError> { Tier2JsBridge::eval_script(self, script, node) }
    fn configure_browser(&mut self, environment: &BrowserEnvironment) -> Result<(), JsError> { Tier2JsBridge::configure_browser(self, environment) }
    fn pump_browser(&mut self, frame: &BrowserFrame) -> Result<(), JsError> { Tier2JsBridge::pump_browser(self, frame) }
    fn complete_browser(&mut self, reply: &BrowserReply) -> Result<(), JsError> { Tier2JsBridge::complete_browser(self, reply) }
    fn take_browser_requests(&mut self) -> Vec<BrowserRequest> { Tier2JsBridge::take_browser_requests(self) }
    fn next_browser_wakeup(&self) -> Option<u64> { self.browser_wakeup }
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
        self.invalidate();
        self.context = NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed) as u64;
        self.mutations.clear();
    }
}

impl<T: Transport> Tier2JsBridge<T> {
    fn close_context(&mut self) {
        if self.opened {
            self.opened = false;
            if let Some(lease) = self.lease.as_mut() {
                let mut send = [0u8; OCEL_JS_IPC_BUF_SIZE];
                let mut receive = [0u8; OCEL_JS_IPC_BUF_SIZE];
                if let Ok(bytes) = postcard::to_slice(&OcelJsRequest::CloseContext { context: self.context }, &mut send) {
                    let _ = lease.exchange(bytes, &mut receive);
                }
            }
        }
    }
}

impl<T: Transport> Drop for Tier2JsBridge<T> {
    fn drop(&mut self) {
        // Close before Lease::drop releases activation demand.
        self.close_context();
    }
}

#[cfg(test)]
mod tests;
