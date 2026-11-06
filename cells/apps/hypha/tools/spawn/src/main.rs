//! Hypha `tool-spawn` — cell lifecycle tool cell (P3).
//!
//! Handles [`AgentToolRequest::Invoke`] from the `core` that spawned it:
//! - `spawn_cell` — spawn one of [`spawn_policy::SPAWN_ALLOWLIST`] via [`sys_spawn_from_path`]
//! - `kill_cell`  — force-terminate a cell this instance spawned, via [`sys_force_exit`]
//!
//! Requires SpawnCap (`manifest spawn = true`), which is why every request from
//! a sender other than the owner named in the spawn argv is dropped unanswered
//! ([`tool_owner`]) and an unbound instance exits at once.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use agent_proto::{AgentToolRequest, AgentToolResponse};
use alloc::string::String;
use hypha_tool_spawn::spawn_policy::{self, SpawnedCells};
use hypha_tool_spawn::tool_owner;
use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::runtime::CellRuntime;
use ostd::syscall::{sys_exit, sys_force_exit, sys_spawn_from_path, SyscallResult};

api::declare_manifest!(block_io = false, network = false, spawn = true);
// ForceExit is always-permitted (SpawnCap-gated at kernel dispatch, not allowlist).
// GrantAlloc: sys_spawn_from_path's VFS-Grant route (read_full_via_grant) needs
// GrantAlloc/Share/Free — the whole Grant family shares bit 39, so declaring
// one covers all six (Alloc/Share/Slice/Free/Register/Unregister).
// StateRestore: reads the spawn argv that names the owning core.
api::declare_syscalls![Send, Recv, Log, SpawnFromPath, GrantAlloc, StateRestore];

ostd::cell_main!(cell_main);

fn cell_main() {
    let Some(owner) = tool_owner::parse_owner(&ostd::args()) else {
        println("[tool-spawn] refusing to start: no owner in spawn argv");
        sys_exit(1);
    };
    let mut spawned = SpawnedCells::new();
    println("[tool-spawn] ready");
    CellRuntime::new().no_heartbeat().run(|ctx, ev| match ev {
        // Any other sender falls through unanswered, not refused: a reply to an
        // arbitrary sender would let it stall this cell on a send it never receives.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data }
            if tool_owner::admits(owner, sender_tid) =>
        {
            handle(ctx, &mut spawned, sender_tid, &data);
        }
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    });
}

fn handle(ctx: &AppContext, spawned: &mut SpawnedCells, sender: usize, data: &[u8]) {
    let reply = match postcard::from_bytes::<AgentToolRequest<'_>>(data) {
        Ok(AgentToolRequest::Invoke { name, args_json }) => dispatch(spawned, name, args_json),
        Err(_) => AgentToolResponse::Err {
            message: String::from("bad AgentToolRequest encoding"),
        },
    };
    let mut buf = [0u8; 4096];
    if let Ok(bytes) = postcard::to_slice(&reply, &mut buf) {
        let _ = ctx.send(sender, bytes);
    }
}

fn err(message: String) -> AgentToolResponse {
    AgentToolResponse::Err { message }
}

fn dispatch(spawned: &mut SpawnedCells, name: &str, args_json: &str) -> AgentToolResponse {
    match name {
        "spawn_cell" => {
            let Some(path) = args_extract_str(args_json, "path") else {
                return err(String::from("spawn_cell: missing 'path' arg"));
            };
            if !spawn_policy::spawn_allowed(path) {
                return err(alloc::format!(
                    "spawn_cell: '{}' refused; allowed: {:?}",
                    path,
                    spawn_policy::SPAWN_ALLOWLIST
                ));
            }
            if spawned.is_full() {
                return err(String::from(
                    "spawn_cell: session limit reached; kill_cell one first",
                ));
            }
            match sys_spawn_from_path(path) {
                SyscallResult::Ok(tid) => {
                    spawned.record(tid);
                    AgentToolResponse::Ok {
                        result_json: alloc::format!("{{\"spawned\":\"{}\",\"tid\":{}}}", path, tid),
                    }
                }
                _ => err(alloc::format!(
                    "spawn_cell: '{}' not found or load failed",
                    path
                )),
            }
        }

        "kill_cell" => {
            let Some(tid) = args_extract_usize(args_json, "tid") else {
                return err(String::from("kill_cell: missing or invalid 'tid' arg"));
            };
            if !spawned.contains(tid) {
                return err(alloc::format!(
                    "kill_cell tid={}: refused; not spawned by spawn_cell in this session",
                    tid
                ));
            }
            // Untracked either way: a failed force-exit means the cell already
            // exited, and its tid is never re-issued, so keeping it protects nothing.
            spawned.release(tid);
            match sys_force_exit(tid) {
                SyscallResult::Ok(_) => AgentToolResponse::Ok {
                    result_json: alloc::format!("{{\"killed\":true,\"tid\":{}}}", tid),
                },
                _ => err(alloc::format!(
                    "kill_cell tid={}: failed (already exited)",
                    tid
                )),
            }
        }

        other => err(alloc::format!("tool-spawn: unknown tool '{}'", other)),
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
    let rest = json[idx..].trim_start_matches([' ', '\t', ':']);
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse::<usize>().ok()
}
