// SPDX-License-Identifier: MIT
//! Synchronous native CSSOM queries keep the QuickJS C stack suspended while
//! the owner acknowledges/apply mutations and commits a real native layout.
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use dom_arena::{BrowserFrame, BrowserLayoutQuery, BrowserLayoutReply, DomMutation, NodeData, NodeId,
    OcelJsReply, OcelJsRequest, OCEL_JS_CHUNK_SIZE, OCEL_JS_IPC_BUF_SIZE, OCEL_JS_MAX_TRANSFER};
use super::{engine, ffi};

use core::sync::atomic::{AtomicUsize, Ordering};
static REPLY_OPERATION: AtomicUsize = AtomicUsize::new(0);
pub(super) fn set_reply_operation(operation: Option<usize>) { REPLY_OPERATION.store(operation.unwrap_or(0), Ordering::Relaxed); }
pub(super) fn reply_operation() -> Option<usize> {
    match REPLY_OPERATION.load(Ordering::Relaxed) { 0 => None, operation => Some(operation) }
}
#[derive(Default)]
pub(super) struct State {
    pub caller: usize,
    pub context: u64,
    pub operation: Option<usize>,
    pub queries: u32,
    pub mutations: usize,
    pub mutation_bytes: usize,
    pub failed: bool,
    pub unapplied: Vec<DomMutation>,
    #[cfg(test)]
    pub host: Option<fn(&BrowserLayoutQuery) -> Result<BrowserFrame, String>>,
}

pub(super) fn install(ctx: *mut ffi::JSContext, state: &mut Box<State>) {
    unsafe { ffi::vios_js_install_native_layout(ctx, (&mut **state as *mut State).cast(), Some(callback)); }
}

unsafe extern "C" fn callback(ctx: *mut ffi::JSContext, opaque: *mut core::ffi::c_void,
    node: u32, wire: *const core::ffi::c_char, length: usize) -> ffi::JSValue {
    let state = &mut *opaque.cast::<State>();
    let result = (|| -> Result<BrowserFrame, String> {
        if state.failed { return Err(String::from("Native layout session has failed")); }
        state.queries += 1;
        if state.queries > 64 { return Err(String::from("Synchronous native layout query limit exceeded")); }
        let text = core::str::from_utf8(core::slice::from_raw_parts(wire.cast::<u8>(), length))
            .map_err(|_| String::from("Invalid native layout mutation encoding"))?;
        let mutations = parse_mutations(text)?;
        let has_host = state.operation.is_some();
        #[cfg(test)]
        let has_host = has_host || state.host.is_some();
        if !has_host {
            state.unapplied.extend(mutations);
            return Err(String::from("Native layout host is unavailable"));
        }
        state.mutations = state.mutations.saturating_add(mutations.len());
        state.mutation_bytes = state.mutation_bytes.saturating_add(dom_arena::ipc::serialized_len_bounded(&mutations, dom_arena::OCEL_JS_MAX_MUTATION_BYTES)
            .map_err(|_| String::from("Native layout mutation byte budget exceeded"))?);
        if state.mutations > dom_arena::OCEL_JS_MAX_MUTATIONS || state.mutation_bytes > dom_arena::OCEL_JS_MAX_MUTATION_BYTES {
            return Err(String::from("Native layout mutation budget exceeded"));
        }
        let query = BrowserLayoutQuery { node: NodeId(node), mutations };
        #[cfg(test)]
        if let Some(host) = state.host { return host(&query); }
        exchange(state, query)
    })();
    match result {
        Ok(frame) => {
            let source = engine::frame_expression(&frame);
            let code = match alloc::ffi::CString::new(source) { Ok(code) => code, Err(_) => return throw(ctx, "Invalid native frame source") };
            ffi::JS_Eval(ctx, code.as_ptr(), code.as_bytes().len(), c"<native-layout>".as_ptr(), ffi::JS_EVAL_TYPE_GLOBAL)
        }
        Err(error) => throw(ctx, &error),
    }
}
unsafe fn throw(ctx: *mut ffi::JSContext, message: &str) -> ffi::JSValue {
    let text = alloc::ffi::CString::new(message.replace('\0', " ")).unwrap();
    ffi::vios_js_throw_error(ctx, text.as_ptr())
}

pub(super) fn parse_mutations(text: &str) -> Result<Vec<DomMutation>, String> {
    let mut out = Vec::new();
    for row in text.lines() {
        if out.len() >= dom_arena::OCEL_JS_MAX_MUTATIONS { return Err(String::from("Mutation count budget exceeded")); }
        let fields = row.split('\t').map(engine::decode_field).collect::<Option<Vec<_>>>()
            .ok_or_else(|| String::from("Invalid mutation encoding"))?;
        let field = |index| fields.get(index).map(String::as_str).ok_or_else(|| String::from("Missing mutation field"));
        let id = |index| field(index)?.parse::<u32>().map(NodeId).map_err(|_| String::from("Invalid mutation node"));
        let mutation = match field(0)? {
            "C" if fields.len() == 5 => DomMutation::CreateNode { node: id(1)?, data: match field(2)? {
                "1" => NodeData::Element { tag: field(3)?.to_string(), attributes: Vec::new() },
                "3" => NodeData::Text(field(4)?.to_string()), "8" => NodeData::Comment(field(4)?.to_string()),
                "11" => NodeData::DocumentFragment, _ => return Err(String::from("Invalid created node type")),
            } },
            "X" if fields.len() == 3 => DomMutation::SetText { node: id(1)?, text: field(2)?.to_string() },
            "A" if fields.len() == 4 => DomMutation::SetAttribute { node: id(1)?, key: field(2)?.to_string(), val: field(3)?.to_string() },
            "R" if fields.len() == 3 => DomMutation::RemoveAttribute { node: id(1)?, key: field(2)?.to_string() },
            "P" if fields.len() == 3 => DomMutation::AppendChild { parent: id(1)?, child: id(2)? },
            "I" if fields.len() == 4 => DomMutation::InsertBefore { parent: id(1)?, child: id(2)?, reference: id(3)? },
            "D" if fields.len() == 3 => DomMutation::RemoveChild { parent: id(1)?, child: id(2)? },
            "T" if fields.len() == 2 => DomMutation::SetDocumentTitle { title: field(1)?.to_string() },
            _ => return Err(String::from("Invalid mutation operation")),
        };
        out.push(mutation);
    }
    Ok(out)
}
fn send(operation: usize, reply: &OcelJsReply) -> Result<(), String> {
    let mut bytes = [0u8; OCEL_JS_IPC_BUF_SIZE];
    let encoded = postcard::to_slice(reply, &mut bytes).map_err(|_| String::from("Native layout frame overflow"))?;
    ostd::ipc::reply(operation, encoded).map_err(|_| String::from("Native layout operation was abandoned"))
}
fn chunk(state: &State, bytes: &[u8], offset: usize) -> Result<OcelJsReply, String> {
    if offset >= bytes.len() { return Err(String::from("Native layout offset out of bounds")); }
    let end = offset.saturating_add(OCEL_JS_CHUNK_SIZE).min(bytes.len());
    Ok(OcelJsReply::LayoutQueryChunk { context: state.context, query: state.queries, offset, total: bytes.len(), data: bytes[offset..end].to_vec() })
}
fn exchange(state: &mut State, query: BrowserLayoutQuery) -> Result<BrowserFrame, String> {
    let operation = state.operation.take().ok_or_else(|| String::from("Native layout host is unavailable"))?;
    let outgoing = dom_arena::ipc::encode_bounded(&query, OCEL_JS_MAX_TRANSFER).map_err(|_| String::from("Native layout query exceeds transfer budget"))?;
    if let Err(error) = send(operation, &chunk(state, &outgoing, 0)?) { state.failed = true; return Err(error); }
    set_reply_operation(None);
    let Some(started) = ostd::syscall::sys_get_scheduler_ticks() else { state.failed = true; return Err(String::from("Native layout clock unavailable")); };
    let mut incoming = Vec::new(); let mut expected = None;
    let mut receive = [0u8; OCEL_JS_IPC_BUF_SIZE];
    loop {
        let now = ostd::syscall::sys_get_scheduler_ticks();
        if now.is_none_or(|now| now.wrapping_sub(started) >= 500) { state.failed = true; return Err(String::from("Native layout owner resume timed out")); }
        let caller = match ostd::syscall::sys_try_recv(0, &mut receive) {
            ostd::syscall::SyscallResult::Ok(caller) if caller > 0 => caller,
            _ => { ostd::syscall::sys_yield(); continue; }
        };
        let Some(operation) = ostd::ipc::current() else { continue; };
        let decoded = postcard::from_bytes::<OcelJsRequest>(&receive);
        let rejected = OcelJsReply::Error { message: String::from("No matching suspended native layout query") };
        if caller != state.caller { let _ = send(operation, &rejected); continue; }
        let result = match decoded {
            Ok(OcelJsRequest::ReadLayoutQuery { context, query, offset }) if context == state.context && query == state.queries => chunk(state, &outgoing, offset),
            Ok(OcelJsRequest::LayoutResumeBegin { context, query, total }) if context == state.context && query == state.queries && total > 0 && total <= OCEL_JS_MAX_TRANSFER => {
                incoming.clear(); expected = Some(total); Ok(OcelJsReply::Ack)
            }
            Ok(OcelJsRequest::LayoutResumeChunk { context, query, offset, data }) if context == state.context && query == state.queries
                && expected.is_some() && offset == incoming.len() && !data.is_empty() && data.len() <= OCEL_JS_CHUNK_SIZE
                && offset.saturating_add(data.len()) <= expected.unwrap_or(0) => { incoming.extend(data); Ok(OcelJsReply::Ack) }
            Ok(OcelJsRequest::LayoutResumeCommit { context, query }) if context == state.context && query == state.queries && expected == Some(incoming.len()) => {
                state.operation = Some(operation);
                set_reply_operation(Some(operation));
                return match postcard::from_bytes::<BrowserLayoutReply>(&incoming) {
                    Ok(BrowserLayoutReply::Frame(frame)) => {
                        if frame.layout.iter().any(|row| ![row.x, row.y, row.width, row.height].iter().all(|n| n.is_finite()) || row.width < 0.0 || row.height < 0.0) {
                            state.failed = true; Err(String::from("Invalid native layout geometry"))
                        } else { Ok(frame) }
                    }
                    Ok(BrowserLayoutReply::Error(message)) => Err(message),
                    Err(_) => { state.failed = true; Err(String::from("Malformed native layout resume")) },
                };
            }
            _ => Ok(rejected),
        };
        let response = result.unwrap_or_else(|message| OcelJsReply::Error { message });
        if send(operation, &response).is_err() { state.failed = true; return Err(String::from("Native layout resume operation was abandoned")); }
    }
}
