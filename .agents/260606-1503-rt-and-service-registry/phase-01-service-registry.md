# Phase 01 — Stable Service-ID Registry

**Priority:** High (closes the supervisor recovery loop)  **Status:** PENDING Law-1 confirm

## Problem
Supervisor respawns a dead service → new tid. Clients hold the old tid (`config_client.rs:5`
`service_id: usize` = tid) → all clients break after a restart. No name→tid indirection exists.

## Design (KISS, trust-centralized in the supervisor)
Kernel-owned `service_id(u16) → current_tid(usize)` map. The **supervisor (init, SpawnCap)** owns
the namespace: it spawns a service, gets the tid, and registers it. On respawn it re-registers
the new tid. Clients resolve `service_id → tid` right before sending, so they always reach the
live instance. The kernel auto-clears a stale entry when its tid dies, so a lookup during the
death→respawn gap returns "none" (client retries) instead of a dead tid.

```
init spawns vfs → tid=4 → RegisterService(VFS, 4)
client: LookupService(VFS) → 4 → sys_send(4, ...)
vfs dies → exit_task clears VFS→4 ; supervisor respawns vfs → tid=9 → RegisterService(VFS, 9)
client: LookupService(VFS) → 9 → reconnected, zero client code aware of the restart
```

## libs/api surface (LAW 1 — 2× CONFIRM REQUIRED)
`libs/api/src/syscall.rs`:
```rust
// in enum ViSyscall, Advanced-IPC 200 range:
/// Register `tid` as the current provider of well-known `service_id`. SpawnCap-gated
/// (the supervisor owns the service namespace). ABI: a0 = service_id:u16, a1 = tid → 0 / MAX.
RegisterService = 205,
/// Resolve a well-known `service_id` to its current provider tid. Open to all cells.
/// ABI: a0 = service_id:u16 → tid (>0), or 0 if no live provider is registered.
LookupService = 206,

// allowlist_bit(): RegisterService is privileged (SpawnCap) → None arm (like NotifyOnExit).
//                  LookupService → a new stable bit (open syscall).
// from(): 205 => RegisterService, 206 => LookupService.

/// Well-known service IDs (stable ABI). 0 reserved = "none".
pub mod service {
    pub const VFS: u16 = 1;
    pub const NET: u16 = 2;
    pub const INPUT: u16 = 3;
    pub const CONFIG: u16 = 4;
    pub const COMPOSITOR: u16 = 5;
}
```

## Implementation steps
1. **libs/api** (Law 1): enum variants 205/206, `allowlist_bit` arms, `from()` arms, `service` mod.
2. **kernel/src/cell/service_registry.rs** (new, <100 lines): `static REGISTRY:
   Spinlock<BTreeMap<u16, usize>>`; `register(id, tid)`, `lookup(id) -> Option<usize>`,
   `clear_tid(tid)` (remove all entries pointing at a dead tid), `force_unlock_locks()`.
3. **kernel/src/task/syscall.rs**: handle `RegisterService` (SpawnCap gate, else PermissionDenied)
   + `LookupService` (open). Wire into the `Syscall` enum + dispatch.
4. **kernel/src/task/scheduler.rs**: in `exit_task`, call `service_registry::clear_tid(tid)` so a
   dead provider is unresolvable until re-registered.
5. **kernel fault path** (`task.rs force_unlock_all_kernel_locks`): add
   `service_registry::force_unlock_locks()`.
6. **libs/ostd/src/syscall.rs**: `sys_register_service(id: u16, tid: usize)`,
   `sys_lookup_service(id: u16) -> Option<usize>`.
7. **cells/apps/init**: after each successful spawn, `sys_register_service(SERVICE, tid)`; same in
   the respawn branch of the supervisor loop.
8. **proof client**: change one client to resolve via `sys_lookup_service` (retry-until-found)
   instead of a fixed tid, to demonstrate reconnection across a restart.

## Success criteria
- Build (kernel + cells) clean; boot to shell.
- Boot-verify: a client resolves its service via LookupService; after killing that service, the
  supervisor respawns it, re-registers, and the client's next lookup returns the NEW tid and the
  request succeeds. No client holds a stale tid. 0 panics.

## Risks
- **Namespace hijack:** mitigated by SpawnCap gate on RegisterService (only the trusted supervisor
  registers). Self-registration deliberately NOT allowed for arbitrary cells.
- **Boot race:** client lookup before init registers → returns 0 → client retries. Documented.
- **Law 1:** 2× confirm before editing libs/api. No other ABI change.
