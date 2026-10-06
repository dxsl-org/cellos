// SPDX-License-Identifier: MIT
//! Tier 2 JavaScript IPC Bridge for Ocel.
//!
//! Gosub-style abstraction connecting Tier 1 Ocel with the Tier 2 `ocel-js` Service.
//! Implements transparent fallback: if `ocel-js` is active in Tier 2, scripts run
//! in the hardware MMU-isolated domain; if absent, falls back to the in-process engine.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use dom_arena::{
    DomEvent, DomMutation, JsContext, JsEngine, JsError, OcelJsRequest, OcelJsResponse,
    OCEL_JS_IPC_BUF_SIZE,
};
use ostd::syscall::{sys_lookup_service, sys_recv, sys_send, sys_yield, SyscallResult};

use crate::js::runtime::SimpleJsRuntime;

pub struct Tier2JsBridge {
    fallback: SimpleJsRuntime,
    cached_js_tid: Option<usize>,
    mutations: Vec<DomMutation>,
    /// One line per session naming which engine actually serves the document.
    backend_named: bool,
}

impl Default for Tier2JsBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl Tier2JsBridge {
    pub fn new() -> Self {
        Self {
            fallback: SimpleJsRuntime::new(),
            cached_js_tid: None,
            mutations: Vec::new(),
            backend_named: false,
        }
    }

    /// Resolves the Tier 2 `ocel-js` service TID.
    ///
    /// `init` spawns and registers the service during boot, but Ocel may start
    /// while that spawn is still settling, so a miss retries briefly instead of
    /// latching the in-process fallback for the rest of the session.
    fn get_js_service_tid(&mut self) -> Option<usize> {
        if let Some(tid) = self.cached_js_tid {
            return Some(tid);
        }
        for _ in 0..200 {
            if let Some(tid) = sys_lookup_service(api::syscall::service::OCEL_JS) {
                self.cached_js_tid = Some(tid);
                return Some(tid);
            }
            sys_yield();
        }
        None
    }

    /// Name the engine that served the script, once per session.
    fn note_backend(&mut self, served_by_tier2: bool) {
        if self.backend_named {
            return;
        }
        self.backend_named = true;
        if served_by_tier2 {
            ostd::io::println("[ocel] js backend: Tier 2 domain service");
        } else {
            ostd::io::println("[ocel] js backend: in-process fallback (no OCEL_JS service)");
        }
    }

    /// Sends an IPC request to Tier 2 `ocel-js` conforming to Spec 17 §2 (masked recv).
    fn send_ipc_request(&mut self, request: &OcelJsRequest) -> Option<OcelJsResponse> {
        let js_tid = self.get_js_service_tid()?;

        let mut send_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];
        let mut recv_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];

        let encoded = postcard::to_slice(request, &mut send_buf).ok()?;

        // Send request to ocel-js service
        if matches!(sys_send(js_tid, encoded), SyscallResult::Err(_)) {
            // Invalidate cache on send failure
            self.cached_js_tid = None;
            return None;
        }

        // Spec 17 §2: the receive must be masked to the service TID, and a
        // masked receive can still surface another task's death notice — so the
        // reply is accepted only when the kernel names `js_tid` as the sender.
        // (The shared `ostd::ipc::recv_from` helper enforces the same rule; this
        // cell keeps the check local so the engine cell does not depend on an
        // API outside this change.)
        match sys_recv(js_tid, &mut recv_buf) {
            SyscallResult::Ok(sender) if sender == js_tid => {
                postcard::from_bytes::<OcelJsResponse>(&recv_buf).ok()
            }
            _ => {
                self.cached_js_tid = None;
                None
            }
        }
    }
}

impl JsEngine for Tier2JsBridge {
    type Context = Tier2JsBridge;

    fn create_context(&mut self) -> Self::Context {
        Self::new()
    }
}

impl JsContext for Tier2JsBridge {
    fn eval(&mut self, script: &str) -> Result<String, JsError> {
        // 1. Try sending to Tier 2 ocel-js service
        let req = OcelJsRequest::Eval {
            script: String::from(script),
        };

        if let Some(resp) = self.send_ipc_request(&req) {
            self.note_backend(true);
            match resp {
                OcelJsResponse::Success {
                    mutations,
                    result_repr,
                } => {
                    self.mutations.extend(mutations);
                    return Ok(result_repr);
                }
                OcelJsResponse::Error { message, line } => {
                    return Err(JsError { message, line });
                }
            }
        }

        // 2. Fallback to in-process SimpleJsRuntime
        self.note_backend(false);
        use crate::js::engine::JsEngine as OldJsEngine;
        match self.fallback.eval(script) {
            Ok(val) => {
                // If title changed in fallback, emulate mutation
                if let Some(crate::js::engine::JsValue::String(title)) =
                    self.fallback.get_global("document.title")
                {
                    self.mutations.push(DomMutation::SetDocumentTitle { title });
                }
                Ok(val.to_string_repr())
            }
            Err(e) => Err(JsError {
                message: e.message,
                line: e.line,
            }),
        }
    }

    fn dispatch_event(&mut self, event: &DomEvent) -> Result<(), JsError> {
        let req = OcelJsRequest::DispatchEvent {
            event: event.clone(),
        };

        if let Some(resp) = self.send_ipc_request(&req) {
            match resp {
                OcelJsResponse::Success { mutations, .. } => {
                    self.mutations.extend(mutations);
                    return Ok(());
                }
                OcelJsResponse::Error { message, line } => {
                    return Err(JsError { message, line });
                }
            }
        }

        Ok(())
    }

    fn take_mutations(&mut self) -> Vec<DomMutation> {
        core::mem::take(&mut self.mutations)
    }

    fn reset(&mut self) {
        self.mutations.clear();
        let _ = self.send_ipc_request(&OcelJsRequest::ResetContext);
    }
}
