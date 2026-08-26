# Phase 03 — `tool-sys` + `tool-spawn`

## Context Links
- [plan.md](./plan.md) · [phase-02-tool-protocol.md](./phase-02-tool-protocol.md) · [os-gaps.md](./os-gaps.md)
- Cells: `cells/apps/hypha/tools/sys/` · `cells/apps/hypha/tools/spawn/`

## Overview
- **Priority**: P3 — "OS agent" milestone; Hypha can now inspect and control its own OS.
- **Status**: ✅ Code complete — builds for riscv64 (ET_DYN PIE); `hypha-p3-boot` integration test added; live boot run #4 deferred to user env step (mock proxy at 10.0.2.2:8080)
- **Description**: Two new tool cells give Hypha read/write access to the Cellos process
  table and cell lifecycle. With these, the agent can answer "what's running?", "what
  OS is this?", "spawn /bin/nc", or "kill cell 7" — purely via capability-gated IPC,
  without the agent itself holding SpawnCap directly for anything it shouldn't do.

## Key Insights
- **`tool-sys` holds no caps**: `sys_get_procs` (GetProcs=30) and `sys_lookup_service`
  (LookupService=206) are open to any cell. No SpawnCap needed.
- **`tool-spawn` holds SpawnCap**: manifest `spawn=true`; `sys_spawn_from_path` and
  `sys_force_exit` (always-permitted, SpawnCap-gated at kernel dispatch) require it.
  This is the LBI least-privilege pattern: core can't kill arbitrary cells directly;
  it asks tool-spawn which holds the explicit kill authority.
- **`ProcessInfo` struct**: `{id: usize, state: usize (0=ready/1=running/2=waiting/3=dead),
  name: [u8; 32]}`. Buffer of 32 entries is enough for G1 cell counts.
- **State-to-string**: 0→"ready", 1→"running", 2→"waiting", _→"dead".
- **Tool routing in core**: `Tools {fs, sys, spawn}` struct + `route(name)` method maps
  tool names to the right cell tid. Unknown tools → error; tid=0 → "unavailable".

## Tools Exposed

### `tool-sys`
| Tool | Args | Result |
|------|------|--------|
| `list_cells` | `{}` | `{"cells":[{"id":N,"name":"...","state":"..."},...]}`|
| `sys_info` | `{}` | `{"os":"Cellos","version":"v0.2.1-dev","arch":"riscv64"}` |
| `lookup_service` | `{"name":"vfs"}` | `{"tid":N}` or Err |

### `tool-spawn`
| Tool | Args | Result |
|------|------|--------|
| `spawn_cell` | `{"path":"/bin/nc"}` | `{"tid":N}` or Err |
| `kill_cell` | `{"tid":N}` | `{"ok":true}` or Err |

## Architecture
```
core → Tools.route(name) → tool-sys  (GetProcs + LookupService)
                         → tool-spawn (SpawnFromPath + ForceExit)
                         → tool-fs   (VfsClient — P2)
```

## Related Code Files
- **Create**: `cells/apps/hypha/tools/sys/{Cargo.toml,build.rs,src/main.rs}`
- **Create**: `cells/apps/hypha/tools/spawn/{Cargo.toml,build.rs,src/main.rs}`
- **Modify**: `cells/apps/hypha/core/src/main.rs` (Tools struct, routing, SYSTEM_PREAMBLE)
- **Modify**: root `Cargo.toml`, `gen_disk.ps1`
- **Modify**: `tools/hypha-mock-llm/mock_proxy.py`

## Todo List
- [x] phase-03 plan doc
- [x] `tool-sys` crate (list_cells, sys_info, lookup_service)
- [x] `tool-spawn` crate (spawn_cell, kill_cell)
- [x] `core`: Tools struct, routing, expanded SYSTEM_PREAMBLE
- [x] Cargo.toml + gen_disk.ps1
- [x] mock proxy: new P3 tool triggers (list_cells / sys_info / lookup_service / spawn_cell / kill_cell)
- [x] **builds riscv64** — `cargo build --release` green (all 5 ET_DYN PIE: gw 76KB, core 52KB, fs 72KB, sys ~60KB, spawn ~58KB)
- [x] `hypha-p3-boot` integration test added (`tests/integration/tests/hypha-p3-boot.rs`)
- [ ] boot run #4 verified (needs host mock proxy + gen_disk.ps1 run)

## Success Criteria
Boot run where:
1. "what cells are running?" → `list_cells` → list of cell names + states
2. "what OS is this?" → `sys_info` → OS name + arch
3. "spawn /bin/nc" → `spawn_cell` → returns tid

## Security
- `tool-sys` is read-only; no caps beyond basic IPC.
- `tool-spawn` holds SpawnCap but accepts only VFS paths (`/bin/...`); the kernel
  validates the path at `sys_spawn_from_path`. `kill_cell` is gated by kernel
  SpawnCap check + rejection of system cells.
- `core` never holds SpawnCap directly for kill — it delegates to tool-spawn.
  (NOTE: core already has spawn=true from P1 for spawning its own tool cells.
  This is acceptable because init owns supervision; core's SpawnCap is limited
  to spawning new tool cells, not replacing services.)
