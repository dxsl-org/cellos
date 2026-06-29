//! Hypha `core` — the agent brain (P3: fs + sys + spawn tools).
//!
//! Spawns `llm-gateway`, `tool-fs`, `tool-sys`, and `tool-spawn`, then loops:
//! read a line from stdin (UART), keep the conversation in heap, run an agentic
//! turn (LLM + optional tool sub-loop), print the final reply.
//!
//! Tool routing by name prefix:
//! - fs tools  (read_file / write_file / list_dir) → tool-fs
//! - sys tools (list_cells / sys_info / lookup_service) → tool-sys
//! - spawn tools (spawn_cell / kill_cell) → tool-spawn

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use agent_proto::{AgentToolRequest, AgentToolResponse, LlmReply, LlmRequest, ToolCall};
use alloc::string::String;
use alloc::vec::Vec;
use ostd::input::request_focus;
use ostd::io::{print, println, stdin};
use ostd::syscall::{sys_exit, sys_recv, sys_send, sys_spawn_from_path, SyscallResult};

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![Send, Recv, RecvTimeout, Read, Log, SpawnFromPath, LookupService];

const GATEWAY_PATH: &str = "/bin/llm-gateway";
const TOOL_FS_PATH: &str = "/bin/tool-fs";
const TOOL_SYS_PATH: &str = "/bin/tool-sys";
const TOOL_SPAWN_PATH: &str = "/bin/tool-spawn";
// Prompt must fit one IPC message (os-gap G5: Grant for large prompts later).
const PROMPT_MAX: usize = 3500;
// Tool call agentic loop guard — prevents infinite tool chains.
const MAX_TOOL_ROUNDS: usize = 5;

const SYSTEM_PREAMBLE: &str = "\
system: You are Hypha, a helpful AI agent inside Cellos OS. \
Writable dirs: /data (persistent), /tmp (scratch). Read-only: /bin (binaries). \
When you need a tool, reply with ONLY this line: \
TOOL_CALL: {\"name\":\"TOOL\",\"args\":{ARGS_JSON}} \
File tools: read_file({\"path\":\"...\"}), write_file({\"path\":\"...\",\"content\":\"...\"}), list_dir({\"path\":\"...\"}). \
Sys tools: list_cells({}), sys_info({}), lookup_service({\"name\":\"vfs|net|input\"}). \
Spawn tools: spawn_cell({\"path\":\"/bin/...\"}), kill_cell({\"tid\":N}). \
After tool_result: incorporate it into your answer.\n";

/// Holds the tid for each tool cell. 0 means unavailable (failed to spawn).
struct Tools {
    fs: usize,
    sys: usize,
    spawn: usize,
}

impl Tools {
    /// Route a tool name to the cell tid that handles it.
    /// Returns 0 if no cell is available for this tool.
    fn route(&self, name: &str) -> usize {
        match name {
            "read_file" | "write_file" | "list_dir" => self.fs,
            "list_cells" | "sys_info" | "lookup_service" => self.sys,
            "spawn_cell" | "kill_cell" => self.spawn,
            _ => 0,
        }
    }
}

#[no_mangle]
pub fn main() {
    println("Hypha — Cellos AI agent (P3: fs+sys+spawn tools). Type 'exit' to quit.");

    let gw = match sys_spawn_from_path(GATEWAY_PATH) {
        SyscallResult::Ok(tid) => tid,
        _ => {
            println("[hypha] ERROR: cannot spawn llm-gateway");
            sys_exit(1);
        }
    };

    let tool_fs = match sys_spawn_from_path(TOOL_FS_PATH) {
        SyscallResult::Ok(tid) => {
            println("[hypha] tool-fs ready");
            tid
        }
        _ => {
            println("[hypha] WARN: tool-fs not found");
            0
        }
    };

    let tool_sys = match sys_spawn_from_path(TOOL_SYS_PATH) {
        SyscallResult::Ok(tid) => {
            println("[hypha] tool-sys ready");
            tid
        }
        _ => {
            println("[hypha] WARN: tool-sys not found");
            0
        }
    };

    let tool_spawn = match sys_spawn_from_path(TOOL_SPAWN_PATH) {
        SyscallResult::Ok(tid) => {
            println("[hypha] tool-spawn ready");
            tid
        }
        _ => {
            println("[hypha] WARN: tool-spawn not found");
            0
        }
    };

    let tools = Tools { fs: tool_fs, sys: tool_sys, spawn: tool_spawn };
    let mut conversation: Vec<(&'static str, String)> = Vec::new();
    let sin = stdin();

    // Register with the input service before entering the readline loop.
    // Retry a few times in case of a boot race where input service is still starting.
    for _ in 0..10 {
        if request_focus() { break; }
    }

    loop {
        print("\nyou> ");
        let mut line = String::new();
        if sin.read_line(&mut line).is_err() {
            break;
        }
        let user = line.trim();
        if user.is_empty() {
            continue;
        }
        if user == "exit" || user == "quit" {
            break;
        }

        conversation.push(("user", String::from(user)));
        let prompt = render_prompt(&conversation);

        match run_turn(gw, &tools, &prompt) {
            Ok(reply) => {
                print("hypha> ");
                println(reply.as_str());
                conversation.push(("assistant", reply));
            }
            Err(e) => {
                print("[hypha] error: ");
                println(e.as_str());
                // Drop the failed turn so it doesn't poison later context.
                conversation.pop();
            }
        }
    }

    println("[hypha] bye");
    sys_exit(0);
}

/// Run one agentic turn: call the LLM, dispatch any tool requests, and return
/// the final text reply. Loops up to `MAX_TOOL_ROUNDS` times to handle chained
/// tool calls. Tool interactions are appended to `working_prompt` in-place
/// (not stored in the permanent conversation — only the final reply is kept).
fn run_turn(gw: usize, tools: &Tools, prompt: &str) -> Result<String, String> {
    let mut working = String::from(prompt);
    for _ in 0..MAX_TOOL_ROUNDS {
        match ask(gw, &working)? {
            LlmReply::Text(t) => return Ok(t),
            LlmReply::ToolCalls(calls) => {
                for call in &calls {
                    print("[hypha] tool: ");
                    println(call.name.as_str());
                    let result = dispatch_tool(tools, call)?;
                    working.push_str("\ntool_call: ");
                    working.push_str(&call.name);
                    working.push(' ');
                    working.push_str(&call.args_json);
                    working.push_str("\ntool_result: ");
                    working.push_str(&result);
                }
                working.push_str("\nassistant: ");
                working = trim_front(working, PROMPT_MAX);
            }
            LlmReply::Error(e) => return Err(e),
        }
    }
    Err(String::from("[tool limit reached — too many sequential calls]"))
}

/// One IPC round-trip with the gateway: send `LlmRequest`, receive `LlmReply`.
fn ask(gw: usize, prompt: &str) -> Result<LlmReply, String> {
    let req = LlmRequest::Complete { prompt };
    let mut out = [0u8; 4096];
    let encoded = postcard::to_slice(&req, &mut out)
        .map_err(|_| String::from("prompt too large for one IPC message"))?;

    match sys_send(gw, encoded) {
        SyscallResult::Ok(_) => {}
        _ => return Err(String::from("send to gateway failed")),
    }

    let mut buf = [0u8; 4096];
    // Kernel `ipc_recv` ignores the mask parameter (os-gap G18: mask filtering not
    // yet enforced). Loop until we get a reply specifically from the gateway,
    // draining and discarding any input-service key events that arrive while the
    // LLM is thinking. take_from_bytes handles trailing zeros in the 4 KiB buffer.
    loop {
        match sys_recv(gw, &mut buf) {
            SyscallResult::Ok(sender) if sender == gw => {
                return match postcard::take_from_bytes::<LlmReply>(&buf) {
                    Ok((reply, _)) => Ok(reply),
                    Err(_) => Err(String::from("bad LlmReply encoding")),
                };
            }
            SyscallResult::Ok(_) => continue, // non-gw message (e.g. input event) — discard
            _ => return Err(String::from("no reply from gateway")),
        }
    }
}

/// Dispatch one tool call to the appropriate tool cell via `AgentToolRequest` IPC.
/// Routes by tool name: fs / sys / spawn tools go to their respective cells.
fn dispatch_tool(tools: &Tools, call: &ToolCall) -> Result<String, String> {
    let cell_tid = tools.route(&call.name);
    if cell_tid == 0 {
        return Err(alloc::format!("tool '{}' not available (cell not spawned)", call.name));
    }

    let req = AgentToolRequest::Invoke {
        name: &call.name,
        args_json: &call.args_json,
    };
    let mut out = [0u8; 4096];
    let encoded = postcard::to_slice(&req, &mut out)
        .map_err(|_| String::from("tool request too large"))?;

    match sys_send(cell_tid, encoded) {
        SyscallResult::Ok(_) => {}
        _ => return Err(alloc::format!("send to tool cell '{}' failed", call.name)),
    }

    let mut buf = [0u8; 4096];
    // Same drain-loop as ask(): kernel ignores mask, so discard non-tool messages.
    loop {
        match sys_recv(cell_tid, &mut buf) {
            SyscallResult::Ok(sender) if sender == cell_tid => {
                return match postcard::take_from_bytes::<AgentToolResponse>(&buf) {
                    Ok((AgentToolResponse::Ok { result_json }, _)) => Ok(result_json),
                    Ok((AgentToolResponse::Err { message }, _)) => Err(message),
                    Err(_) => Err(String::from("bad AgentToolResponse encoding")),
                };
            }
            SyscallResult::Ok(_) => continue,
            _ => return Err(alloc::format!("no reply from tool cell '{}'", call.name)),
        }
    }
}

/// Flatten the heap conversation into a single role-tagged transcript.
/// The system preamble is prepended so the LLM always knows about tools.
/// Trimmed from the front if it would exceed the one-message IPC budget.
fn render_prompt(conv: &[(&'static str, String)]) -> String {
    let mut s = String::from(SYSTEM_PREAMBLE);
    for (role, text) in conv {
        s.push_str(role);
        s.push_str(": ");
        s.push_str(text);
        s.push('\n');
    }
    s.push_str("assistant: ");
    trim_front(s, PROMPT_MAX)
}

/// Keep the tail of `s` within `max` bytes (drop oldest content), char-boundary safe.
fn trim_front(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    String::from(&s[start..])
}
