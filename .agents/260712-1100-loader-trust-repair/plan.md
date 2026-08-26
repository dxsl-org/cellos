---
title: "P-TRUST — Loader spawn trust-model repair (cap-ceiling closes all authority channels)"
description: "Kernel-only prerequisite phase: fold path-triggered privileged caps into the CapSet ceiling intersection so 'fleet-signed + CapSet⊆ceiling' actually means 'authorized to run as this role with this authority'. Closes a LIVE DMA-anywhere/LBI-bypass reachable today via sys_spawn_from_elf. Blocks supervisory-P00, package-dist-P01, hypha-P4."
status: complete (landed in 721e1f6f)
priority: P0 (security — live LBI bypass)
effort: 1 phase (kernel-only, ~150 LOC + tests)
branch: main
tags: [security, loader, capability, spawn, prerequisite, lbi]
created: 2026-07-12
law1: none (CapSet/Spawner are kernel-internal cap.rs; recommended path avoids libs/api)
---

# P-TRUST — Loader spawn trust-model repair

> **Portfolio correction (D39, 2026-08-01):** the former blocker landed. Consumers may
> treat the dependency as satisfied, but remain queued under the active-plan WIP limit.

**Design authority:** `.agents/reports/spec-260712-loader-trust-model-repair.md` (full contract, 4 channels,
alternatives analysis). This file is the cookable phase for **channel (a) only** — the G1-blocking fix.
Channels (b)/(c)/(d) are G2 hardening, tracked in the spec, NOT in this phase.

## Why this phase exists / blocks everything

The spawn-gate binds *who signed the code* (fleet Ed25519 key) + intersects the **CapSet** ceiling — but
`PcieDriverCap` / `PlatformCap` / `SupervisorCap` / cell-store-region authority are granted by
**unconditional path-match** at `loader.rs:301-324`, executed AFTER and BLIND TO that intersection.

**Live today (not hypothetical):** `sys_spawn_from_elf` (`syscall.rs:1761`) passes the caller's path to
`spawn_gated` gated only by `caller_has_spawn`. Any `SpawnCap` holder + any fleet-signed cell spawned as
`/bin/nvme` → child gets `PcieDriverCap` regardless of the intended ceiling → claim a PCIe BAR + `GrantDma`
→ **DMA writes anywhere in the single address space, bypassing LBI.** Reachability *increases* with
supervisory P00 (`SpawnReplacement`) and Hypha P4 (`tool-spawn` = a new `SpawnCap` actor), which is why
this lands FIRST.

## The fix (spec §2.1 Option A — recommended over path-identity binding)

Fold the four out-of-CapSet authority channels **into** the `CapSet` ceiling so a single intersection
governs every grant:
1. Extend `CapSet` (`kernel/src/task/cap.rs`) to carry `pcie_driver` / `platform` / `supervisor` /
   `cell_store_region` authority bits alongside block_io/network/spawn/hypervisor/mmio.
2. Rework the path-match grants (`loader.rs:301-324`) to **request** these caps, then run them through the
   SAME `requested ∩ ceiling` intersection as the rest — no ungated post-intersection grant remains.
3. The ceiling source stays as-is per caller class (init=Root exempt; normal spawn=spawner_caps;
   SpawnReplacement=frozen-original caps once P00 lands). Because A folds into CapSet, SpawnReplacement's
   frozen ceiling then clamps ALL channels for free.

Chosen over the audit's path-identity binding because **A also closes the today-reachable
`sys_spawn_from_elf` hole**, not just the unbuilt SpawnReplacement.

## Implementation Steps
1. Extend `CapSet` with the 4 authority bits (+ `cell_store_region`); update constructors/Root.
2. Convert `loader.rs:301-324` path-triggered grants → requested-caps folded into the existing ceiling
   intersection; delete the ungated grant.
3. Audit every `spawn_gated` caller (init, loader spawn_from_path, `sys_spawn_from_elf`) for the new
   ceiling: init stays Root-exempt; confirm the legit driver-cell spawn path (init→/bin/nvme) still grants
   PcieDriverCap because init's ceiling permits it.
4. Fail-closed: any requested authority not in ceiling → denied + audit event (no silent drop, no silent grant).

## Success Criteria (oracles)
- **Regression (must stay green):** 3-arch boot — init still spawns nvme/e1000/platform with their caps;
  x86 nvme suite 3/3, aarch64 7/7, riscv main suite. Driver cells still work → ceiling correctly permits.
- **New negative test:** a SpawnCap-holding, non-privileged test cell calls `sys_spawn_from_elf("/bin/nvme")`
  (or a fleet-signed dummy under that path) → child comes up WITHOUT PcieDriverCap (or spawn denied) +
  audit event. Today this test would FAIL (cap leaks); after P-TRUST it passes.
- **No Law 1:** `git diff libs/api` is empty.

## Risk Assessment
| Risk | Mitigation |
|------|-----------|
| Over-tighten → legit driver cells lose caps → boot regression | Step 3 caller audit + the 3-arch green oracle; init's Root/ceiling must permit its children's caps |
| CapSet layout change ripples to snapshot/hotswap serialize | Kernel-internal only; check `snapshot.rs` serializes CapSet by value; no ABI |
| Interaction with in-flight P00/P4 edits to the same files | This phase lands FIRST by design; P00/P4 rebase onto the unified ceiling |

## Security Considerations
Closes a live LBI bypass — the highest-severity finding of the 2026-07-12 analysis window. After this,
`SpawnReplacement` (P00) and `tool-spawn` (P4) inherit a sound ceiling instead of amplifying the hole.
Channels (b) name↔binary, (c) phdr integrity, (d) revocation remain G2 hardening (spec §2.3-2.5) —
they matter when packages are untrusted; under G1 first-party trust they are documented follow-ups, not
cook blockers.

## Open-question resolutions (2026-07-12)

Spec §7 open questions adjudicated in
`.agents/260712-1836-mythos-g123-analysis/dossier-1-trust-spawn-verdicts.md` (Part A):
- **§7.1 `TotalCaps` vs extend `CapSet`** → **extend `CapSet` in place** (a second type
  re-creates the very split the total-ceiling fix removes; add `pcie_driver`/`platform`/
  `supervisor` bools + fold cell-store into the existing `block_regions` bit `0b1000`).
- **§7.2 `sys_spawn_from_path` (237)** → fold applies **uniformly**, no special-case; add
  a `from_path` spawn of `/bin/nvme` **by init** to the regression check (proves init's
  Root ceiling still permits `pcie_driver` post-fold).
- **§7.3 B1 + SpawnReplacement path** → G2; a sign-workflow requirement (sign for the
  frozen original's role path), not a G1 kernel decision.
- **§7.4 policy-anchor separation** → G2 gating assertion on the channel-(d) phase
  (cell-signing key ≠ policy-signing key before `/POLICY.BIN` carries revocation).

## Next Steps / consumers
- **supervisory `.agents/260712-0800` P00** — SpawnReplacement clamps all channels via frozen ceiling (free after A).
- **package-dist `.agents/260712-1000` P01** — /bin write gate; the spec's A2/A4/A3 G2 items attach here.
- **Hypha `.agents/260621-1433` P4** — init-spawn of tool-peripheral now flows through the unified ceiling.
