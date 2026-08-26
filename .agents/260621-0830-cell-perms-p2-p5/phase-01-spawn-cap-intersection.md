---
phase: 01
title: Spawn-time capability intersection (P2)
tier: thinking
depends: []
status: planned
---

# Phase 01 — Spawn-time capability intersection (P2)

> **Revised post-red-team (2026-06-21).** Key corrections: init grant is a DIRECT main.rs write, not
> via manifest (C1); HotSwap must thread a ceiling, not the `None` exempt branch (C2); Spinlock is
> non-reentrant — snapshot spawner caps in a dropped guard (M-lock); `from_manifest` must replicate
> the exact `block_regions` derivation (M-block_regions).

## Context Links
- Design: [research-cell-security-permissions.md](../../docs/research/research-cell-security-permissions.md) §2.4
- Roadmap: §G.2 "Spawn-time cap intersection (delegation)"
- Touch points: `kernel/src/loader.rs`, `kernel/src/task/syscall.rs` (SpawnFromPath + HotSwap), `kernel/src/cell/hotswap.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/cap.rs`, `kernel/src/main.rs`

## Overview
Enforce `effective = child_manifest_caps ∩ spawner_caps` for every **user-cell-initiated** spawn so a
cell cannot grant a child a cap it does not hold (monotonic downgrade). `init` is the root authority
(`CapSet::ALL`, granted directly in main.rs). HotSwap inherits the replaced cell's caps as a ceiling.

## Key Insights (red-team-corrected)
- **C1 — init does NOT route through `spawn_from_path`.** It is spawned by
  `task::spawn_from_mem` ([main.rs:455](../../kernel/src/main.rs#L455)) with caps granted manually at
  [main.rs:462](../../kernel/src/main.rs#L462). So expanding init's `declare_manifest!` is a **no-op**
  — the manifest is never read. **Grant `CapSet::ALL` by direct TCB write in that main.rs block.** Do
  NOT route init through the loader (if it had block_io there, loader.rs:185 would re-point the VFS
  fast-IPC handler to init — a bug).
- **C2 — the choke point is plural.** `spawn_from_path` is called by (a) the SpawnFromPath syscall
  (`caller_id` in scope, unforgeable — hart-local), and (b) **HotSwap**
  ([hotswap.rs:100](../../kernel/src/cell/hotswap.rs#L100), kernel-internal, no user caller). HotSwap
  must NOT use the `None` root-exempt branch (that grants full manifest caps with no intersection/
  policy — a laundering hole). It must pass the **replaced cell's CapSet** as the ceiling.
- **Lock model (M-lock):** there is no single SCHEDULER lock held across the grant flow; the Spinlock
  is **non-reentrant** ([sync.rs:25](../../kernel/src/sync.rs#L25)). `caps_of(spawner)` must snapshot
  into a `CapSet` in its OWN `SCHEDULER.lock()` block and **drop the guard** before the child-mutation
  block re-locks. Never read the spawner inside the child guard via a second `.lock()`.
- The existing privilege gate (loader.rs ~L90) already blocks non-`/bin/` user cells from declaring
  caps; P2's value is constraining `/bin` cells delegating to children + making downgrade explicit.

## Requirements
**Functional**
- F1: `spawn_from_path(path, spawner: Spawner)` where `Spawner = Root | User(tid) | Ceiling(CapSet)`.
  `Root` ⇒ full manifest (boot only). `User(tid)` ⇒ `manifest ∩ caps_of(tid)`. `Ceiling(caps)` ⇒
  `manifest ∩ caps` (HotSwap, snapshot-internal re-spawn).
- F2: init granted `CapSet::ALL` via **direct write in main.rs** (NOT manifest, NOT loader). Add a
  boot-log line dumping init's granted CapSet so a regression is visible in the smoke test.
- F3: Intersect every cap dimension: `block_io`, `network`, `spawn`, `hypervisor`,
  `mmio_devices` (bitwise AND), `block_regions` (bitwise AND). `syscall_allowlist` is NOT in CapSet
  (G1 decision — see plan Out-of-scope).
- F4: Legacy (manifest-absent) `/bin/` path grants are intersected too when spawner is `User`.
- F5: HotSwap threads the replaced cell's CapSet (snapshot before freeze) as `Ceiling`.

**Non-functional**
- N1: `caps_of` is one extra dropped-guard lock + a few ANDs. No manifest ABI change (Law 1). Kernel-side only (Law 4).

## Architecture
`kernel/src/task/cap.rs`:
```
pub struct CapSet { pub block_io: bool, pub network: bool, pub spawn: bool,
                    pub hypervisor: bool, pub mmio_devices: u8, pub block_regions: u8 }
impl CapSet {
    pub const ALL: CapSet = …; pub const EMPTY: CapSet = …;
    pub fn of_task(t: &Task) -> CapSet;
    pub fn from_manifest(m: &CellManifest) -> CapSet;   // MUST replicate loader.rs:175-177:
        // block_regions = data | (lfs<<1) | (lfs<<2)  — NOT a 1:1 bit copy, or VFS loses P5 range
    pub fn intersect(self, o: CapSet) -> CapSet;        // field min / bitwise-AND
    pub fn apply_to(self, t: &mut Task);
}
pub enum Spawner { Root, User(usize), Ceiling(CapSet) }
```
Grant flow in `loader.rs` (replaces current per-cap block; PRESERVE P1 device-scoped MMIO + P3 measurement):
```
let manifest_caps = manifest_opt.map(CapSet::from_manifest)
                                .unwrap_or_else(|| legacy_path_caps(path));
let granted = match spawner {
    Root            => manifest_caps,
    User(stid)      => manifest_caps.intersect(snapshot_caps_of(stid)),  // dropped-guard snapshot
    Ceiling(ceil)   => manifest_caps.intersect(ceil),
};
// then a SEPARATE SCHEDULER.lock() block: granted.apply_to(child_task)
```

## Related Code Files
**Modify**
- `kernel/src/task/cap.rs` — `CapSet` + `Spawner` + helpers (+ `#[cfg(test)]` intersect + from_manifest tests).
- `kernel/src/loader.rs` — `spawn_from_path(path, Spawner)`; dropped-guard `caps_of`; grant refactor preserving P1/P3.
- `kernel/src/task/syscall.rs` — `SpawnFromPath` handler passes `User(caller_id)`; `SpawnFromMem` confirmed cap-less; `HotSwap` handler threads the replaced cell's CapSet.
- `kernel/src/cell/hotswap.rs` — snapshot replaced cell's `CapSet` before freeze; pass `Ceiling(caps)` to `spawn_from_path` (NOT `Root`).
- `kernel/src/main.rs` (~455-464) — init grant block: `CapSet::ALL.apply_to(init)` + boot-log dump; init spawn stays `Root` only for itself.
- `cells/tools/init/src/main.rs` — **no manifest change** (decision (a) removed). Confirm init's child spawn paths are compile-time constants (no data-derived paths) — escalation-oracle bound.

## Implementation Steps
1. `CapSet` + `Spawner` + helpers; `from_manifest` replicates block_regions derivation; unit tests
   (`from_manifest(vfs_manifest).block_regions == 0b111`; intersect monotonicity).
2. Refactor loader grant block (preserve P1 mmio_devices + P3 measurement); dropped-guard `caps_of`.
3. Thread `Spawner` through all call sites: syscall `User(caller_id)`; boot init `Root` + `CapSet::ALL`
   direct write; HotSwap `Ceiling(replaced_caps)`.
4. Build riscv64 + aarch64.
5. Boot-verify: `./run-tests.ps1 boot boots_to_shell_prompt` → `ViCell >`, vfs+net+shell up, no faults;
   confirm the init-CapSet boot-log dump shows ALL.
6. **Regression test (non-init spawner):** a cell holding ONLY SpawnCap spawns `/bin/vfs` → child's
   `block_io == None` (cap stripped). Plus inverse: init (full) spawns vfs → block_io present.
7. **HotSwap regression:** hot-swap a target from a SpawnCap-only initiator → replacement cannot
   exceed the replaced cell's caps (no full-manifest re-grant).

## Todo
- [ ] `CapSet`/`Spawner` + helpers + unit tests (block_regions derivation, intersect)
- [ ] loader grant refactor (preserve P1/P3) + dropped-guard `caps_of`
- [ ] thread `Spawner` (syscall=User, boot init=Root+ALL direct, hotswap=Ceiling)
- [ ] init `CapSet::ALL` in main.rs + boot-log dump; verify spawn targets are compile-time constants
- [ ] build riscv64 + aarch64 clean
- [ ] boot smoke green (init=ALL dumped, no faults)
- [ ] delegation regression (non-init spawner) + inverse
- [ ] hotswap regression (ceiling enforced)

## Success Criteria
- Boots to `ViCell >`, services up, no faults; init CapSet == ALL in boot log.
- Non-init spawner cannot grant a child a cap it lacks (test); HotSwap cannot re-grant beyond the
  replaced cell (test).
- `cargo build` clean both arches; no manifest ABI change.

## Risk Assessment
| Risk | Mitigation |
|------|-----------|
| init grant still via manifest (no-op) → boot strips vfs caps | C1 fix: direct `CapSet::ALL` in main.rs; boot-log dump asserts it |
| HotSwap uses `None`/`Root` → laundering | `Ceiling(replaced_caps)`; hotswap regression test |
| Re-entrant lock deadlock in `caps_of` | dropped-guard snapshot; `policy::lookup` (P04) outside any SCHEDULER guard |
| `from_manifest` naive bit-copy drops VFS P5 region | replicate loader.rs:175-177 derivation; unit test == 0b111 |
| Missed spawn site | compiler forces new param; grep `spawn_from_path`/`spawn_from_mem` (incl main.rs:455, syscall.rs:1716, hotswap.rs:100) |
| init spawns a data-derived path → escalation oracle | enforce/verify init spawn paths are compile-time constants |

## Security Considerations
- Monotonic downgrade enforced kernel-side from `init` (root authority) down. init's full-cap grant is
  load-bearing → init must remain first-party/trusted (future: secure-boot/signing) AND its spawn
  targets must be compile-time constants (no data-derived paths) to avoid a confused-deputy oracle.

## Next Steps
Phase 04 composes `∩ policy` at the same `effective_caps` site (outside any SCHEDULER guard).
