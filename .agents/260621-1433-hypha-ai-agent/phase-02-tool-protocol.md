# Phase 02 — Tool protocol + `tool-fs`

## Context Links
- [plan.md](./plan.md) · [architecture.md](./architecture.md) · [os-gaps.md](./os-gaps.md)
- [phase-01-core.md](./phase-01-core.md) · libs/agent-proto · cells/apps/hypha/tools/fs/

## Overview
- **Priority**: P2 — first real agentic capability.
- **Status**: ✅ **COMPLETE** — code + boot run #3 verified 2026-06-22
- **Description**: Add a typed tool-call sub-protocol between `core` and tool Cells.
  The LLM signals tool use by outputting `TOOL_CALL: {...}` in its reply content;
  `llm-gateway` parses this and returns `LlmReply::ToolCalls`. `core` dispatches each
  call to the appropriate tool Cell via `AgentToolRequest/Response` typed IPC, collects
  results, appends them to the working prompt as `tool_result: ...`, and calls the LLM
  again (up to `MAX_TOOL_ROUNDS = 5`). `tool-fs` is the first tool: read/write/list `/data`.

## Key Insights
- **Text-based TOOL_CALL protocol (ReAct-style)**: the LLM outputs `TOOL_CALL: {json}` as
  its entire content when it wants a tool. No streaming, no function-calling API format —
  works with any instruction-following model and the mock proxy without API changes.
- **`tool-fs` needs NO `block_io` cap**: it reaches the filesystem via IPC to the VFS
  service (`VfsClient` = postcard IPC), not direct disk access. Capabilities: `[Send,Recv,Log,LookupService]`.
- **Agentic loop inside `core`**: the tool sub-loop is entirely in `core::run_turn()`.
  The gateway is a dumb transducer — it just parses the `TOOL_CALL:` prefix and returns
  `LlmReply::ToolCalls`. This keeps the gateway stateless and swappable.
- **Sequential execution in v1**: tool calls run one-at-a-time in the sub-loop. Parallelism
  via worker-Cell fan-out is G11 (later).
- **Conversation context**: tool interactions are appended to the working prompt string
  (`tool_call: / tool_result:` lines) for the LLM's next turn. `trim_front` still handles
  overflow. Tool interactions are NOT persisted to the permanent conversation — only the
  final LLM text reply is stored (no context pollution in P2).
- **Mock proxy trigger**: any prompt containing "list", "files", "ls", "dir", or "folder"
  (case-insensitive) makes the mock return a `list_dir` tool call on `/data`. Follow-up
  prompts containing `TOOL_RESULT:` make the mock synthesize a final summary.

## Requirements
- **agent-proto**: add `LlmReply::ToolCalls(alloc::vec::Vec<ToolCall>)` variant.
- **llm-gateway**: `extract_tool_call(body: &[u8]) -> Option<ToolCall>` in `http.rs`;
  `main.rs` returns `ToolCalls([call])` when a tool call is detected.
- **tool-fs** (new crate `cells/apps/hypha/tools/fs/`):
  - Manifest: `block_io=false, network=false, spawn=false`
  - Syscalls: `[Send, Recv, Log, LookupService]`
  - Handles: `AgentToolRequest::Invoke{name,args_json}` → dispatch to VfsClient
  - Supports: `read_file`, `write_file`, `list_dir`
  - Returns: `AgentToolResponse::Ok{result_json}` or `Err{message}`
- **core**: spawn `tool-fs`, system preamble in `render_prompt`, `run_turn()` tool loop.

## Architecture
```
core                         tool-fs
 ├─ render_prompt()           ├─ AgentToolRequest::Invoke
 ├─ ask(gw, prompt)           ├─ dispatch(name, args_json)
 ├─ run_turn(gw, tf, prompt)  │   ├─ read_file  → VfsClient
 │   ├─ LlmReply::Text  ──────│   ├─ write_file → VfsClient
 │   ├─ LlmReply::ToolCalls   │   └─ list_dir   → VfsClient
 │   │   └─ dispatch_tool(tf) └─ AgentToolResponse::Ok/Err
 │   │       └─ AgentToolRequest → tool-fs
 │   └─ loop (max 5 rounds)
 └─ conversation += final reply
```

## Related Code Files
- **Modify**: `libs/agent-proto/src/lib.rs`
- **Modify**: `cells/apps/hypha/llm-gateway/src/http.rs`
- **Modify**: `cells/apps/hypha/llm-gateway/src/main.rs`
- **Modify**: `cells/apps/hypha/core/src/main.rs`
- **Create**: `cells/apps/hypha/tools/fs/{Cargo.toml,build.rs,src/main.rs}`
- **Modify**: root `Cargo.toml` (+ `cells/apps/hypha/tools/fs`)
- **Modify**: `gen_disk.ps1` (+ `/bin/tool-fs`)
- **Modify**: `tools/hypha-mock-llm/mock_proxy.py` (tool call simulation)

## Todo List
- [x] phase-02 plan doc created
- [x] `agent-proto`: `LlmReply::ToolCalls` variant
- [x] `llm-gateway/http.rs`: `extract_tool_call()` + helpers
- [x] `llm-gateway/main.rs`: return `ToolCalls` on tool call
- [x] `tool-fs` crate (Cargo.toml + build.rs + src/main.rs)
- [x] `core/main.rs`: TOOL_FS_PATH, system preamble, `run_turn()` loop
- [x] root `Cargo.toml` + `gen_disk.ps1` updated
- [x] `mock_proxy.py` tool simulation mode
- [x] **builds for riscv64** — all three ET_DYN PIE: tool-fs 72KB, core 52KB, gateway 76KB
- [x] **boot + tool round-trip VERIFIED 2026-06-22** — full agentic loop confirmed (see os-gaps.md § Boot run #3)

## Success Criteria
A boot run where:
1. User asks: "what files are in /data?"
2. Hypha replies with `TOOL_CALL:` → `tool-fs` returns list → LLM synthesizes answer.
3. No panics; tool errors surface gracefully; conversation context is preserved.

## Risk Assessment
- **VFS `/data` access (🟡)**: `list_dir("/data")` may return empty or error if the
  FAT32 partition has no `/data` dir. Mitigation: `tool-fs` propagates the error
  cleanly; mock proxy returns a canned list regardless.
- **IPC ordering race (🟢)**: sequential dispatch (tool request then recv) has no
  interleaving risk since gateway and tool-fs are separate cells that only respond
  when queried. sys_recv(0, ...) receives from any sender; safe because requests
  are strictly sequential.
- **args_json brace-counting parser (🟢)**: only handles well-formed single-level JSON.
  Good enough for tool calls the LLM generates; robust parser is G4 (later).

## Security Considerations
- `tool-fs` can only write to paths the VFS service permits — kernel enforces the
  `block_io=false` constraint; all file access goes through the VFS service's policy.
- `core` holds no file or network caps directly; it can only call `tool-fs` via IPC.
  This is the LBI least-privilege model: dangerous authority is scoped to the minimum
  Cell that needs it.

## Next Steps
- P3: `tool-sys` (cell list, process info) + `tool-spawn` (launch cells by name).
- G7 (dynamic service discovery) becomes relevant when tool Cells need self-registration.
