---
title: "Dossier 1 — Trust/Spawn open-question verdicts (P-TRUST + Supervisory P00)"
description: "Adjudicates every open question left in the P-TRUST spec (§7) and the Supervisory migration plan (§Open Questions) so both can cook without re-deriving. Analysis-only (Mythos window)."
status: verdicts-final
window: mythos-analysis-only (expires 2026-07-14)
created: 2026-07-12
inputs:
  - .agents/reports/spec-260712-loader-trust-model-repair.md
  - .agents/260712-1100-loader-trust-repair/plan.md
  - .agents/260712-0800-supervisory-cell-migration/plan.md
  - kernel/src/main.rs (init spawn, snapshot try_restore gate)
  - kernel/src/snapshot.rs (restore path)
---

# Dossier 1 — Trust / Spawn verdicts

Both plans are already design-complete; the analysis value left is **closing the
open questions** so no re-derivation happens at cook time. Verdicts below are
grounded in code re-checked at HEAD 2026-07-12. Nothing here is coded — this is
the decision record the two plans reference.

---

## Part A — P-TRUST (`260712-1100`) open questions (spec §7)

### A1. `TotalCaps` new type vs extend `CapSet` in place — **VERDICT: extend `CapSet`**

The spec left this as "implementor's call, both kernel-internal (no Law 1)." It is
not neutral. Extend `CapSet` in place, do **not** add a sibling `TotalCaps`:

- The whole point of §2.2 Option A (chosen over path-identity B) is that the
  intersection becomes **total** — *one* ceiling governs *every* channel. A second
  cap type re-introduces the exact split the fix removes: code would have to
  remember which caps live in `CapSet` and which in `TotalCaps`, and every
  `intersect`/`apply_to`/`of_task` site would juggle two objects. That is the
  divergence risk §3 warns about, re-created inside one struct.
- Cost of extending in place is bounded and enumerable: `ALL`, `EMPTY`, `of_task`,
  `from_manifest`, `intersect`, `apply_to`, + 2 unit tests (spec §7.1 lists them).
  All kernel-internal, no Law 1 (spec §4 confirms `CapSet` is not an ABI type).
- Snapshot/hotswap serialize `CapSet` by value (plan risk row 2) — a single struct
  keeps that one serialization site correct; two structs means two.

**Guidance for the cook phase:** add `pcie_driver: bool`, `platform: bool`,
`supervisor: bool` fields and fold the cell-store region into the existing
`block_regions` bitmask (bit `0b1000` already exists per spec §2.2). Keep the
`PlatformCap` singleton latch downstream of the ceiling gate (spec §2.2 last bullet).

### A2. Does `sys_spawn_from_path` (237) share the hole? — **VERDICT: fold applies uniformly; no separate fix; add one init-path regression assertion**

Confirmed lower-risk than `from_elf`: `sys_spawn_from_path` resolves bytes via the
kernel early loader over image-curated VIFS1 paths, so the bytes at `/bin/nvme`
*are* the real nvme (spec §7.2). The exploit in §1.3 needs *caller-supplied bytes*,
which only `from_elf` (238) gives. **But the fix must not special-case this** — the
folded ceiling intersection runs on both entry points identically, which is
correct-by-construction. The only action: the load-bearing regression check
(spec §6 row 1) must include a `from_path` spawn of `/bin/nvme` by **init**, to
prove init's Root ceiling still permits `pcie_driver` after the fold.

### A3. B1 + `SpawnReplacement` path semantics — **VERDICT: G2, out of P-TRUST scope; recorded as a sign-workflow requirement**

B1 (sign canonical path into payload) is G2 hardening (spec §2.3). When it lands,
a replacement ELF must be signed for the **frozen original's** canonical path
(net v2 signed for `/bin/net`), not the caller's path. This is a
`sign-cell.py`/`gen_disk.ps1` workflow requirement, not a kernel decision. **No
action in G1.** Flag for the pkg-publisher design (package-dist P04) so the
"sign-for-role-path" capability exists before B1 is switched on.

### A4. Policy anchor separation (A12) — **VERDICT: G2 prerequisite; assert distinctness before `/POLICY.BIN` carries revocation**

The `/POLICY.BIN` revocation set (spec §2.5 recommended) only becomes an authority
when channel (d) is built (G2). Before then, confirm the **cell-signing fleet key ≠
the policy-signing key** — otherwise a single key compromise both forges cells and
forges the revocation list that would catch them. **No action in G1** (revocation
not built); this is a gating assertion on the G2 channel-(d) phase, added here so
it is not forgotten.

**P-TRUST net:** all four open questions resolved without expanding G1 scope.
A1 is the only one that changes the cook instructions (extend, don't wrap). The
plan is cook-ready as written + the A1/A2 notes.

---

## Part B — Supervisory migration (`260712-0800`) open questions

### B1. Cap-ceiling mechanism shape (Phase 00) — **VERDICT: `sys_spawn_replacement(old_tid, path)`, kernel reads the frozen TCB's CapSet as ceiling. Reject the ceiling-param variant.**

This is the riskiest decision in the plan and P-TRUST changes its answer. Two sub-
questions were tangled:

1. *Where does cap authority live?* — **Kernel, non-negotiable.** Boundary law §1.2
   + P-TRUST §1.2: the ceiling is root-of-trust; a userspace supervisor must never
   name the ceiling. This rejects the plan's own "broad ceiling to supervisor"
   alternative (plan already leans reject — confirmed).

2. *What is the syscall shape?* — **`sys_spawn_replacement(old_tid, path)`.** The
   kernel looks up `old_tid` (which is `FROZEN`), reads its `CapSet` live from the
   still-frozen TCB, and uses that as the `Spawner::Ceiling`. The supervisor passes
   only *which cell* to replace and *what bytes/path* — never the caps. Reject a
   `frozen_ceiling: CapSet` parameter on `sys_spawn_from_path`: passing a CapSet
   from userspace is exactly the authority-naming the boundary law forbids, even if
   the kernel intersects it — the supervisor could pass a *broader* set and, absent
   P-TRUST, some channel might honor it. With `old_tid`, the ceiling is
   kernel-derived and unforgeable.

**Why P-TRUST makes this clean:** once path-caps fold into `CapSet` (P-TRUST §2.2),
"read the frozen cell's `CapSet`" automatically clamps `PcieDriverCap`/
`SupervisorCap`/cell-store too — Phase 00 no longer needs its own path-cap-clamp
reasoning (spec §3 row: P00's invariant becomes correct-by-construction). **Phase 00
must land P-TRUST first** and then is a thin syscall + a TCB read.

*Law 1:* `sys_spawn_replacement` is a new syscall number in `libs/api` → 2× user
confirm. The `old_tid`-based shape is what should be confirmed (not the
CapSet-param shape).

### B2. ResumeCell drain param vs new syscall (Phase 01) — **VERDICT: fold `drain_from: Option<Tid>` into `sys_resume_cell`**

Confirmed the plan's lean. The boundary law budgets 3 lifecycle primitives
(freeze/resume/kill); a dedicated `sys_transfer_pending` adds a 4th mechanism
syscall for what is logically "resume, and bring the frozen predecessor's mailbox
with you." Fold it. `drain_from: Option<Tid>` (None = plain resume, Some = drain
old→new before unfreeze) keeps the primitive count and makes ordered delivery
atomic with the unfreeze (no window where the replacement is runnable but its
inbox is empty). *Law 1:* param change to an existing syscall signature → 2× user
confirm; the `Option<Tid>` shape is the one to confirm.

### B3. Snapshot warm-boot restore liveness — **VERDICT: dormant on every CI/QEMU target; live only on an untested real-MMC board. Phase 03 migrates trigger authority ONLY; the restore path stays untouched kernel mechanism.**

Verified in code, not assumed:
- `try_restore()` is gated `#[cfg(any(riscv64, aarch64))]` and reads via
  `block::read_sector(SNAPSHOT_BASE_LBA, …)` (`snapshot.rs:190`).
- On QEMU, `block_device()` is `NullBlock` → `read_sector` errors → `"[snapshot] no
  block device → cold boot"` (`snapshot.rs:191`). So **warm-boot restore never runs
  on any QEMU/CI arch** post-G2-loader-redesign (the real disk is the virtio-blk
  Cell, which is not up at `try_restore` time — boot-order, pre-cell).
- It is a live feature *only* on a real board whose `block_device()` routes to
  in-kernel MMC — which the tracked tech-debt itself says has no QEMU coverage and
  is pending real-board test.

**Consequence for Phase 03:** the migration is *only* re-gating the snapshot
**trigger** (syscall 420: SpawnCap → SupervisorCap) and adding the supervisor's
`SnapshotRequest` handler. The **save** mechanism (frame-walk + CRC + disk write)
and the **restore** path (`try_restore`, boot-only, pre-cell, all-RAM overwrite)
are irreducible kernel mechanism and **do not move** — consistent with the plan's
"net snapshot.rs shrink ≈ 0 LOC." Phase 03 does **not** need a kernel
snapshot-partition reader independent of the block Cell, because restore is already
degraded to no-op on the only tested targets; do not re-plumb it. If/when a
real-MMC board becomes a supported target, warm-boot restore is a *separate*
follow-up (kernel keeps a minimal snapshot-partition reader) — file it, don't build
it in this migration.

### B4. init-death fallback (Phase 04) — **VERDICT: kernel spawns init from embedded ELF at boot with `is_critical=true`; there is NO runtime init-respawn loop → the fallback is panic-reboot. The supervisor-of-supervisor claim holds via the reboot root, not a respawn root.**

Verified: `main.rs:535` spawns init via `spawn_from_mem(INIT_ELF, "init", CellId(1))`
and sets `is_critical=true` (`main.rs:557`) so neither the Supervisor Cell nor
anything in userspace can freeze/kill it. The visible failure handling
(`main.rs:562`, probe 'F') is *spawn-time* failure, not *runtime death*. No
`NotifyOnExit`-on-init loop or explicit init-respawn path is present in `main.rs`.
Therefore init death falls through to the reboot-on-panic mechanism (reliability
P00-03), i.e. **the kernel re-roots the whole tree by rebooting, not by respawning
init in place.** This is a legitimate root (matches the "kernel keeps the minimal
root so the tree can always be re-rooted" framing) — but the plan's phrasing
("kernel respawns init from embedded / panic-reboot — verify which") should be
corrected to: **panic-reboot is the mechanism; there is no in-place init respawn.**
Phase 04's boundary-law claim is grounded on that. *Action:* the docs edit in
Phase 04 should state this precisely rather than leaving it as an either/or.

### B5. hotswap-demo cap coverage — **VERDICT: the existing e2e cannot catch the Phase 00 regression; add a privileged-service swap to the test matrix. This is a gating requirement for Phase 00 completeness, not optional.**

The demo cells are unprivileged, so `SpawnReplacement`'s cap-ceiling clamp is never
exercised — a Phase 00 that silently over- or under-grants would pass the current
suite green. The Phase 00 success criterion "a privileged service (e.g. `net`)
hotswapped retains its `NetworkCap`" (plan §Success Criteria) **requires** a test
that actually swaps a cap-holding cell. Two options, in order of preference:

1. **Swap `net` (or another real privileged service) in the e2e** and assert it
   still answers on `service::NET` + retains `NetworkCap` after the swap. Highest
   fidelity — exercises the real frozen-TCB CapSet read.
2. If swapping a live service in CI is too heavy, add a **minimal demo cell that
   declares one cap** (e.g. `network=true`) and assert (a) it retains it across a
   same-role swap and (b) a `SpawnReplacement` to a `/bin/nvme` path yields **no**
   `PcieDriverCap` (the P-TRUST negative test, re-used at the supervisor layer).

Either way the matrix must include one cap-bearing swap before Phase 00 is called
done. Recommend option 1 if `net` swap is stable, else option 2.

---

## Sequencing consequence (both plans)

P-TRUST is the root; its A1 verdict (extend `CapSet`) is what makes Supervisory B1
(`sys_spawn_replacement` reads frozen `CapSet`) a thin, correct-by-construction
change. Land order is unchanged from the plans: **P-TRUST → Supervisory P00 → P01 →
(P02 ∥ P03) → P04**. The only additions this dossier makes are: extend-not-wrap
(A1), the `old_tid` syscall shape (B1), fold drain (B2), restore-is-dormant so don't
re-plumb (B3), panic-reboot is the fallback so document it precisely (B4), and a
cap-bearing swap test is mandatory (B5).
