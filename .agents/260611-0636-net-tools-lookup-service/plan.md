---
title: "net-tools: migrate hardcoded service tids → LookupService"
description: "Replace hardcoded NET/VFS endpoint constants in net-tools bins with sys_lookup_service so tools survive service restarts."
status: pending
priority: P2
effort: 1 phase (~1.5h)
branch: main
tags: [net-tools, service-registry, reliability, ipc]
created: 2026-06-11
---

# net-tools → LookupService Migration

## Problem
5 net-tools bins hardcode service **task IDs** (not service IDs):
- `curl.rs:10`, `nc.rs:10`, `mqtt.rs:16` — `const NET_ENDPOINT: usize = 6;`
- `wget.rs:15-16`, `httpd.rs:19-20` — `NET_ENDPOINT = 6` + `VFS_ENDPOINT = 3`

Tids change when the supervisor respawns a service → every tool breaks after a
restart. The kernel exposes `sys_lookup_service(service_id) -> Option<usize>`
(syscall 206), resolving the *live* provider tid. Well-known IDs live in
`api::syscall::service` (`VFS = 1`, `NET = 2`).

## Solution (single phase)
Resolve `service::NET` / `service::VFS` once at the top of `main()`, return early
with a logged error on `None`, and thread the resolved tid into helper functions
(they currently read the global const directly).

## Phases
| # | Phase | Status | Effort | File |
|---|-------|--------|--------|------|
| 01 | LookupService migration (5 bins) | ✅ Complete (2026-06-11) | ~1.5h | [phase-01](phase-01-lookup-service-migration.md) |

## Key dependencies / gotchas
- **`LookupService` is gated by `declare_syscalls!`** (cap bit 37). All 5 bins
  currently declare `[Send, Recv, Log, StateRestore]` — must add `LookupService`.
  (`ping.rs` declares only `[Log]` and does no networking — leave untouched.)
- Helpers (`query_state`, `close_socket`, `tcp_send`, `vfs_read`,
  `serve_connection`, `mqtt_handshake`, `do_publish`, `do_subscribe`) reference
  the global const → must take `net_ep: usize` / `vfs_ep: usize` params.
- `None` window is transient (death→respawn). Tools are one-shot/short-lived, so
  a clean error+return is acceptable (no retry loop required for MVP).

## Success criteria
net-tools (curl/nc/mqtt/wget/httpd) keep working after net/VFS respawn under a
new tid; build is clean; no hardcoded endpoint constants remain.
