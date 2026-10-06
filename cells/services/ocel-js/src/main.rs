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
use ostd::syscall::{sys_recv, sys_send, sys_yield, SyscallResult};

// Untrusted-protection-class manifest: the kernel classifies this cell as a
// domain cell and admits it to Tier 2 (private page tables), containing any
// engine fault to this cell. Registration is NOT done here — an untrusted cell
// holds no SpawnCap, and the kernel refuses ordinary `RegisterService` without
// it. `init` spawns this cell and registers `service::OCEL_JS` on its behalf
// (cells/tools/init/src/boot.rs, `spawn_optional_services`).
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);
api::declare_syscalls![Log, Recv, Send, TryRecv, GetTime];

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[ocel-js] Starting Tier 2 JavaScript Engine Service...");

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
