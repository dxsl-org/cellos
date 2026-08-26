# Phase 01 — net-tools LookupService Migration

**Context:** [plan.md](plan.md) · Priority P2 · Status: ✅ Complete (2026-06-11) · Effort ~1.5h

## Overview
Replace hardcoded `NET_ENDPOINT=6` / `VFS_ENDPOINT=3` constants in 5 net-tools
bins with `sys_lookup_service(service::NET|VFS)` resolved at startup, so tools
reconnect transparently after a service respawn.

## Key Insights (verified)
- `sys_lookup_service(service_id: u16) -> Option<usize>` — `ostd/src/syscall.rs:369`.
  Returns `Some(tid)` live, `None` when no provider. Open to all cells.
- Service IDs: `api::syscall::service::{VFS=1, NET=2}` — `api/src/syscall.rs:414-424`.
- `LookupService` IS allowlist-gated (cap bit 37, `api/src/syscall.rs:307-308`).
  All 5 bins declare `[Send, Recv, Log, StateRestore]` → must append `LookupService`.
- Reference pattern (`robot-demo/src/main.rs:101`):
  ```rust
  let net_ep = match sys_lookup_service(service::NET) {
      Some(ep) => ep,
      None => { println("curl: no net service"); return; }
  };
  ```
- The const is read-only; used only as `sys_send(NET_ENDPOINT, ...)`. Helpers
  reference it globally → thread resolved tid as a param.

## Requirements
- F1: Each bin resolves its endpoint(s) once in `main()`; `None` → log + return (no panic).
- F2: Helper fns that send IPC take `net_ep: usize` (and `vfs_ep: usize` for wget/httpd).
- F3: Add `LookupService` to each bin's `declare_syscalls!`.
- F4: Import `api::syscall::service` + `ostd::syscall::sys_lookup_service`.
- NF1: `#![forbid(unsafe_code)]` preserved (Law 4). Tid stays plain `usize` (not a VAddr).

## Files to modify
| File | Const → lookup | VFS too? | Helpers needing `ep` param |
|------|----------------|----------|----------------------------|
| `cells/apps/net-tools/src/bin/curl.rs` | NET | no | `query_state`, `close_socket` |
| `cells/apps/net-tools/src/bin/nc.rs` | NET | no | `server_mode`, `serve_connection`, `query_state`, `close_socket` |
| `cells/apps/net-tools/src/bin/mqtt.rs` | NET | no | `mqtt_handshake`, `do_publish`, `do_subscribe`, `tcp_send`, `mqtt_recv_once`, `mqtt_recv`, `close_socket` |
| `cells/apps/net-tools/src/bin/wget.rs` | NET | yes | net helpers + `vfs_*` (thread both tids) |
| `cells/apps/net-tools/src/bin/httpd.rs` | NET | yes | `close_cap`, `query_state`, `tcp_send`, `drain_request` (net); `vfs_read` (vfs) |

Leave `ping.rs` untouched (declares only `[Log]`, no networking).

## Implementation steps (per bin)
1. Delete `const NET_ENDPOINT`/`const VFS_ENDPOINT`.
2. Add imports: `use ostd::syscall::sys_lookup_service;` and `use api::syscall::service;`.
3. Append `LookupService` to `api::declare_syscalls![...]`.
4. In `main()`, resolve tid(s) with the match pattern above (error string per tool,
   e.g. `"httpd: no net service"`, `"wget: no vfs service"`); return on `None`.
5. Add `net_ep: usize` (and `vfs_ep: usize`) params to every helper that sends IPC;
   replace `NET_ENDPOINT`→`net_ep`, `VFS_ENDPOINT`→`vfs_ep`; update call sites.

## Todo
- [x] curl.rs
- [x] nc.rs
- [x] mqtt.rs
- [x] wget.rs (net + vfs)
- [x] httpd.rs (net + vfs)
- [x] `cargo check -p app-net-tools` clean (1 pre-existing warning in ostd, unrelated)

## Success criteria
- No `const NET_ENDPOINT`/`VFS_ENDPOINT` remain (grep clean).
- `cargo check -p net-tools` (or workspace) passes; no clippy unsafe.
- Boot + run: curl/wget/httpd/nc/mqtt work; still work after net/VFS respawn
  (supervisor restart) under a new tid — the prior failure mode.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Forget `LookupService` in allowlist → cap fault at runtime | M×H | Step 3 explicit per bin; grep `declare_syscalls` after edit |
| Helper threading misses a call site → stale const compile error | M×L | Compiler catches (const removed); fix until clean |
| `None` during respawn window mid-run | L×M | Tools are short-lived/one-shot; clean error+return is acceptable for MVP |
| Wrong service ID (tid 6/3 ≠ service id 2/1) | L×H | Use named `service::NET`/`service::VFS`, never literals |

## Security
`LookupService` is an open syscall; adding it to net-tools grants no new privilege
beyond resolving public service endpoints. No unsafe introduced.

## Next Steps
- Follow-up (out of scope): consider a small retry-on-`None` loop if tools ever
  become long-running daemons (httpd already loops — but resolves net once at start;
  if net respawns mid-serve, re-lookup inside the accept loop would be the upgrade).
