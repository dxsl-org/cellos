# Admission request — architecture remediation (plan 261110-1703)

**Date:** 2026-11-10 · **Requester:** planning session · **Owner:** the accountable maintainer (the user)
**Plan:** [plan.md](plan.md) · **Source:** [architecture review](../reports/architecture-review-261110.md) · **Red team:** [reports/red-team.md](reports/red-team.md)

## 1. What is requested

1. Admit this plan into the portfolio as **measured-defect work serving Intel C2C** (ADR-0022: every task names a dependency or measured defect). No ADR amendment is requested; if the owner judges one necessary, Phase 01 waits for it.
2. Approve the slice order S0 → S1 → S2 → S3 → S4 → S5 → S6 → S7 → S8 (S7 may move ahead of S5/S6, never in parallel).
3. Grant a **file-bounded exception for S0 only** (§4). Every later slice files its own handoff request with an exact file set; Law-1 slices also file CP1 before edits and CP2 after evidence.
4. Answer the four design decisions in §3.

Nothing beyond S0 becomes executable from this request.

## 2. Active and approved slices (inventory, 2026-11-10)

| Slice | State observed in records | Files it holds / will hold | Conflict with S0? |
|---|---|---|---|
| x86 breadth S4 (phases 10–12) | Holds Intel WIP lane from 2026-11-07 (`cell-contract-0024-locks.md:168`); plan marks 10–12 completed; **no close record found** | S4 request §2 paths (kernel/HAL/drivers) | No (S0 is docs + `Cargo.toml` members + one script + one CI step) |
| x86 breadth S5 (phases 13–19, NIC drivers) | Driver code landed (commits `9297e3404`…`aff9ff81c`); **no handoff record found** for S5 | `cells/drivers/{e1000e,igb,igc,r8169,bcm57xx,ixgbe,i40e}` | No |
| iGPU kernel DMA-export slice | Approved, not yet effective (`:171-176`); conditions 1–3 | `kernel/src/task/{syscall,task}.rs`, `iommu*`, `libs/api/src/abi/syscall.rs` | No for S0; **yes for S2/S4/S5** of this plan |
| x86 Tier-2 GPU runtime R1–R4, R6 | Effective beside WIP, user-space files only | per its handoff §3 | No |
| Cellbox follow-ups | Bounded exceptions, landed | `scripts/check-production-relay-image.py`, net socket table | No |
| Apple Silicon lane | Slot free since 2026-11-05 | — | No |

**Lane observation for the owner:** S4 and S5 of x86 breadth lack close/handoff records; the lane state should be recorded before S1 of this plan starts (S1 edits `ci.yml` and `kernel/build.rs`).

## 3. Design decisions (defaults are reversible)

| # | Question | Default | Alternatives | Consequence of default |
|---|---|---|---|---|
| D1 | Spec 26 skeleton crates (`budget-authority`, `workload-router`, `placement-controller`) | Remove from workspace `members` and images; keep source with a "not wired, fail-open" doc line | Delete; keep as is | No build/test cost, no risk of packaging a fail-open TCB; source available for P08 |
| D2 | CSpace | Promote existing `CellAuthority` to the single per-incarnation authority record (Phase 07) | Cut CSpace to the BootFS file-ticket table and add a new small record | Reuses 1.5k lines and existing register/retire hooks |
| D3 | `kernel/src/snapshot.rs` | cfg-gate behind `snapshot-qualified` in place | Move out of the kernel crate | Smallest change; Spec 15 §3.2 move stays future work |
| D4 | 32-bit targets (rv32, arm, x86-32) | Keep, quarantine under `kernel/src/arch/legacy` | Remove | ADR-0022 "preserve existing code" honoured; unbuilt code stays unverified |

## 4. S0 file-bounded exception (Phase 01 only)

- `.agents/261011-1200-cell-contract-authority/plan.md` and its `phase-01..10-*.md` (frontmatter status + appended dated correction; no evidence file rewritten)
- `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md` (records only)
- `docs/system-architecture.md`, `docs/roadmap/current-focus.md`, `docs/project-changelog.md`
- Root `Cargo.toml` (`members` only, per D1); `lib.rs` doc line in the three crates per D1
- New `scripts/check-plan-status.py`; one guard step in `.github/workflows/ci.yml` pre-build block
- This plan directory

No `libs/api`/`libs/types` change (no Law-1 delta). No kernel, HAL or service source change. Evidence ceiling: host (script, `cargo metadata --locked`).

## 5. Expected Law-1 deltas in later slices (for awareness; each needs its own CP1/CP2)

Phase 06 (only if the caller trailer must carry a role), 08 (generated syscall table, byte-identical), 11 (IPC outcome type, reserved opcodes, lookup removal), 12 (per-service proto modules, byte-identical), 13 (`api` split into `abi`/`wire`/`posix-shim`).

## 6. Minimum useful cut

S0–S2 (phases 00–06): truthful baseline, x86 C2C regression gate, both Critical findings closed. The owner may stop or reorder after any checkpoint.
