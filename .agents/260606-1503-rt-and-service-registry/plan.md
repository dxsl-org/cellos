# Plan: Stable Service-ID Registry + P06 RT Hardening

**Created:** 2026-06-06  **Track:** Reliability ("không chết") — close recovery loop + add RT guarantees
**Branch:** main

Two reliability features, sequenced. **Phase 01 first** (higher never-die value, tractable,
closes the recovery loop the supervisor opened). **Phase 02** is data-gated and lands after.

## Why these

- **Service-ID registry:** the supervisor (NotifyOnExit, shipped) respawns a dead service, but
  it gets a NEW tid. Clients address services by raw tid (`config_client.rs`), so every client
  breaks after a restart. Recovery without reconnection is half a recovery. A stable
  service-id → current-tid registry closes the loop.
- **P06 RT:** priority-preemptive scheduling already exists (3 levels, zero-latency RT preempt,
  RT watchdog, deadline sweep). Missing: CPU-budget/period accounting + deadline-miss detection.
  Full EDF/WCET is academic on QEMU TCG (no cycle-accurate timing) — so the tractable slice is
  deadline-miss *detection* + budget *accounting*, not a new scheduler.

## Phase 01 — Stable Service-ID Registry  (status: PENDING Law-1 confirm)
Detail: [phase-01-service-registry.md](phase-01-service-registry.md)

Kernel-owned `service_id(u16) → current_tid` map, managed by the trusted supervisor.
- **libs/api (Law 1):** `RegisterService = 205`, `LookupService = 206`; `pub mod service`
  well-known IDs (VFS=1, NET=2, INPUT=3, CONFIG=4, COMPOSITOR=5).
- **kernel:** `service_registry` module (`Spinlock<BTreeMap<u16,usize>>` + force_unlock);
  RegisterService gated by `SpawnCap` (supervisor owns the namespace); LookupService open;
  `exit_task` clears stale entries for a dead tid (no client ever gets a dead tid).
- **ostd:** `sys_register_service(id, tid)`, `sys_lookup_service(id) -> Option<tid>`.
- **init:** register each service after spawn; re-register after respawn.
- **proof:** wire one client (shell→config or a vfs client) to resolve via lookup + survive a
  service restart. Build + boot-verify: kill a service, client reconnects to the new instance.

## Phase 02 — P06 RT Hardening  (status: PENDING; data-gated)
Detail: [phase-02-rt-hardening.md](phase-02-rt-hardening.md)

Build on existing primitives (run_ticks, deadline sweep, RT watchdog). NOT a new scheduler.
- **Deadline-miss detection:** the RecvTimeout deadline sweep already wakes a late Recv with
  Timeout — add a per-cell *deadline-miss counter* + audit event so a control loop's missed
  cycle is observable (currently silent).
- **CPU-budget accounting:** reuse `run_ticks` (already counts non-yielding ticks). Expose a
  per-cell budget (declared at spawn alongside priority) and an audit event when an RT cell's
  run_ticks crosses budget *before* the hard watchdog kill — early warning vs. hard kill.
- **`pending_future` timeout** (the gap found while scoping P05): a cell stuck `Polling` forever
  if a future never readies. Add a deadline to `pending_future` and sweep it like RecvTimeout.
- **libs/api (Law 1, if budget is declared):** likely extend `SpawnPinned` args or add a
  `SetSchedParams`. Decide after Phase 01. QEMU-TCG caveat documented.

## Law 1 gate
Both phases touch `libs/api`. Per the 8 Laws, each needs **2× user confirmation** before editing
`libs/api`. Phase 01 surface is spelled out above and in phase-01; awaiting confirm #1 then #2.

## Sequencing
01 (full impl + boot-verify) → 02 (design locked by 01's outcome, then impl). Commit per slice.
