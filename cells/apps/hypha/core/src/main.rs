//! Hypha `core` — the agent brain (P3: fs + sys + spawn tools).
//!
//! Spawns `llm-gateway`, `tool-sys`, `tool-fs`, and `tool-spawn`, then loops:
//! read a line from stdin (UART), keep the conversation in heap, run an agentic
//! turn (LLM + optional tool sub-loop), print the final reply.
//!
//! Tool routing by name prefix:
//! - fs tools  (read_file / write_file / list_dir) → tool-fs
//! - sys tools (list_cells / sys_info / lookup_service) → tool-sys
//! - spawn tools (spawn_cell / kill_cell) → tool-spawn
//!
//! LLM output is untrusted: side-effecting calls wait for the operator's `y`
//! ([`tool_gate`]), and `tool-fs`/`tool-spawn` are spawned bound to this core so
//! no other cell can drive them ([`tool_owner`]).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use agent_proto::{AgentToolRequest, AgentToolResponse, LlmReply, LlmRequest, ToolCall};
use alloc::string::String;
use alloc::vec::Vec;
use hypha_core::preamble::system_preamble;
use hypha_core::{tool_gate, tool_owner};
use ostd::input::request_focus;
use ostd::io::{print, println, stdin};
use ostd::syscall::{
    sys_exit, sys_recv, sys_send, sys_spawn_from_path, sys_spawn_from_path_raw, SyscallResult,
};

api::declare_manifest!(block_io = false, network = false, spawn = true);
// GrantAlloc: sys_spawn_from_path's VFS-Grant route (read_full_via_grant) needs
// GrantAlloc/Share/Free — the whole Grant family shares bit 39, so declaring
// one covers all six (Alloc/Share/Slice/Free/Register/Unregister).
// StateStash: publishes the owner argv each bound tool cell is spawned with.
api::declare_syscalls![
    Send,
    Recv,
    RecvTimeout,
    Read,
    Log,
    SpawnFromPath,
    LookupService,
    GrantAlloc,
    StateStash
];

const GATEWAY_PATH: &str = "/bin/llm-gateway";
const TOOL_FS_PATH: &str = "/bin/tool-fs";
const TOOL_SYS_PATH: &str = "/bin/tool-sys";
const TOOL_SPAWN_PATH: &str = "/bin/tool-spawn";
// Prompt must fit one IPC message (os-gap G5: Grant for large prompts later).
const PROMPT_MAX: usize = 3500;
// Tool call agentic loop guard — prevents infinite tool chains.
const MAX_TOOL_ROUNDS: usize = 5;

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

ostd::cell_main!(cell_main);

fn cell_main() {
    println("Hypha — Cellos AI agent (P3: fs+sys+spawn tools). Type 'exit' to quit.");

    let gw = match sys_spawn_from_path(GATEWAY_PATH) {
        SyscallResult::Ok(tid) => tid,
        _ => {
            println("[hypha] ERROR: cannot spawn llm-gateway");
            sys_exit(1);
        }
    };

    let tool_sys = spawn_tool(TOOL_SYS_PATH, "tool-sys", &[]);
    // Bind the authority-holding tools to this core. The kernel offers no
    // "own tid" query; tool-sys echoes the kernel-stamped sender instead.
    let owner_argv = (tool_sys != 0)
        .then(|| invoke(tool_sys, "caller_tid", "{}").ok())
        .flatten()
        .and_then(|reply| tool_owner::parse_caller_tid(&reply))
        .map(|tid| alloc::vec![tool_owner::owner_arg(tid)]);
    let (tool_fs, tool_spawn) = match &owner_argv {
        Some(argv) => (
            spawn_tool(TOOL_FS_PATH, "tool-fs", argv),
            spawn_tool(TOOL_SPAWN_PATH, "tool-spawn", argv),
        ),
        None => {
            println("[hypha] WARN: own tid unknown; tool-fs and tool-spawn not started");
            (0, 0)
        }
    };

    let tools = Tools {
        fs: tool_fs,
        sys: tool_sys,
        spawn: tool_spawn,
    };
    let preamble = system_preamble();
    let mut conversation: Vec<(&'static str, String)> = Vec::new();
    let sin = stdin();

    // Register with the input service before entering the readline loop.
    // Retry a few times in case of a boot race where input service is still starting.
    for _ in 0..10 {
        if request_focus() {
            break;
        }
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
        let prompt = render_prompt(&preamble, &conversation);

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
    Err(String::from(
        "[tool limit reached — too many sequential calls]",
    ))
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
    // mask=0 (wildcard): accept from any sender so that input-service EV_ASCII events
    // dispatched to this cell (because it holds keyboard focus) don't deadlock the
    // input service. G18 enforces the mask in ipc_send — a masked recv blocks senders
    // that don't match, so we must use 0 here and filter by sender in the match arm.
    loop {
        match sys_recv(0, &mut buf) {
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
/// Side-effecting calls run only after the operator approves them on the console.
fn dispatch_tool(tools: &Tools, call: &ToolCall) -> Result<String, String> {
    let cell_tid = tools.route(&call.name);
    if cell_tid == 0 {
        return Err(alloc::format!(
            "tool '{}' not available (cell not spawned)",
            call.name
        ));
    }
    if tool_gate::needs_confirmation(&call.name) && !operator_approves(call) {
        // Ends the turn: a denied call must not let the model chain further calls.
        return Err(alloc::format!(
            "tool call '{}' denied by operator",
            call.name
        ));
    }
    invoke(cell_tid, &call.name, &call.args_json)
}

/// Show the exact call and wait for the operator's answer on this console.
/// Keys typed while the model was answering were discarded by `ask()`'s receive
/// loop, so only an answer typed after the prompt can approve.
fn operator_approves(call: &ToolCall) -> bool {
    print(tool_gate::confirmation_prompt(&call.name, &call.args_json).as_str());
    let mut answer = String::new();
    stdin().read_line(&mut answer).is_ok() && tool_gate::is_confirmed(&answer)
}

/// One `AgentToolRequest::Invoke` round trip with tool cell `cell_tid`.
fn invoke(cell_tid: usize, name: &str, args_json: &str) -> Result<String, String> {
    let req = AgentToolRequest::Invoke { name, args_json };
    let mut out = [0u8; 4096];
    let encoded =
        postcard::to_slice(&req, &mut out).map_err(|_| String::from("tool request too large"))?;

    match sys_send(cell_tid, encoded) {
        SyscallResult::Ok(_) => {}
        _ => return Err(alloc::format!("send to tool cell '{}' failed", name)),
    }

    let mut buf = [0u8; 4096];
    // mask=0: same fix as ask() — accept from any sender to avoid G18 deadlock with
    // the input service; filter by cell_tid in the match arm.
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender == cell_tid => {
                return match postcard::take_from_bytes::<AgentToolResponse>(&buf) {
                    Ok((AgentToolResponse::Ok { result_json }, _)) => Ok(result_json),
                    Ok((AgentToolResponse::Err { message }, _)) => Err(message),
                    Err(_) => Err(String::from("bad AgentToolResponse encoding")),
                };
            }
            SyscallResult::Ok(_) => continue,
            _ => return Err(alloc::format!("no reply from tool cell '{}'", name)),
        }
    }
}

/// Spawn a tool cell with `argv` as its command line; 0 when it cannot start.
///
/// Route order mirrors the shell: the kernel path route first — the only one
/// that admits a launch edge carrying authority (tool-spawn's SpawnCap) — then
/// the VFS route. A rejected attempt discards the staged argv, so it is
/// re-published before each attempt; otherwise a bound tool would start unbound.
fn spawn_tool(path: &str, label: &str, argv: &[String]) -> usize {
    let attempt = |spawn: fn(&str) -> SyscallResult| {
        if !ostd::set_spawn_argv(argv) {
            return None;
        }
        match spawn(path) {
            SyscallResult::Ok(tid) => Some(tid),
            _ => None,
        }
    };
    match attempt(sys_spawn_from_path_raw).or_else(|| attempt(sys_spawn_from_path)) {
        Some(tid) => {
            print("[hypha] ");
            print(label);
            println(" ready");
            tid
        }
        None => {
            print("[hypha] WARN: ");
            print(label);
            println(" not found");
            0
        }
    }
}

/// Flatten the heap conversation into a single role-tagged transcript.
/// The system preamble is prepended so the LLM always knows about tools.
/// Trimmed from the front if it would exceed the one-message IPC budget.
fn render_prompt(preamble: &str, conv: &[(&'static str, String)]) -> String {
    let mut s = String::from(preamble);
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
