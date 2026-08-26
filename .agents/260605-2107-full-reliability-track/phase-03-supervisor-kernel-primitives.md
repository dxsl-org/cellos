---
phase: 03
title: Supervisor Kernel Primitives
priority: P0
status: planned
depends_on: ["00", "02"]
risk: medium
---

# Phase 03 — Supervisor Kernel Primitives

> ⚠️ **Red-team revisions (authoritative — override conflicting text below):**
> - **Do NOT extend `CellNode`.** It's stored as `Arc<CellNode>` ([registry.rs:58](../../kernel/src/cell/registry.rs))
>   (immutable through Arc) AND `CELL_REGISTRY` is **never populated** (`spawn_from_path` never calls
>   `register()`). Store `(elf_path, spawn_args, restart_count, parent_cell_id)` on the **`Task`**
>   ([tcb.rs](../../kernel/src/task/tcb.rs)) or a new kernel-owned `Spinlock<BTreeMap>`. Add
>   "persist path/args at spawn" as **real work**, not a field addition.
> - **NotifyOnExit delivery is plumbed via `exit_task` (done in Phase 00).** Phase 00 moves
>   waiter-wake into `exit_task` so Exit/ForceExit/**fault** all fire uniformly. This phase only
>   adds the subscriber table + the `NotifyOnExit` syscall on top of that chokepoint.
> - **Stable service id is NOT sufficient for VFS recovery.** VFS is reached via a raw fn-pointer
>   `VFS_HANDLER_PTR: AtomicPtr` ([fast_ipc.rs:31](../../libs/ostd/src/fast_ipc.rs)), nulled on fault.
>   A restarted VFS must re-run `register_vfs`/`set_vfs_handler_cell` from the new instance.
>   Stable-id covers only the `sys_send(id,…)` slow path. List fast-IPC re-registration as a
>   first-class restart step (consumed by Phase 04).
> - **Add a generation/validity marker to `state_stash` (validated decision 2026-06-05).** State
>   recovery is ON, so the stash primitive must let the supervisor tell "this blob preceded a
>   crash". Add a per-slot `generation: u64` (bumped on each `stash`) returned by `restore`, plus
>   an explicit `poison(key)` so the supervisor can invalidate a blob that caused a post-restore
>   fault. Kernel-side primitive here; policy in Phase 04.

## Context Links
- Spec: [12-reliability.md](../../docs/specs/12-reliability.md) §4.3
- Code: [kernel/src/task/tcb.rs](../../kernel/src/task/tcb.rs) (Task struct, `waiters`)
- Code: [kernel/src/task/scheduler.rs](../../kernel/src/task/scheduler.rs) (`exit_task` @239-269)
- Code: [kernel/src/loader.rs](../../kernel/src/loader.rs) (`spawn_from_path` @56)
- Code: [kernel/src/cell/registry.rs](../../kernel/src/cell/registry.rs) (`CellNode`)
- Code: [kernel/src/task/syscall.rs](../../kernel/src/task/syscall.rs) (`Wait{pid}`, `Exit{code}`)
- Code: [kernel/src/audit.rs](../../kernel/src/audit.rs) (`CellExit`/`CellFault`)

## Overview
- **Priority:** P0 (highest ROI — ~70% latent already)
- **Status:** planned
- **Description:** Add the four kernel primitives a userspace supervisor needs to implement
  Erlang/OTP-style "let it crash + restart": (1) parent tracking, (2) ELF-path persistence,
  (3) stable well-known service IDs, (4) push-model death notification. The existing
  `waiters` + `Wait{pid}` + audit log get us most of the way; this fills the gaps.

## Key Insights
- `Wait{pid}` already blocks a watcher until a task exits — a thin synchronous "link". The
  missing piece is **async, multi-watcher, fire-on-both-exit-and-fault** notification.
- `CellId` is currently derived from `tid` (`CellId(tid as u64)` @loader.rs:120). A respawned
  cell gets a NEW tid → NEW CellId → callers lose the endpoint. **Stable service IDs** require
  decoupling a *well-known service id* from the volatile tid.
- `spawn_from_path(path)` already does everything needed to restart a cell — but the path is
  **not stored**. Persisting `(path, manifest, spawn_args)` makes restart a one-call op.
- Keep policy OUT of the kernel. Kernel exposes mechanism (notify, respawn-by-path, stable id);
  the supervisor *cell* (Phase 04) owns restart strategy. KISS + correct layering.

## Requirements
**Functional**
- Each task records `parent_cell_id`.
- The kernel stores each spawned cell's `elf_path` (+ spawn args) for restart.
- A name→id service registry maps a stable service id (e.g. "vfs") to the current tid;
  respawn updates the mapping so callers keep using the stable id.
- New syscall `NotifyOnExit { watched }`: registers caller to receive an async message when
  `watched` exits OR faults; the message carries `{watched_id, reason}`.
- A `RespawnFromStash { service }` or reuse of `spawn_from_path` + state-stash so a restarted
  cell can recover its last serialized state.

**Non-functional**
- Notification delivery survives the watched task's death (queued to watcher, not lost).
- Registry + path storage bounded (reuse `MAX_CELLS`); document caps.

## Architecture
```
Task (tcb.rs) += parent_cell_id: Option<CellId>
CellNode (registry.rs) += elf_path: Option<String>, spawn_args: Option<Vec<u8>>, restart_count

Service registry (new, kernel/src/cell/service_registry.rs):
   BTreeMap<ServiceName, CellId>   // "vfs" → current tid-derived id
   register(name, id) / lookup(name) -> Option<CellId> / rebind(name, new_id)

Death notification:
   subscribers: BTreeMap<watched_tid, Vec<watcher_tid>>
   NotifyOnExit{watched}  → subscribers[watched].push(caller)
   on exit_task(tid) / terminate_on_fault(tid):
       for w in subscribers.remove(tid): deliver async msg {tid, reason} to w
       (reuse IPC delivery; if w blocked in Recv, wake; else queue)

Restart (driven by Phase 04 supervisor cell):
   supervisor receives death msg → looks up elf_path → spawn_from_path → rebind service id
```

## Related Code Files
**Modify**
- `kernel/src/task/tcb.rs` — `parent_cell_id` field.
- `kernel/src/cell/registry.rs` — `elf_path`, `spawn_args`, `restart_count` on `CellNode`.
- `kernel/src/loader.rs` — persist path/args into the registry at spawn; set `parent_cell_id`.
- `kernel/src/task/scheduler.rs` — `exit_task` fires death notifications; faults route through too.
- `kernel/src/task/syscall.rs` — dispatch `NotifyOnExit` (and service lookup/register if exposed to cells).
- `kernel/src/audit.rs` — (optional) `CellRestart` event.
- `libs/api/src/syscall*.rs` + `libs/api/src/ipc.rs` — **Law 1: new syscall id + death-msg
  type. Requires 2× user confirm.**

**Create**
- `kernel/src/cell/service_registry.rs` (parallel `service_registry/` only if it grows; no mod.rs).

## Implementation Steps
1. Add `parent_cell_id` to `Task`; set it in the spawn path from the caller's cell id.
2. Add `elf_path`/`spawn_args`/`restart_count` to `CellNode`; populate in `spawn_from_path`.
3. Create `service_registry.rs` with register/lookup/rebind under a `Spinlock`.
4. **(Law 1 gate)** Define syscall `NotifyOnExit{watched}` and the death-notification message
   type in `libs/api`. Pause for 2× user confirmation before editing `libs/api`.
5. Implement subscriber table + delivery; hook BOTH `exit_task` and `terminate_*_on_fault`.
6. Ensure delivery is reason-tagged (`Exit{code}` vs `Fault{scause}` vs `Watchdog`).
7. Expose `service lookup/rebind` to the supervisor cell (gated by `SpawnCap` or a new cap).
8. Build per-arch; boot QEMU; verify a watcher cell receives a death message.

## Todo List
- [ ] `parent_cell_id` on Task
- [ ] `elf_path`/`spawn_args`/`restart_count` on CellNode + populate at spawn
- [ ] `service_registry.rs` (register/lookup/rebind)
- [ ] **Law 1 confirm:** `NotifyOnExit` syscall + death-msg type in libs/api
- [ ] Subscriber table + delivery on exit AND fault
- [ ] Reason-tagged messages
- [ ] Expose service lookup/rebind (capability-gated)
- [ ] Test: watcher cell gets {watched, reason} when target exits and when it faults

## Success Criteria
- A watcher cell calling `NotifyOnExit(target)` receives a message when `target` exits
  **and** when `target` is killed by fault/watchdog (both paths fire).
- `spawn_from_path` for a service records its path; a manual respawn call recreates it and
  `service_registry` lookup returns the new tid under the same stable name.
- No notification lost when the watcher is mid-`Recv` vs ready.

## Risk Assessment
- **Law 1 interface change (Med→High process risk).** New syscall + IPC message type touches
  the sacred ABI. *Mitigation:* batch all `libs/api` edits into one reviewed change; get 2×
  confirm; add `syscall_tests` coverage (mirrors existing test file).
- **Notification races at death (Med).** Watcher could subscribe as target is dying.
  *Mitigation:* deliver-or-immediately-return-dead semantics; subscribe checks live-state under
  the scheduler lock.
- **Stable-id aliasing (Med).** Rebinding a service id while an old caller holds the prior tid.
  *Mitigation:* callers resolve via service name each session OR get a "service moved" error;
  document the contract.

## Security Considerations
- `NotifyOnExit` leaks liveness of arbitrary cells. Gate to same-parent or capability-holders
  to avoid an untrusted cell mapping the system. Default: a cell may watch only cells it
  spawned (parent_cell_id match) unless it holds a supervisor capability.

## Next Steps
- Phase 04 implements the actual restart policy in userspace on top of these primitives.
