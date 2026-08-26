---
phase: 04
title: Root Supervisor Cell + Restart Policies
priority: P0
status: planned
depends_on: ["03", "05"]
risk: medium
---

# Phase 04 — Root Supervisor Cell

> ⚠️ **Red-team revisions (authoritative — override conflicting text below):**
> - **Now depends on Phase 05 (reclamation).** Auto-restart turns one death into an unbounded
>   restart stream; `exit_task` frees nothing today → every restart leaks stacks + segments → OOM
>   (which triggers the Phase-00 lock path). **Do not enable auto-restart until 05 lands**, OR ship
>   with auto-restart OFF by default + a hard restart cap.
> - **Supervisor death must NOT default to reboot.** Add a kernel-minimal "respawn the supervisor
>   from its stored `elf_path`" fallback (reuse Phase 03 persistence + `spawn_from_path`) BEFORE
>   escalating to reboot. Reboot is last-resort after the supervisor trips its own intensity limit.
> - **State recovery = ON with a generation marker (validated decision 2026-06-05).**
>   `state_stash::restore` currently returns the same blob every time with no validity check
>   ([state_stash.rs:44](../../kernel/src/cell/state_stash.rs)) → a cell that crashed *because* of
>   its state would restore-then-crash forever. The generation/validity marker is built into
>   `state_stash` in **Phase 03**; here the supervisor uses it: on a post-restore fault, mark the
>   slot poisoned and **cold-restart** (no state) on the next attempt. Track `restart_count` +
>   last-restore-generation per child to detect the restore→fault correlation.
> - **`rest_for_one`/`one_for_all` is NOT pure-YAGNI.** A faulting cell holding a grant/lease may
>   leave shared memory corrupt; restart recovers only the cell's *private* state. Dependents that
>   shared memory with the dead cell must also be restarted. Document this; start `one_for_one` but
>   design the dependency hook.
> - **Unify with hotswap.** [hotswap.rs](../../kernel/src/cell/hotswap.rs) also replaces cells via
>   `spawn_from_path` + a separate `FROZEN` set. Fault during a hotswap → double instance / orphaned
>   FROZEN / mis-routed IPC. Supervisor restart must check `is_frozen()` and decline; hotswap must
>   suppress the death-notification for the instance it intentionally tears down. Ideally both fund
>   through one "replace cell" primitive.

## Context Links
- Spec: [12-reliability.md](../../docs/specs/12-reliability.md) §4.3
- Code: [kernel/src/main.rs](../../kernel/src/main.rs) (init spawned CellId(1) + SpawnCap @36,248-256)
- Code: [cells/](../../cells/) (init/shell cell sources — root supervisor host)
- Code: [kernel/src/cell/state_stash.rs](../../kernel/src/cell/state_stash.rs) (state recovery)
- Depends on Phase 03 primitives (NotifyOnExit, service registry, respawn-by-path).

## Overview
- **Priority:** P0 (turns mechanism into actual "never-die")
- **Status:** planned
- **Description:** Implement the Erlang/OTP-style supervisor as a **userspace cell** (extend
  `init`, which already holds `SpawnCap`). It watches its supervised children, and on death
  restarts them per policy with backoff and restart-intensity limits. This is the component
  that makes a crashed VFS/net driver come back automatically.

## Key Insights
- Policy belongs in userspace (`#![forbid(unsafe_code)]`), not the kernel — KISS + safety.
- A supervised child is `{ name, path, policy, max_restarts, period }`. The supervisor keeps a
  static child-spec list (config or compiled-in initially; VFS-loaded later).
- **Restart intensity limit** is the safety valve: if a child crashes faster than
  `max_restarts` within `period`, the supervisor escalates (stop trying, or reboot via SBI) —
  prevents an infinite crash-restart storm that looks "alive" but does nothing.
- State recovery is opt-in via `state_stash`: a child stashes state before dying (or
  periodically); the restarted instance restores it. Stateless drivers skip this.

## Requirements
**Functional**
- Supervisor registers `NotifyOnExit` for each child after spawning it.
- On child death: classify reason; apply policy:
  - `permanent` → always restart.
  - `transient` → restart only on abnormal exit (fault/watchdog), not clean exit.
  - `temporary` → never restart.
- Enforce `max_restarts` within rolling `period`; on breach → escalate (configurable:
  give-up-and-log, or SBI reboot for critical children).
- Exponential backoff between restarts (cap at a max delay).
- After restart, rebind the child's stable service id (Phase 03 registry) so callers reconnect.

**Non-functional**
- Supervisor itself must be robust: it must not crash on a child it can't restart (log + skip).
- Supervisor is the most-trusted cell — minimal logic, heavily reviewed.

## Architecture
```
ChildSpec { name, path, policy: Permanent|Transient|Temporary,
            max_restarts: u32, period_ticks: u64, backoff: Backoff, stateful: bool }

supervisor loop:
  for spec in children: spawn(spec); register service id; NotifyOnExit(child)
  loop {
     msg = recv()                       // death notification {watched, reason}
     spec = lookup(msg.watched)
     if should_restart(spec.policy, msg.reason):
        if within_intensity(spec):       // sliding window of restart timestamps
           sleep(backoff(spec))
           new = spawn_from_path(spec.path)
           if spec.stateful: restore_state(new)   // via state_stash key = spec.name
           rebind_service(spec.name, new)
           NotifyOnExit(new)
        else:
           escalate(spec)                // log + (critical? SBI reboot : drop)
  }
```
Restart strategies (one_for_one first; rest_for_one/one_for_all later if dependency graph
demands it — YAGNI: start with one_for_one).

## Related Code Files
**Modify**
- `cells/.../init` (or a dedicated `cells/services/supervisor`) — supervisor logic.
- Child-spec config: compiled-in table first; optional `/etc/supervisor.toml` via VFS later.

**Create**
- `cells/services/supervisor/src/main.rs` (+ `Cargo.toml`, linker, manifest) IF splitting from
  init. Decide: extend init vs new cell. Recommend **new cell** for separation; init spawns it
  first and grants it the supervisor capability.

## Implementation Steps
1. Decide host: new `cells/services/supervisor` cell (recommended) spawned by init with the
   supervisor capability + `SpawnCap`. Add manifest declaring needed caps.
2. Define `ChildSpec` + a static initial child list (vfs, net — the critical services).
3. Spawn children, register service ids, subscribe `NotifyOnExit`.
4. Implement the death-handling loop with policy classification.
5. Implement restart-intensity sliding window + exponential backoff (tick-based; no
   `Date.now`-style host time — use kernel monotonic ticks via syscall).
6. Implement escalation: log always; for `critical` children, `system_reset` via a privileged
   path (or signal kernel to reboot).
7. Wire state recovery via `state_stash` for `stateful` children.
8. Build cells; boot QEMU; kill a child; observe automatic restart + reconnect.

## Todo List
- [ ] Supervisor cell scaffold (manifest, caps, spawned by init)
- [ ] `ChildSpec` + initial critical-child list (vfs, net)
- [ ] Spawn + service-register + NotifyOnExit per child
- [ ] Policy classification (permanent/transient/temporary)
- [ ] Restart-intensity window + backoff
- [ ] Escalation (give-up vs reboot)
- [ ] Stateful recovery via state_stash
- [ ] Test: `kill vfs` → auto-restart → `cat`/`vcat` works again without reboot
- [ ] Test: crash-storm child → intensity breach → escalation fires (not infinite loop)

## Success Criteria
- Killing the VFS service cell → supervisor restarts it → filesystem ops work again, **no
  manual respawn, no reboot**, verified live in QEMU.
- A child that crashes immediately on start trips the intensity limit and escalates instead of
  looping forever (visible in audit/log).
- A `temporary` child that exits cleanly is **not** restarted.

## Risk Assessment
- **Supervisor is a single point of failure (High).** If the supervisor dies, nothing
  restarts children. *Mitigation:* keep it tiny and `forbid(unsafe)`; let the kernel
  detect supervisor death and reboot (supervisor is `permanent` + critical → kernel-level
  fallback). Document this as the one cell whose death = reboot.
- **Restart storm masking a real bug (Med).** Auto-restart can hide a crashing component.
  *Mitigation:* intensity limit + loud audit; escalation after N failures.
- **State recovery restoring corrupt state → re-crash loop (Med).** *Mitigation:* on repeated
  fault after restore, restart WITHOUT state (cold) before giving up.

## Security Considerations
- Supervisor holds powerful caps (spawn, service rebind, possibly reboot). It must be a
  signed/first-party Tier-1 cell. Child specs must come from a trusted source (compiled-in or
  signed config) — an attacker editing child specs could spawn arbitrary cells.

## Next Steps
- With recovery working, Phase 05 ensures restarts don't leak memory over long uptimes.
