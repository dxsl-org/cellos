# S6 handoff request — shared service runtime and wire modules (phase 12)

**Date:** 2026-11-13 · **Plan:** [plan.md](plan.md) · **Prior slice:** S5b Phase 11 closed 2026-11-12 (CP2 confirmed, merged to `main` at `f094e9df5`).

## 1. Scope (measured on `f094e9df5`)

| Surface | Today |
|---|---|
| Service loops | 10 hand-rolled receive loops (config, kms, ai, supervisor, compositor, vfs, net, input, silo, net-broker). Only supervisor uses `AppContext`. `service_entry!` has no user (`ostd::dispatch` was deleted in Phase 11). |
| Heartbeat | Only net, input and net-broker heartbeat. VFS, config, kms, ai, supervisor and compositor never do. |
| Attestation | ai reads no caller identity at all. The others each parse the attested tail themselves. |
| Owner-death reclaim | Implemented separately in VFS, net and compositor. |
| Wire literals | 28 files declare `const OP_*: u8`. The kernel hard-codes the supervisor's power request (`acpi/sci.rs:40`, `[0xAC, 0x00, 0x04, 0x02]`). Errors are bare `u8`, with collisions (net `0xFF` is both failure and EOF). |
| VFS → block driver | The masked receive has no timeout (`blk_router.rs:82`, `sys_recv(tid, …)`), so a frozen driver hangs VFS forever. |
| VFS trust store | `cells/services/vfs/src/trust.rs` (239 lines) and `trust/`; VFS calls up into input (`trust/first.rs`). |

**Already done elsewhere:** the plan's "multi-sector block requests" item. The multi-sector plan `261112-1035` shipped `OP_READ_RUN` (7 sectors per round trip) in all block drivers; its phase 03 also deleted the VFS fallback. Phase 12 does not redo it.

## 2. Staging: proposed

**Step 1 (now)** is read-only plus a document. It:
- inventories every service wire (opcodes, lengths, error bytes, every place each is declared);
- designs `libs/api/src/services/proto/<service>.rs` with values pinned to today's bytes;
- designs the `ostd::service::serve` hooks (`on_request`, `on_owner_death`, `on_idle`, built-in heartbeat and attestation) against the 10 loops;
- picks the trust store's destination;
- writes `cp1-service-wire.md`.

**Nothing in `libs/api`, `libs/ostd`, `cells/` or the kernel changes until you confirm CP1.**

**Steps 2–8** follow, one commit per step, migrations split per service:
2. Proto modules with byte tests; switch servers, clients, tools and the kernel constant; delete the literals.
3. `serve` built; config → kms → ai migrated; `service_entry!` deleted.
4. VFS: bounded block receive mapped to `BlockOutcome::Indeterminate`, heartbeat, `FsBackend` returning `Result<_, FsError>`.
5. supervisor, compositor and VFS migrated to `serve`.
6. Trust store moved; VFS stops calling a later service.
7. Net: `NetState` struct, one no-op probe type, migrated last with an x86 bench before/after.
8. CP2.

## 3. File set (file-bounded exception, after CP1)

- **Create:** `libs/api/src/services/proto/{vfs,net,supervisor,nic,tls,compositor,config,kms,ai}.rs`, `libs/ostd/src/service/serve.rs`, `cp1-service-wire.md`, `cp2-service-wire.md`, the frozen-block-driver x86 fixture.
- **Modify:**
  - `libs/api/src/services/{ipc.rs,mod}`, `libs/ostd/src/{service,runtime,tls}.rs`, `libs/ostd/src/clients/**`;
  - the service crates `config`, `kms`, `ai`, `supervisor`, `compositor`, `vfs`, `net`;
  - `cells/tools/sys-tools/src/bin/{hotswap,shutdown}.rs`, the NIC drivers' dispatch, `kernel/src/acpi/sci.rs`;
  - `docs/specs/{09-vfs,12-reliability,17-ipc-wire-contract}.md`.
- **Delete:** `service_entry!`, the per-service owner-watch copies, `cells/services/vfs/src/trust.rs` and `trust/` (moved).
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`.

## 4. Behaviour changes to expect

- **No wire byte moves.** Byte tests pin every opcode and error value to today's literal.
- **VFS stops hanging on a frozen driver.** It returns `Indeterminate` within the bound and keeps serving in-memory requests. A timed-out write is reported and never retried.
- **Every migrated service heartbeats and reads the attested caller.** ai becomes attested, so an unattested caller is refused.
- **Net migrates last,** with an x86 bench before and after.

## 5. Conflicts

- **The multi-sector / Chrome session** (`261112-1035`, phase 04 in progress) owns the hypervisor disk path and has recently touched VFS `blk_router/*` and the block drivers. Step 4 changes `blk_router` too: each commit is rebased on whatever that session has committed, and none of its uncommitted files is staged.
- **The UI session** (compositor, `ostd::display`): step 5 touches the compositor main loop only.

## 6. Questions to the owner

1. Grant S6 staged as in §2: step 1 (inventory + CP1) now, steps 2–8 only after your CP1 confirmation.
2. Where does the SSH trust store go: into **kms** (it already holds SSH key custody; no new cell), or into **its own cell**?
3. Are the multi-sector requests rightly treated as done by `261112-1035`, and so dropped from Phase 12?
