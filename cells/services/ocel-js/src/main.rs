// SPDX-License-Identifier: MIT
//! Ocel Tier 2 JavaScript Service Cell.
//!
//! Hosts JavaScript execution in a hardware MMU-isolated Paged Domain.
//! Communicates with Tier 1 Ocel via typed IPC (Spec 17), accepting scripts/events
//! and returning batched DOM mutations.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

mod engine;

use dom_arena::{JsContext, JsEngine, OcelJsRequest, OcelJsResponse, OCEL_JS_IPC_BUF_SIZE};
use engine::OcelJsServiceEngine;
use ostd::syscall::{sys_recv, sys_register_service, sys_send, sys_yield, SyscallResult};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Log, RegisterService, Recv, Send, TryRecv, GetTime];

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[ocel-js] Starting Tier 2 JavaScript Engine Service...");

    // Register service in the kernel service registry (service::OCEL_JS = 16)
    match sys_register_service(api::syscall::service::OCEL_JS, 0) {
        SyscallResult::Ok(_) => {
            ostd::io::println("[ocel-js] Registered as service::OCEL_JS (id=16)");
        }
        SyscallResult::Err(_) => {
            ostd::io::println(
                "[ocel-js] WARN: Failed to register service::OCEL_JS (already registered?)",
            );
        }
    }

    let mut engine = OcelJsServiceEngine::new();
    let mut ctx = engine.create_context();

    let mut recv_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];
    let mut send_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];

    ostd::io::println("[ocel-js] Ready to process scripts and DOM events.");

    loop {
        // Spec 17 §2: sys_recv(0) is appropriate for a service's main dispatch loop
        match sys_recv(0, &mut recv_buf) {
            SyscallResult::Ok(caller_tid) if caller_tid > 0 => {
                let response = match postcard::from_bytes::<OcelJsRequest>(&recv_buf) {
                    Ok(OcelJsRequest::Eval { script }) => match ctx.eval(&script) {
                        Ok(res) => {
                            let mutations = ctx.take_mutations();
                            OcelJsResponse::Success {
                                mutations,
                                result_repr: res,
                            }
                        }
                        Err(e) => OcelJsResponse::Error {
                            message: e.message,
                            line: e.line,
                        },
                    },
                    Ok(OcelJsRequest::DispatchEvent { event }) => {
                        match ctx.dispatch_event(&event) {
                            Ok(_) => {
                                let mutations = ctx.take_mutations();
                                OcelJsResponse::Success {
                                    mutations,
                                    result_repr: alloc::string::String::from("event_dispatched"),
                                }
                            }
                            Err(e) => OcelJsResponse::Error {
                                message: e.message,
                                line: e.line,
                            },
                        }
                    }
                    Ok(OcelJsRequest::ResetContext) => {
                        ctx.reset();
                        OcelJsResponse::Success {
                            mutations: alloc::vec::Vec::new(),
                            result_repr: alloc::string::String::from("context_reset"),
                        }
                    }
                    Err(_) => OcelJsResponse::Error {
                        message: alloc::string::String::from("Malformed OcelJsRequest IPC payload"),
                        line: 0,
                    },
                };

                if let Ok(encoded) = postcard::to_slice(&response, &mut send_buf) {
                    let _ = sys_send(caller_tid, encoded);
                }
            }
            _ => {
                sys_yield();
            }
        }
    }
}
