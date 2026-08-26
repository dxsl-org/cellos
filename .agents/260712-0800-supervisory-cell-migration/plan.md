---
title: "Supervisory Cell Migration — Exile Hotswap/Snapshot Orchestration from Kernel"
description: "Cut hotswap + snapshot orchestration off the kernel path onto the existing Supervisory Trusted Cell, closing correctness gaps, then delete the dead kernel orchestrator."
status: complete (phases 00-04 complete; post-plan followups recorded)
priority: P2
effort: 5 phases (~14-19 dev-days)
branch: main
tags: [kernel-boundary, supervisor-cell, hotswap, snapshot, reliability, sas, lbi, migration]
created: 2026-07-12
---

# Supervisory Cell Migration

> **Portfolio correction (D39, 2026-08-01):** P-TRUST landed in `721e1f6f`; Phase 00 is
> no longer blocked on that plan. Supervisory migration is complete; this program remains
> queued until Midori runtime closure for phases 07/08, except a separately authorized P0
> security/CI repair.

> Ratified law: `docs/specs/15-kernel-boundary.md` §3.2 (2026-06-23). Kernel keeps only the
> freeze/resume/kill mechanisms; ALL orchestration (hotswap sequencing, snapshot trigger,
> restart policy) moves to a trusted Supervisory Cell. *"Orchestration is policy, not mechanism."*

## Reframing (verified 2026-07-12 — this is NOT a greenfield migration)

The blacklist entry reads "move ~400 LOC hotswap + ~350 LOC snapshot to a Supervisory Cell,"
which implies nothing has moved. **Re-grepping the tree shows the migration is ~60% done and
the remaining 40% is a cutover + two correctness gaps + a delete.** The kernel mechanism primitives
and a working Supervisor Cell already exist; the kernel orchestrator was never removed and is
**still the live path**. This plan finishes and cuts over, it does not build from scratch.

| What the law says to build | Actual state (file:line) |
|----------------------------|--------------------------|
| Kernel keeps `sys_freeze_cell`/`sys_resume_cell`/`sys_kill_cell` | **DONE** — `FreezeCell=413`,`ResumeCell=414`,`KillCell=415`,`QueryHotswapReady=419`, all `SupervisorCap`-gated, allowlist bit 49 (`libs/api/src/abi/syscall.rs`; dispatch `kernel/src/task/syscall.rs:2437-2497`) |
| A Supervisory Cell | **EXISTS** — `cells/services/supervisor/src/{main.rs,hotswap.rs,protocol.rs,error.rs}`; spawned by init `/bin/supervisor`, Permanent policy, registers `service::SUPERVISOR=11` (`cells/tools/init/src/main.rs:69,85,100`) |
| `SupervisorCap` + supervisor-of-supervisor guard | **DONE** — `SupervisorCap` `cap.rs:59` (path-grant `loader.rs:315-317`); `is_critical` TCB flag `tcb.rs:314` blocks freeze/kill of init/kernel cells |
| Move hotswap orchestration out of kernel | **DONE** — `/bin/hotswap` sends Supervisor IPC; the legacy kernel `hotswap()` path and `HotSwap=400` retirement are complete in Phase 04 |
| Move snapshot trigger authority out of kernel | **DONE FOR TRIGGER, MECHANISM STAYS** — `Snapshot=420` is SupervisorCap-gated and shell routes through Supervisor IPC; QEMU proves NullBlock/unavailable and direct non-supervisor denial; real MMC save/restore remains host-gated. |

## Verified Baseline (re-grepped 2026-07-12 — do not trust without re-checking)

| Fact | Value | Source |
|------|-------|--------|
| Kernel orchestrator (to delete) | `hotswap()` + envelope builders + poll loops | `kernel/src/cell/hotswap.rs:188-508` |
| Kernel mechanism (to keep) | `FROZEN` set, `set_task_frozen`, `unfreeze_task`, `exit_task_internal`, `set_task_hotswap_ready`, `force_unlock_locks` | `hotswap.rs:60-186` |
| Mechanism syscalls | 413/414/415/419 → internal fns above | `syscall.rs:2437-2497` |
| `HotSwap=400` gate | **SpawnCap** (`caller_has_spawn`, current `kernel/src/task/syscall.rs:4051`) — the path to retire/reserve in Phase 04 | |
| `HotSwapReady=401` | new cell sets `Task.hotswap_ready`; keep (cell-facing mechanism) | `hotswap.rs:103`, demo uses `sys_hotswap_ready` `cells/demos/hotswap-demo-v2/src/main.rs:29` |
| State stash (rendezvous) | `stash`/`restore`/`remove`, global-by-key, kernel heap `BTreeMap`, MAX_ENTRIES=64, MAX_STASH_LEN=1MB | `kernel/src/cell/state_stash.rs`; ostd `sys_state_stash/restore/clear`=410/411/412 |
| Cap ceiling on kernel hotswap spawn | kernel passes **replaced cell's CapSet** as ceiling | `hotswap.rs:326-334` + `loader.rs:243` ("HotSwap passes the replaced cell's caps as the ceiling") |
| Cap ceiling on supervisor spawn | `sys_spawn_from_path` → `Spawner::User(sup_tid)` → `requested ∩ supervisor caps` | `loader.rs:254-260` |
| Supervisor's own caps | SpawnCap + SupervisorCap only (`declare_manifest! spawn=true`) | `supervisor/src/main.rs:89-92` |
| pending_msgs drain (message-loss guard) | supervisor cutover uses PauseService + Frozen FIFO + `commit_hotswap_barrier`; old kernel path is legacy fallback only until Phase 04 | phase files 01/02; current Phase 04 re-grep required |
| Snapshot save mechanism | frame-walk + CRC + disk write; needs frame allocator + privileged frame reads | `snapshot.rs:91-174` |
| Snapshot restore | `try_restore()` at boot, pre-cell, privileged all-RAM overwrite | `snapshot.rs:195`; called `main.rs:532-535` |
| Block routing at boot | `block_device()` → `NullBlock` on QEMU (fails), MMC on real board; real disk = virtio-blk Cell via IPC | `kernel/src/task/drivers/block.rs:8-56` |
| Highest syscall / free gap | 422; `HotSwap=400` is retired/reserved by Phase 04, not reused | current Phase 04 audit |
| Test asset | `tests/integration/tests/hotswap-smoke.rs` — spawns demo-v1/v2 + key-derivation unit tests; **no automated end-to-end swap** | line 5 note |

## The mechanism-vs-policy split (the deliverable of this migration)

```
KERNEL KEEPS (mechanism — privilege / root-of-trust / pre-cell):
  • FROZEN set + IPC-queue intercept          (scheduler/IPC routing)
  • set_task_frozen / unfreeze_task            → FreezeCell(413)/ResumeCell(414)
  • exit_task_internal (Frozen-kill bypass)    → KillCell(415)
  • set_task_hotswap_ready flag                → HotSwapReady(401) set / QueryHotswapReady(419) read
  • pending_msgs drain + old-ingress closure   → NEW: atomic ResumeCell cutover barrier (Phase 01)
  • CapSet of frozen cell as replacement ceiling → NEW: kernel is the only cap authority (Phase 00)
  • snapshot frame-walk + CRC + disk write     → sys_snapshot SAVE mechanism (SupervisorCap-gated by Phase 03)
  • snapshot try_restore()                     → boot-only, irreducible (pre-cell, all-RAM overwrite)
  • init respawn / panic-reboot                → root of the supervision tree

SUPERVISOR CELL OWNS (policy — sequencing / triggers / decisions):
  • 5-phase hotswap sequence                   (already in supervisor/src/hotswap.rs)
  • Snapshot trigger IPC envelope              → NEW opcode-only SnapshotRequest (Phase 03)
  • stash/ready poll loops with timeout        (already there for hotswap)
  • WHEN to snapshot (trigger authority)       → NEW SnapshotRequest handler (Phase 03)
  • service re-registration on commit          (already there)
```

**Honest LOC correction to the law:** hotswap splits ~110 LOC kept / ~290 deleted from the kernel
(not "all 400 moved" — 400 LOC of orchestration already lives in the cell; the kernel copy is deleted).
**Snapshot does NOT move ~350 LOC:** its state machine is irreducible kernel mechanism (serialize
needs privileged all-frame access; `try_restore` runs at boot before any cell exists). Only the
*trigger authority* is policy and moves. Net kernel snapshot.rs shrink ≈ 0 LOC; the change is
adding a real SupervisorCap dispatch gate to `Snapshot=420` and routing shell through the supervisor.
This is a real deviation from the
blacklist one-liner and is called out so the boundary-law table can be corrected.

## Supervisor-of-supervisor answer (chicken-and-egg)

Three-tier tree, terminating in the kernel — no infinite regress:

1. **Kernel** spawns `init` from the embedded VIFS1 ELF (`main.rs:534`, CellId 1, `is_critical=true`).
   If init dies the kernel is the fallback (respawn-from-embedded / panic-reboot — see Phase 04
   open item to *verify* which). This mirrors the NullBlock graceful-degradation pattern: the kernel
   keeps the minimal root so the tree can always be re-rooted.
2. **init** (Permanent policy, NotifyOnExit=204 loop) supervises the Supervisor Cell + all services.
   If the supervisor crashes, init restarts it. **Frozen targets survive** because freeze state lives
   in the kernel, not the supervisor (`main.rs` comment; `sys_freeze_cell` persists across respawn).
   init retains SupervisorCap-adjacent authority to unfreeze orphaned targets (`loader.rs:298-300`).
3. **Supervisor Cell** orchestrates hotswap/snapshot for ordinary + privileged service cells.
   `is_critical` (`tcb.rs:314`) prevents a buggy/hostile supervisor from freezing/killing init or
   kernel cells — the restart root cannot be disabled from userspace.

The supervisor is a **disk cell-store** cell (`gen_disk.ps1:186,438`), spawned post-VFS by init — it
does **not** participate in early-boot restart (init does), so VIFS1 ramdisk placement is unnecessary.

## Dependency Graph

```
Phase 00 (cap-ceiling preservation — kernel spawns replacement with frozen cell's CapSet)  [GATE: Law 1 ×2]
    │   RISKIEST — live security/correctness gap in the shipped supervisor path
    ▼
Phase 01 (atomic message drain + old-ingress closure in ResumeCell barrier)                [GATE: Law 1 ×2]
    │
    ▼
Phase 02 (cut over hotswap CLI + shell → service::SUPERVISOR IPC; add e2e swap test)       [needs 00+01 green]
    │
    ├──> Phase 03 (snapshot trigger → supervisor: regate 420, SnapshotRequest handler)     [independent of 02; needs 00]
    │
    ▼
Phase 04 (kernel cleanup: delete hotswap() orchestrator + HotSwap=400; shrink to mechanism)[complete; followups recorded]
```

Phases 02 and 03 are parallel-eligible (disjoint files: 02 = CLI/shell hotswap + supervisor;
03 = shell snapshot + supervisor snapshot handler + syscall gate). Phase 04 deletes only after both
land behind a full green boot on all three arches.

## File-Ownership Map (no two parallel phases touch the same file)

| Phase | Owns (create/modify) |
|-------|----------------------|
| 00 | `libs/api/src/abi/syscall.rs` (new syscall/param), `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs` (freeze records ceiling / spawn-replacement arm), `kernel/src/cell/hotswap.rs` (freeze stores CapSet), `cells/services/supervisor/src/hotswap.rs` (use replacement spawn) |
| 01 | `libs/api/src/abi/syscall.rs` (ResumeCell ABI docs only; preserve 414/bit49), `libs/ostd/src/syscall.rs` (commit wrapper), `kernel/src/task/{syscall.rs,tcb.rs}` (ResumeCell barrier + old-ingress-closed state), `kernel/src/task.rs` (reject post-cutover cached sends), `kernel/src/cell/{hotswap.rs,service_registry.rs}` (FIFO transfer + paused->active compare commit), `cells/services/supervisor/src/hotswap.rs` (barrier before kill), `tests/integration/tests/hotswap-smoke.rs`, `cells/tests/bench/src/scenarios/hotswap_supervisor.rs` |
| 02 | `cells/tools/sys-tools/src/bin/hotswap.rs` (→ IPC), `cells/services/supervisor/src/{main.rs,protocol.rs}`, `tests/integration/tests/hotswap-smoke.rs` (e2e scenario) |
| 03 | `cells/tools/shell/src/executor.rs` (snapshot built-in → IPC), `cells/services/supervisor/src/{main.rs,protocol.rs,snapshot.rs (new)}`, `kernel/src/task/syscall.rs` (regate Snapshot=420 → SupervisorCap) |
| 04 | DELETE `kernel/src/cell/hotswap.rs` orchestration (`hotswap()`, envelope builders, poll loops); remove `HotSwap=400` dispatch + enum variant + SpawnCap gate; `docs/specs/15-kernel-boundary.md` (correct LOC table) |

> Conflict watch: 00, 01, 03, 04 all touch `kernel/src/task/syscall.rs` — serialize their merges;
> the arms are logically disjoint (freeze/spawn vs resume vs snapshot-gate vs delete). 00 and 01
> both touch `kernel/src/cell/hotswap.rs` — 01 depends on 00, so they are sequential, not parallel.

## Phases

| # | File | Title | Status | Effort | Risk |
|---|------|-------|--------|--------|------|
| 00 | [phase-00-cap-ceiling-preservation.md](phase-00-cap-ceiling-preservation.md) | Replacement inherits frozen cell's CapSet | complete | 3-4d | **HIGH** (Law 1; security correctness; kernel cap authority) |
| 01 | [phase-01-message-queue-drain.md](phase-01-message-queue-drain.md) | Message-queue drain in supervisor path | complete | 2-3d | MED (Law 1 if new param; IPC ordering) |
| 02 | [phase-02-cutover-hotswap-cli.md](phase-02-cutover-hotswap-cli.md) | Cut CLI/shell over to Supervisor IPC + e2e test | complete | 3-4d | MED (behavior change; test coverage gap) |
| 03 | [phase-03-snapshot-trigger.md](phase-03-snapshot-trigger.md) | Snapshot trigger authority → supervisor | complete | 2-3d | HIGH (missing capability gate; QEMU NullBlock proof only) |
| 04 | [phase-04-kernel-cleanup.md](phase-04-kernel-cleanup.md) | Retire syscall 400; delete kernel orchestrator; shrink to mechanism | complete | 2-3d | HIGH (Law 1 ABI retirement; destructive; requires confirmation checkpoint) |

## Cross-Cutting Constraints

- **Law 1**: any `libs/api/` edit (new syscall number, ResumeCell register semantics, Snapshot regate, syscall 400 retirement) needs 2× user confirm. Phases 00, 01, 03, 04.
- **Law 2**: IPC handlers use `Box<[u8]>`, never `&mut [u8]`.
- **Law 4**: Supervisor Cell stays `#![forbid(unsafe_code)]` — everything via syscalls.
- **Law 5**: no `mod.rs`; `foo.rs` parallel to `foo/`.
- **Law 8**: supervisor impls `Drop`/`Shutdown` cleanup; frozen targets are kernel-owned so they survive.
- **SAS frame-identity** ([[project-sas-frame-identity-invariant]]): state transfer uses the global-by-key stash (kernel heap), never grant-mapped cell frames; no unmap-on-free. No change needed but must not regress.
- **Never-die**: cell-death → init-restart path stays green THROUGHOUT. Every phase boots + passes the reliability suite before merge.
- **Multi-arch**: riscv64 main suite, aarch64 7/7, x86_64 13/13 stay green each phase.

## Global Rollback Strategy

Phases 00-03 added/redirected, and Phase 04 completed the destructive cleanup. The kernel
`hotswap()` path (`HotSwap=400`) is retired; rollback of the cleanup commit restores it if
needed. Per-phase rollback:

- 00: revert the new syscall/param; supervisor falls back to `sys_spawn_from_path` (privileged-service hotswap stays broken but ordinary swap works — the pre-plan state).
- 01: revert drain param; message loss on swap returns (pre-plan behavior, reliability §5 already flags it incomplete).
- 02: before Phase 04 only, revert CLI/shell to the legacy kernel path; after Phase 04, rollback requires reverting the Phase 04 retirement commit first.
- 03: revert shell `snapshot` IPC route, supervisor `OP_SNAPSHOT`, and the new SupervisorCap handler gate; on-disk format and restore path remain unchanged.
- 04: git revert the deletion commit; kernel orchestrator restored verbatim.

## Success Criteria (whole plan)

- [x] `hotswap <service> <elf>` CLI performs the swap via `service::SUPERVISOR` IPC (no `sys_hotswap` call anywhere in `cells/`)
- [x] A privileged SpawnCap test cell hotswapped via the supervisor **retains SpawnCap** (`[hotswap-demo-v2] SpawnCap retained`)
- [x] Buffered IPC to a frozen cell is delivered to the replacement in order (Phase 01 verified, closes reliability §5)
- [x] `snapshot` built-in routes through the supervisor; only `SupervisorCap` holders can enter the kernel snapshot handler; QEMU reports NullBlock/unavailable, real-MMC write/restore proof deferred
- [x] `kernel/src/cell/hotswap.rs` contains only mechanism (no public `hotswap()`, no kernel envelope/poll code); exact helper callers were re-grepped before deletion
- [x] `HotSwap=400` retired/reserved from `ViSyscall`; `from_number(400)` returns `Unknown`; `HotSwapReady=401` stays live
- [x] `docs/specs/15-kernel-boundary.md` §3.2 table updated with the true mechanism/policy LOC split
- [x] Phase 04 verification matrix green from fresh images: RV64 boot, `hotswap-smoke`, `launch-profile` snapshot authority, fresh `gen_disk.ps1`, and release-kernel builds; x86/AArch64 fresh boot lanes remain host-tooling-gated with the missing path bridge, and real MMC snapshot save/restore is host-gated
- [x] End-to-end swap: `hotswap-demo-v1` inc×5 → swap to v2 → `get` returns 5 (state preserved) — automated in hotswap-smoke

## Open Questions — RESOLVED 2026-07-12

> Adjudicated in `.agents/260712-1836-mythos-g123-analysis/dossier-1-trust-spawn-verdicts.md` (Part B).
> Verdicts below; originals kept for provenance.

1. **Cap-ceiling mechanism shape (Phase 00)** — **RESOLVED: `sys_spawn_replacement(old_tid, path)`.**
   The kernel looks up `old_tid` (FROZEN), reads its `CapSet` live from the still-frozen TCB, uses it as
   the `Spawner::Ceiling`. **Reject the `frozen_ceiling: CapSet` parameter variant** — passing a CapSet
   from userspace is the authority-naming the boundary law forbids (a compromised supervisor could pass a
   broader set). With `old_tid` the ceiling is kernel-derived + unforgeable. After P-TRUST folds path-caps
   into `CapSet`, "read the frozen cell's CapSet" clamps pcie/supervisor/cell-store for free → P00 becomes
   correct-by-construction. **Phase 00 depends on P-TRUST (.agents/260712-1100) landing first.** Law 1:
   confirm the `old_tid`-based syscall shape (not the CapSet-param shape).
2. **ResumeCell drain param vs new syscall (Phase 01)** — **REVISED: fold atomic cutover into
   `ResumeCell=414`.** `a1=0` keeps plain abort resume; `a1=old_tid,a2=service_id` means
   `a0=new_tid` and commits old-ingress closure + FIFO transfer + service publication in one kernel
   barrier. Keeps the 3-primitive budget. Law 1: confirm the register-semantics extension twice.
3. **Snapshot warm-boot restore** — **RESOLVED: DORMANT on every QEMU/CI arch, live only on an untested
   real-MMC board.** Verified: `try_restore()` (snapshot.rs:195) reads via `block::read_sector`; QEMU
   `block_device()` = `NullBlock` → errors → cold boot (snapshot.rs:198-200). So Phase 03 migrates the snapshot
   **trigger authority ONLY** (add SupervisorCap gate to 420 + opcode-only SnapshotRequest handler); the save
   mechanism + `try_restore` (boot-only, pre-cell) are irreducible kernel mechanism and DO NOT move
   (net snapshot.rs shrink ≈ 0). **Do NOT re-plumb a block-Cell-independent reader** — restore is already
   a no-op on tested targets. Real-MMC warm-boot is a separate future follow-up.
4. **init-death fallback (Phase 04)** — **RESOLVED: panic-reboot, NOT in-place respawn.** Verified:
   main.rs:535 spawns init from embedded ELF at boot with `is_critical=true` (main.rs:557); the only failure
   handling is spawn-time (probe 'F', main.rs:562); no `NotifyOnExit`-on-init loop exists. Init death falls
   through to reboot-on-panic (reliability P00-03). Phase 04's docs edit must state this precisely rather
   than "respawn or panic-reboot — verify which."
5. **hotswap-demo cap coverage** — **RESOLVED: mandatory, not optional.** The current unprivileged-demo e2e
   cannot catch a Phase 00 cap regression. Phase 00 is not done until the test matrix includes ONE cap-bearing
   swap: the privileged SpawnCap probe cell must show `[hotswap-demo-v2] SpawnCap retained`; the negative
   P-TRUST test (SpawnReplacement to `/bin/nvme` yields NO PcieDriverCap) stays re-used at the supervisor layer.
