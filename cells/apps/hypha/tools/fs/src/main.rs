//! Hypha `tool-fs` — filesystem tool cell (P2).
//!
//! Receives [`AgentToolRequest::Invoke`] from the `core` that spawned it,
//! dispatches to [`ostd::clients::VfsClient`] (VFS service via IPC — no
//! `block_io` cap needed), and replies [`AgentToolResponse::Ok`] or
//! [`AgentToolResponse::Err`].
//!
//! Supported tools, every path confined to [`workspace::WORKSPACE_ROOT`]:
//! - `read_file`  `{"path":"..."}` → `{"content":"..."}`
//! - `write_file` `{"path":"...","content":"..."}` → `{"ok":true}`
//! - `list_dir`   `{"path":"..."}` → `{"files":["a","b",...]}`
//!
//! Requests from any sender other than the owner named in the spawn argv are
//! dropped unanswered ([`tool_owner`]); an unbound instance exits at once.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use agent_proto::{AgentToolRequest, AgentToolResponse};
use alloc::string::String;
use hypha_tool_fs::{tool_owner, workspace};
use ostd::app::{AppContext, AppEvent};
use ostd::clients::VfsClient;
use ostd::io::println;
use ostd::runtime::CellRuntime;
use ostd::syscall::sys_exit;

abi::declare_manifest!(block_io = false, network = false, spawn = false);
// StateRestore: reads the spawn argv that names the owning core.
abi::declare_syscalls![
    Send,
    SendGather,
    Recv,
    Log,
    LookupServiceBound,
    VfsMutate,
    StateRestore
];

ostd::cell_main!(cell_main);

const READ_FILE_MAX_BYTES: usize = 64 * 1024;
const READ_FILE_RESULT_CHAR_LIMIT: usize = 2048;

fn cell_main() {
    let Some(owner) = tool_owner::parse_owner(&ostd::args()) else {
        println("[tool-fs] refusing to start: no owner in spawn argv");
        sys_exit(1);
    };
    // The workspace may not exist on a fresh disk; an existing one is not an
    // error, and any other failure resurfaces on the first write.
    let _ = VfsClient::new().mkdir(workspace::WORKSPACE_ROOT);
    println("[tool-fs] ready");
    CellRuntime::new().no_heartbeat().run(|ctx, ev| match ev {
        // Any other sender falls through unanswered, not refused: a reply to an
        // arbitrary sender would let it stall this cell on a send it never receives.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data }
            if tool_owner::admits(owner, sender_tid) =>
        {
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
        let _ = ctx.reply(sender, bytes);
    }
}

/// The confined `path` argument, or the refusal to report.
fn workspace_path<'a>(tool: &str, args_json: &'a str) -> Result<&'a str, AgentToolResponse> {
    let raw = args_extract_str(args_json, "path").ok_or_else(|| AgentToolResponse::Err {
        message: alloc::format!("{}: missing 'path' arg", tool),
    })?;
    workspace::confine(raw).map_err(|why| AgentToolResponse::Err {
        message: alloc::format!(
            "{} {}: refused ({:?}); paths must be under {}",
            tool,
            raw,
            why,
            workspace::WORKSPACE_ROOT
        ),
    })
}

fn dispatch(name: &str, args_json: &str) -> AgentToolResponse {
    let path = match name {
        "read_file" | "write_file" | "list_dir" => match workspace_path(name, args_json) {
            Ok(path) => path,
            Err(refusal) => return refusal,
        },
        other => {
            return AgentToolResponse::Err {
                message: alloc::format!("unknown tool: {}", other),
            }
        }
    };
    match name {
        "read_file" => {
            let mut vfs = VfsClient::new();
            match vfs.read_file_bounded(path, READ_FILE_MAX_BYTES) {
                Ok(bytes) => AgentToolResponse::Ok {
                    result_json: read_file_result_json(&bytes),
                },
                Err(e) => AgentToolResponse::Err {
                    message: alloc::format!("read_file {}: {:?}", path, e),
                },
            }
        }
        "write_file" => {
            let content = args_extract_str(args_json, "content").unwrap_or("");
            let mut vfs = VfsClient::new();
            match vfs.write_file(path, content.as_bytes()) {
                Ok(()) => AgentToolResponse::Ok {
                    result_json: String::from("{\"ok\":true}"),
                },
                Err(e) => AgentToolResponse::Err {
                    message: alloc::format!("write_file {}: {:?}", path, e),
                },
            }
        }
        // "list_dir": the only name the guard above lets through besides these two.
        _ => {
            let mut vfs = VfsClient::new();
            match vfs.list_dir(path) {
                Ok(raw) => {
                    // VFS list format: "d:name\n" or "f:name\n" per entry.
                    // Strip the two-byte type prefix before building the JSON array.
                    let text = core::str::from_utf8(&raw).unwrap_or("");
                    let entries_json: String = text.split('\n').filter(|e| e.len() > 2).fold(
                        String::new(),
                        |mut acc, entry| {
                            // strip leading "d:" / "f:" type prefix
                            let name = if entry.starts_with("d:") || entry.starts_with("f:") {
                                &entry[2..]
                            } else {
                                entry
                            };
                            if !acc.is_empty() {
                                acc.push(',');
                            }
                            acc.push('"');
                            acc.push_str(&json_escape(name));
                            acc.push('"');
                            acc
                        },
                    );
                    AgentToolResponse::Ok {
                        result_json: alloc::format!("{{\"files\":[{}]}}", entries_json),
                    }
                }
                Err(e) => AgentToolResponse::Err {
                    message: alloc::format!("list_dir {}: {:?}", path, e),
                },
            }
        }
    }
}

/// Extract a plain string field from a simple JSON args object.
/// Handles basic `\"` escaping; does not decode other escape sequences.
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
    idx += 1; // skip opening quote
    let start = idx;
    while idx < bytes.len() && bytes[idx] != b'"' {
        if bytes[idx] == b'\\' {
            idx += 1; // skip escaped char
        }
        idx += 1;
    }
    Some(&json[start..idx])
}

fn read_file_result_json(bytes: &[u8]) -> String {
    // Keep the previous 64 KiB VFS ceiling explicit while preserving the
    // existing 2048-char response truncation and binary-data fallback.
    let text = core::str::from_utf8(bytes).unwrap_or("[binary data]");
    let truncated: String = text.chars().take(READ_FILE_RESULT_CHAR_LIMIT).collect();
    json_obj_str("content", &truncated)
}

/// Wrap a single key/value pair into a JSON object string: `{"key":"value"}`.
fn json_obj_str(key: &str, value: &str) -> String {
    alloc::format!("{{\"{}\":\"{}\"}}", json_escape(key), json_escape(value))
}

/// Escape special characters for use inside a JSON string literal.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Format control characters as \uXXXX
                let code = c as u32;
                out.push('\\');
                out.push('u');
                for shift in [12u32, 8, 4, 0] {
                    let nibble = (code >> shift) & 0xF;
                    out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                }
            }
            c => out.push(c),
        }
    }
    out
}
