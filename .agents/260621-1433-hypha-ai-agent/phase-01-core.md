# Phase 01 — `core` (agent brain, interactive chat)

## Context Links
- [plan.md](./plan.md) · [architecture.md](./architecture.md) · [os-gaps.md](./os-gaps.md) · [phase-00-llm-gateway.md](./phase-00-llm-gateway.md)
- Cell: `cells/apps/hypha/core/` · Gateway: `cells/apps/hypha/llm-gateway/`

## Overview
- **Priority**: P1 (first end-to-end chat).
- **Status**: ✅ Code complete — builds for riscv64 (ET_DYN PIE); `hypha-boot` integration test covers banner+prompt+exit; live LLM round-trip deferred to user env step (mock proxy at 10.0.2.2:8080).
- **Description**: `core` is the agent brain. It spawns `llm-gateway`, then loops: read a line
  from stdin (UART), keep the conversation in heap, ask the gateway over IPC, print the reply.

## Key Insights
- **`register_service` needs SpawnCap** (only `init` has it), so P1 avoids the registry entirely:
  `core` *spawns* the gateway via `sys_spawn_from_path` and gets its tid back, then does IPC by tid.
  `core` therefore needs the `spawn` capability (which it will need anyway for P3 `tool-spawn`).
- **Gateway refactored from standalone (P0) → service**: `CellRuntime::new().no_heartbeat()` (an
  LLM round-trip can exceed the default 5 s watchdog) + handler decoding `LlmRequest`, replying `LlmReply`.
- **One-message IPC budget**: prompt and reply each must fit ~4 KB (`IPC_BUF_SIZE`). `core` trims the
  transcript from the front; the gateway truncates the reply. Grant streaming lifts this later (os-gap G5).
- **stdin**: `ostd::io::stdin().read_line()` → `sys_read` (blocks on UART, echoes). Syscall `Read`.

## Requirements
- `core` manifest: `spawn = true`, `network = false`, `block_io = false`; syscalls
  `[Send, Recv, Read, Log, SpawnFromPath]`.
- Conversation kept in heap (`Vec<(role, String)>`), flattened to a role-tagged prompt per turn.
- Graceful: failed turn is popped from history; `exit`/`quit` ends the loop.
- `#![forbid? ]` — cells are `#![no_std] #![no_main]`; no unsafe.

## Related Code Files
- **Create**: `cells/apps/hypha/core/{Cargo.toml,build.rs,src/main.rs}`.
- **Modify**: `cells/apps/hypha/llm-gateway/src/main.rs` (standalone → service), its `Cargo.toml`
  (+ `agent-proto`, `postcard`).
- **Modify**: root `Cargo.toml` (+ `cells/apps/hypha/core`), `gen_disk.ps1` (build + `/bin/hypha`).

## Todo List
- [x] gateway → service (recv `LlmRequest` → TLS → reply `LlmReply`, no_heartbeat)
- [x] `core` cell: spawn gateway, stdin loop, conversation in heap, IPC round-trip
- [x] registered in root `Cargo.toml` + `gen_disk.ps1` (`/bin/hypha`, `/bin/llm-gateway`)
- [x] **builds for riscv64** — `cargo build --release -p hypha-core -p hypha-llm-gateway` green (ET_DYN PIE)
- [x] host LLM proxy at 10.0.2.2:8080 (plaintext `--plain` mode) — **confirmed working (boot run #2)**
- [x] boot + chat round-trip verified on QEMU — **CONFIRMED 2026-06-22**: multi-turn, clean exit ✅

## Success Criteria
- A boot run where `/bin/hypha` prints a prompt, accepts a typed line, and prints the LLM's reply;
  multi-turn context is preserved (heap conversation). No panics; gateway respawn (never-die) leaves
  `core` able to retry (deferred — needs supervisor wiring or core-side respawn).

## Risk Assessment
- **LLM reachability (G3, 🔴)** — host proxy + pinned IP; unverified until a boot run.
- **4 KB IPC cap (G5, 🟡)** — long prompts/replies truncated in P1; Grant streaming later.
- **UART contention** — `core` reads stdin while the shell is its parent; relies on the shell
  blocking on the foreground child. Validate on the boot run.

## Security Considerations
- `core` holds only `spawn` (+ basic IPC/stdin). No network, no block-I/O, no GPIO — side effects
  are delegated to capability-gated tool Cells in later phases.
- Gateway holds no network capability (TLS is mediated by the `net` service).

## Next Steps
- P2: tool protocol (`AgentToolRequest/Response`) + `tool-fs`; the agentic tool-use sub-loop.
- Promote hand-rolled HTTP/JSON to `ostd::http` + a no_std JSON crate once a second consumer appears.
