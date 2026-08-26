---
title: "Mythos window — G1/G2/G3 deep-analysis dossier set"
description: "The 7 items across the whole roadmap/spec/plan/code surface that genuinely need high-capability architectural analysis before any coding, adjudicated during the 2026-07-12→14 analysis-only window. Every dossier is analysis/spec — no implementation."
status: analysis-complete
window: mythos-analysis-only (expires 2026-07-14) — see [[feedback-mythos-window-analysis-only]]
created: 2026-07-12
---

# Mythos window — deep-analysis dossier set

## What this is

The user asked: across all of G1/G2/G3, what still needs Fable/Mythos-level analysis
before coding? A 4-agent sweep (roadmap tail, specs, `.agents/` plans, code markers)
found ~43 open roadmap items, of which the large majority are **mechanical** (design
settled) or **hardware-blocked** (waiting on physical boards). **Seven** carry an
unmade architectural/trust decision or a live correctness/security hole. Those seven
are adjudicated here. No code was written (window is analysis-only).

## The seven, by disposition

| # | Item | Dossier | State entering | Verdict / output |
|---|------|---------|----------------|------------------|
| 1 | P-TRUST loader trust-model (spec §7 open Qs) | [dossier-1](dossier-1-trust-spawn-verdicts.md) | plan+spec done, 4 open Qs | resolved: extend `CapSet` (not wrap); uniform fold; (b)(c)(d) = G2 |
| 2 | Supervisory P00 (5 open Qs) | [dossier-1](dossier-1-trust-spawn-verdicts.md) | plan done, 5 open Qs | resolved: `sys_spawn_replacement(old_tid)`; fold drain; restore is dormant; panic-reboot root; cap-bearing swap test mandatory |
| 3 | Tier 3b VMM hardening (red-team open decisions) | [dossier-6](dossier-6-tier3b-verdicts.md) | red-teamed, 3 open | resolved: per-VM backing mandatory; freeze P05 at Alpine; C1 = bounded coalescing IRQ set |
| 4 | **Manifest v2** (PKU tier / CAN-ADC flags / cap_args) | [dossier-2](dossier-2-manifest-v2.md) | **no dossier existed** | design-ready: one 16-byte versioned struct, tier-as-**floor** invariant, one Law-1 bump; needs `/hc-plan` |
| 5 | **Revocation in SAS** | [dossier-3](dossier-3-revocation-sas.md) | **no dossier; thought "planned"** | reclassified: `CapRevoke` **ships but is unsound** (stale authority); split eager(ambient)/lazy(syscall); G1 narrow + G2 teardown plan |
| 6 | **DICE / KMS / K2-K3 identity** | [dossier-4](dossier-4-dice-identity.md) | **no dossier** | assembly not invention; 3 decisions locked (CDI source, EAT shape, identity ladder); G2 plan |
| 7 | **Thread CellId inheritance** | [dossier-5](dossier-5-thread-cellid.md) | filed "low-pri gap" | **reclassified HIGH**: live memory-quota escape, defeats G1 graduation criterion #2 |

## Highest-value findings (what the analysis actually changed)

1. **#7 is a live quota escape, not an edge case.** `Syscall::Spawn` → `CellId(0)` →
   `charge()` short-circuits → thread memory is uncharged. G1 criterion #2 ("bounded
   memory on EVERY write path") is not actually met. Small fix, high severity.
2. **#5 revocation already ships and is a lie for Class-2 (ambient) authority.**
   Revoking `PcieDriverCap` leaves the IOMMU domain live (`unmap_dma` is a no-op
   stub) → DMA-anywhere persists. Recommend G1 *narrowing* (refuse what it can't
   truly revoke) + G2 teardown plan.
3. **#4 manifest v2 is a single Law-1 bump that unblocks 3 tracks** (PKU tier,
   CAN/ADC, cap_args hook). The non-obvious part: `tier` is a **floor**, not a
   ceiling — inverting it is a privilege escalation.
4. **#1/#2 open questions all resolved without expanding G1 scope.** P-TRUST stays
   the root of the dependency tree; land order unchanged.

## Dependency / sequencing (coding, after the window)

```
P-TRUST (#1, dossier-1)  ── root; kernel-only, no Law 1
   ├─ Supervisory P00 (#2)          → correct-by-construction after P-TRUST
   ├─ Manifest v2 (#4)  [Law 1 ×2]  → tier-floor interacts with unified CapSet
   └─ Revocation G2 teardown (#5)   → shares the DMA-anywhere invariant
Thread CellId (#7)       ── independent, priority correctness fix, no Law 1
Tier 3b C1/backing (#3)  ── independent of P-TRUST; C1 is the LIVE must-fix
DICE/K3 + KMS (#6)       ── G2, hardware-informed; software CDI slice testable first
```

## Not needing analysis (documented, closed)

G3 NPU/ViAccelerator (roadmap bars detailed spec pre-hardware), H1/H2 enterprise VM
(customer-gated), all SBC bring-up (code done, board-blocked), and the mechanical
set (suite-green, SPI, pkg-dist, shell-on-screen A/B, utilities, game ports).

## Derived full plans (created 2026-07-12 from these dossiers)

The dossiers' verdicts were turned into cookable plans. The three pre-existing plans
had their Open-Questions sections resolved in place; four new plans were authored:

| Item | Plan location | State |
|------|---------------|-------|
| #1 P-TRUST | `.agents/260712-1100-loader-trust-repair/` | existed; §7 resolutions appended |
| #2 Supervisory | `.agents/260712-0800-supervisory-cell-migration/` | existed; Open Questions → RESOLVED |
| #3 Tier 3b | `.agents/260712-0952-tier3b-vm-hardening-compat/` | existed; Mythos verdict layer added |
| #4 Manifest v2 | `.agents/260712-1900-manifest-v2/` | NEW |
| #5 Revocation | `.agents/260712-1901-cap-revocation/` | NEW |
| #6 DICE/KMS/K3 | `.agents/260712-1902-dice-attestation-identity/` | NEW |
| #7 Thread CellId | `.agents/260712-1903-thread-cellid-quota-fix/` | NEW |

All plans are docs only (window discipline). None is cooked.

## Window discipline

Per [[feedback-mythos-window-analysis-only]] (expires 2026-07-14): these are
plan/spec/decision records only. The one item with a defensible claim to jump the
rule is **#7** (defeats a graduation gate); flagged for the user to decide. Everything
else is document-now, cook-after-window.
