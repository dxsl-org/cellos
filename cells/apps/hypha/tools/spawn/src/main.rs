//! Hypha `tool-spawn` — cell lifecycle tool cell (P3).
//!
//! Handles [`AgentToolRequest::Invoke`] for cell lifecycle operations:
//! - `spawn_cell` — spawn a cell from a VFS path via [`sys_spawn_from_path`]
//! - `kill_cell`  — force-terminate a cell by tid via [`sys_force_exit`]
//!
//! Requires SpawnCap (`manifest spawn = true`). The kernel validates the path
//! at spawn time and rejects kill requests for system cells (block_io/network).

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use agent_proto::{AgentToolRequest, AgentToolResponse};
use alloc::string::String;
use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::runtime::CellRuntime;
use ostd::syscall::{sys_exit, sys_force_exit, sys_spawn_from_path, SyscallResult};

api::declare_manifest!(block_io = false, network = false, spawn = true);
// ForceExit is always-permitted (SpawnCap-gated at kernel dispatch, not allowlist).
// GrantAlloc: sys_spawn_from_path's VFS-Grant route (read_full_via_grant) needs
// GrantAlloc/Share/Free — the whole Grant family shares bit 39, so declaring
// one covers all six (Alloc/Share/Slice/Free/Register/Unregister).
api::declare_syscalls![Send, Recv, Log, SpawnFromPath, GrantAlloc];

#[no_mangle]
pub fn main() {
    println("[tool-spawn] ready");
    CellRuntime::new().no_heartbeat().run(|ctx, ev| match ev {
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            handle(ctx, sender_tid, &data);
        }
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    });
}

fn handle(ctx: &AppContext, sender: usize, data: &[u8]) {
    let reply = match postcard::from_bytes::<AgentToolRequest<'_>>(data) {
        Ok(AgentToolRequest::Invoke { name, args_json }) => dispatch(name, args_json),
        Err(_) => AgentToolResponse::Err {
            message: String::from("bad AgentToolRequest encoding"),
        },
    };
    let mut buf = [0u8; 4096];
    if let Ok(bytes) = postcard::to_slice(&reply, &mut buf) {
        let _ = ctx.send(sender, bytes);
    }
}

fn dispatch(name: &str, args_json: &str) -> AgentToolResponse {
    match name {
        "spawn_cell" => {
            let path = args_extract_str(args_json, "path").unwrap_or("/bin/nc");
            match sys_spawn_from_path(path) {
                SyscallResult::Ok(tid) => AgentToolResponse::Ok {
                    result_json: alloc::format!(
                        "{{\"spawned\":\"{}\",\"tid\":{}}}",
                        path,
                        tid
                    ),
                },
                _ => AgentToolResponse::Err {
                    message: alloc::format!("spawn_cell: '{}' not found or load failed", path),
                },
            }
        }

        "kill_cell" => {
            let tid = match args_extract_usize(args_json, "tid") {
                Some(t) => t,
                None => {
                    return AgentToolResponse::Err {
                        message: String::from("kill_cell: missing or invalid 'tid' arg"),
                    }
                }
            };
            match sys_force_exit(tid) {
                SyscallResult::Ok(_) => AgentToolResponse::Ok {
                    result_json: alloc::format!("{{\"killed\":true,\"tid\":{}}}", tid),
                },
                _ => AgentToolResponse::Err {
                    message: alloc::format!(
                        "kill_cell tid={}: failed (system cell, self, or not found)",
                        tid
                    ),
                },
            }
        }

        other => AgentToolResponse::Err {
            message: alloc::format!("tool-spawn: unknown tool '{}'", other),
        },
    }
}

fn args_extract_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let search = alloc::format!("\"{}\"", key);
    let mut idx = json.find(search.as_str())? + search.len();
    let bytes = json.as_bytes();
    while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t' | b':') {
        idx += 1;
    }
    if idx >= bytes.len() || bytes[idx] != b'"' {
        return None;
    }
    idx += 1;
    let start = idx;
    while idx < bytes.len() && bytes[idx] != b'"' {
        if bytes[idx] == b'\\' {
            idx += 1;
        }
        idx += 1;
    }
    Some(&json[start..idx])
}

fn args_extract_usize(json: &str, key: &str) -> Option<usize> {
    let search = alloc::format!("\"{}\"", key);
    let idx = json.find(search.as_str())? + search.len();
    let rest = json[idx..].trim_start_matches(|c: char| matches!(c, ' ' | '\t' | ':'));
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse::<usize>().ok()
}
