// SPDX-License-Identifier: MIT
//! Ocel's opt-in Tier 2 script engine: vendored QuickJS.
//!
//! Same cell contract as `cells/services/ocel-js` — it parks on IPC, registers
//! nothing itself (`init` registers `service::OCEL_JS` for whichever engine
//! cell the image ships), and answers `OcelJsRequest` frames with batched
//! `DomMutation`s. The difference is the engine behind the trait: real
//! JavaScript (control flow, functions, closures, the standard library, regexp
//! and Unicode tables) instead of `ocel-js`'s line-oriented statement matcher.
//!
//! It is a **sibling** of `ocel-js`, not a replacement: an image ships one or
//! the other, `init` prefers this one when present, and the viewer's lookup of
//! `service::OCEL_JS` cannot tell which engine answered except by the identity
//! line this cell prints at start-up.

#![no_std]
#![no_main]

extern crate alloc;
extern crate api;
extern crate ostd;

#[cfg(not(qjs_c_unavailable))]
use dom_arena::{JsContext, JsEngine, OcelJsRequest, OcelJsResponse, OCEL_JS_IPC_BUF_SIZE};
#[cfg(not(qjs_c_unavailable))]
use ostd::syscall::{sys_recv, sys_send, sys_yield, SyscallResult};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);
api::declare_syscalls![Log, Recv, Send, TryRecv, GetTime];

// The engine's translation units, the prelude and the engine state together
// need more than the 1 MiB default cell heap: QuickJS compiles every function
// it evaluates into bytecode, and the prelude itself is ~4 KiB of source.
#[cfg(not(qjs_c_unavailable))]
ostd::declare_custom_heap!(1024 * 1024);

#[cfg(not(qjs_c_unavailable))]
mod engine;
#[cfg(not(qjs_c_unavailable))]
mod ffi;

#[cfg(not(qjs_c_unavailable))]
ostd::cell_main!(cell_main);

/// Stub entry point for targets the engine cannot be built for (see build.rs):
/// the cell still links, and says why it has no engine.
#[cfg(qjs_c_unavailable)]
#[no_mangle]
extern "C" fn main() -> usize {
    ostd::io::println(
        "[ocel-quickjs] engine unavailable: this target has no Tier-A C ABI or no ELF C compiler.",
    );
    1
}

#[cfg(not(qjs_c_unavailable))]
fn cell_main() {
    init_custom_heap();
    ostd::io::print("[ocel-quickjs] ");
    ostd::io::print(&engine::engine_identity());
    ostd::io::println(" engine ready (Tier 2)");

    let mut engine = engine::QuickJsEngine::new();
    let mut ctx = engine.create_context();

    // Start-up self-check: one evaluation that exercises the parts of the
    // engine a stub cannot fake — statement control flow, closures, a Unicode
    // regexp (libregexp/libunicode tables), `Array.prototype.map`, and number
    // formatting. Its exact output is the marker the QEMU lane asserts, so a
    // build that compiles but cannot execute is caught before any document.
    const SELF_CHECK: &str = r#"(function () {
        var sum = 0;
        for (var i = 1; i <= 10; i++) { sum += i; }
        var doubled = [1, 2, 3].map(function (x) { return x * 2; }).join(",");
        var accent = /^[\u00e1\u00e9]+$/.test("\u00e1\u00e9\u00e1");
        return sum + ":" + doubled + ":" + (accent ? "accent" : "no-accent") + ":" + (0.1 + 0.2).toFixed(2);
    })()"#;
    match ctx.eval(SELF_CHECK) {
        Ok(result) => {
            ostd::io::print("[ocel-quickjs] self-check ");
            ostd::io::println(&result);
        }
        Err(e) => {
            ostd::io::print("[ocel-quickjs] self-check FAILED: ");
            ostd::io::println(&e.message);
        }
    }
    for line in ctx.take_logs() {
        ostd::io::println(&line);
    }
    ctx.reset();

    let mut recv_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];
    let mut send_buf = [0u8; OCEL_JS_IPC_BUF_SIZE];

    loop {
        match sys_recv(0, &mut recv_buf) {
            SyscallResult::Ok(caller_tid) if caller_tid > 0 => {
                let response = match postcard::from_bytes::<OcelJsRequest>(&recv_buf) {
                    Ok(OcelJsRequest::Eval { script }) => match ctx.eval(&script) {
                        Ok(result_repr) => OcelJsResponse::Success {
                            mutations: ctx.take_mutations(),
                            result_repr,
                        },
                        Err(e) => OcelJsResponse::Error {
                            message: e.message,
                            line: e.line,
                        },
                    },
                    Ok(OcelJsRequest::DispatchEvent { event }) => {
                        match ctx.dispatch_event(&event) {
                            Ok(_) => OcelJsResponse::Success {
                                mutations: ctx.take_mutations(),
                                result_repr: alloc::string::String::from("event_dispatched"),
                            },
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
                // Script output (`console.log`) and engine diagnostics go to the
                // cell log, not into the reply: the viewer's IPC payload is the
                // mutation batch, and the log is where a reader expects to find
                // a script's own words.
                for line in ctx.take_logs() {
                    ostd::io::println(&line);
                }
            }
            _ => {
                sys_yield();
            }
        }
    }
}
