//! Hypha `tool-sys` — system introspection tool cell (P3).
//!
//! Handles [`AgentToolRequest::Invoke`] for read-only OS queries:
//! - `list_cells` — running cells via [`sys_get_procs`]
//! - `sys_info`   — static OS / arch string
//! - `lookup_service` — service name → provider tid via [`sys_lookup_service`]
//!
//! No SpawnCap required — all syscalls are open to any cell.

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use agent_proto::{AgentToolRequest, AgentToolResponse};
use alloc::string::String;
use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::runtime::CellRuntime;
use ostd::syscall::{sys_exit, sys_get_procs, sys_lookup_service};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, Log, GetProcs, LookupService];

#[no_mangle]
pub fn main() {
    println("[tool-sys] ready");
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
        "list_cells" => {
            let mut procs = [api::syscall::ProcessInfo::default(); 32];
            let count = match sys_get_procs(&mut procs) {
                Ok(n) => n,
                Err(_) => {
                    return AgentToolResponse::Err {
                        message: String::from("sys_get_procs failed"),
                    }
                }
            };

            let mut cells_json = String::new();
            for info in &procs[..count] {
                // Decode null-terminated name bytes.
                let end = info.name.iter().position(|&b| b == 0).unwrap_or(32);
                let cell_name = core::str::from_utf8(&info.name[..end]).unwrap_or("?");
                let state = match info.state {
                    0 => "ready",
                    1 => "running",
                    2 => "waiting",
                    _ => "dead",
                };
                if !cells_json.is_empty() {
                    cells_json.push(',');
                }
                cells_json.push_str(&alloc::format!(
                    "{{\"id\":{},\"name\":\"{}\",\"state\":\"{}\"}}",
                    info.id,
                    json_escape(cell_name),
                    state
                ));
            }
            AgentToolResponse::Ok {
                result_json: alloc::format!("{{\"cells\":[{}]}}", cells_json),
            }
        }

        "sys_info" => AgentToolResponse::Ok {
            result_json: String::from(
                "{\"os\":\"Cellos\",\"version\":\"v0.2.1-dev\",\
                 \"codename\":\"Mycelium\",\"arch\":\"riscv64\"}",
            ),
        },

        "lookup_service" => {
            let svc_name = args_extract_str(args_json, "name").unwrap_or("vfs");
            let sid: u16 = match svc_name {
                "vfs" | "VFS" => api::syscall::service::VFS,
                "net" | "NET" => api::syscall::service::NET,
                "input" | "INPUT" => api::syscall::service::INPUT,
                "config" | "CONFIG" => api::syscall::service::CONFIG,
                "compositor" | "COMPOSITOR" => api::syscall::service::COMPOSITOR,
                other => {
                    return AgentToolResponse::Err {
                        message: alloc::format!("unknown service name: {}", other),
                    }
                }
            };
            match sys_lookup_service(sid) {
                Some(tid) => AgentToolResponse::Ok {
                    result_json: alloc::format!("{{\"service\":\"{}\",\"tid\":{}}}", svc_name, tid),
                },
                None => AgentToolResponse::Err {
                    message: alloc::format!("service '{}' not currently registered", svc_name),
                },
            }
        }

        other => AgentToolResponse::Err {
            message: alloc::format!("tool-sys: unknown tool '{}'", other),
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

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}
