---
phase: 07
title: Scoped SUM — drop whole-lifetime sstatus.SUM=1
tier: thinking
status: deferred-decided-nogo-g1
depends_on: [06]
gate: spike-first-then-go-no-go
---

# Phase 07 — Scoped SUM (deferred; spike-first)

## Context links
- Plan: [plan.md](plan.md)
- Origin: analysis-report RC-4 revised (Bước 3) + red-team M2.

## Overview
Replace whole-lifetime `sstatus.SUM=1` — S-mode freely reading/writing U-mode pages — with **scoped RAII windows** around the specific kernel→U-mode writes that need it. Restores the hardware S/U boundary as defense-in-depth on top of LBI.

> **⚠️ DEFERRED / SPIKE-FIRST (red-team M2).** SUM is NOT localized: set whole-lifetime at `main.rs:483`, **baked into per-task context** `sstatus=0x42120` (`task.rs:568`), and required on every secondary hart (`smp::start_secondaries`). Kernel→U-mode writes are raw pointer/slice writes with no single `copy_to_user` chokepoint — spanning IPC reply copy-out, ISR/timer event injection, and grant copies. The census is cross-cutting exactly as feared. **Default action: run the census as a standalone spike, then spin this into its own plan.** Phases 01–06 + 08 are the complete, shippable RC-4 closure without this.

## Key insights
- G1 benefit is modest (LBI already isolates cells); the value is **G2 multi-tenant / untrusted-Tier-1b** hardening (ties to BS#1, spec 15 §1.4).
- RISC-V pattern: an RAII `SumGuard` (set `SUM` on `new`, clear on `Drop`), analogous to x86 SMAP `stac`/`clac`, wrapping the minimal copy.
- Because SUM is in the per-task `sstatus` template and per-hart, "remove the global enable" is not one edit — every context-restore path must default SUM off and only the guarded copy turns it on.

## Requirements (if the spike says GO)
- **Functional:** kernel boots + all IPC/event delivery works with SUM off by default, on only inside scoped windows. No regression in burst typing, VFS write, DHCP.
- **Non-functional:** the guard is the *only* place SUM toggles; a lint proves no whole-lifetime enable (global, per-task template, or per-hart) remains.

## Architecture
- `struct SumGuard` in the RISC-V HAL (documented `// SAFETY:`).
- Remove SUM from `main.rs:483`, the `task.rs:568` per-task `sstatus` template, and secondary-hart setup; wrap every kernel→U-mode write site in a `SumGuard` scope.
- aarch64/x86: confirm PAN/SMAP posture; scope symmetrically or document why N/A.

## The spike (do this FIRST, before committing the phase)
1. Census: enumerate every kernel→U-mode write site (IPC copy-out in `syscall.rs`, ISR/timer event inject, grant copies). Count + list with file:line.
2. If the census is small + single-owner → proceed with the phase.
3. If cross-cutting (expected) → **stop, write a dedicated plan, ship Phases 01–06 + 08 as the closure.**

## Related code files (if GO)
- Modify: `hal/arch/riscv/*` (SumGuard + CSR), `kernel/src/task/syscall.rs` (copy-out sites), ISR/timer event-inject, `kernel/src/main.rs:483`, `kernel/src/task.rs:568`, `smp` secondary setup.

## Todo
- [x] Census spike complete (site list + count) — see Spike findings (2026-07-08)
- [x] Go/no-go recorded (default = split out) — NO-GO for G1; split to own G2 plan
- [ ] (if GO) `SumGuard` + all sites wrapped + global/template/hart enables removed
- [ ] (if GO) 3-arch burst-typing/VFS/DHCP regression + anti-regression lint

## Success criteria
- **Spike:** a concrete census + a recorded go/no-go decision.
- **(if GO) Runtime evidence:** 3-arch boot with SUM off by default; burst typing, VFS write, DHCP correct (sensitive detectors for a missed copy-out site); lint proves no whole-lifetime enable.

## Risk assessment
- *Missed copy-out site* → silent U-mode fault / dropped IPC. Exhaustive census; burst-typing + DHCP are sensitive detectors.
- **Primary mitigation: this phase is optional to the migration's completeness — split by default.**

## Security considerations
- Restores hardware S/U boundary — defense-in-depth beyond LBI for G2 untrusted/Tier-1b adjacency (BS#1).

## Next steps
Phase 08 regression + docs runs regardless of the SUM decision (records SUM status accordingly).

---

## Spike findings (2026-07-08) — census complete, decision = NO-GO for G1 (split to own G2 plan)

**Census (current line numbers).** SUM=1 is enabled in three places, exactly as feared:
1. **Global, early boot** — `kernel/src/main.rs:285` `csrs sstatus, 0x40000` (riscv64-only; x86 uses PKU, aarch64 uses PAC/PAN — SUM is a RISC-V concept).
2. **Per-task kernel-context template** — `kernel/src/task.rs:569` `sstatus = 0x42120 // SUM=1,FS=1,SPP=1,SPIE=1` and `task.rs:1642` `0x40120 // SUM=1`. This is the dominant enable: every context restore re-sets SUM, so removing the global alone changes nothing. Secondary harts inherit it via this template (no separate per-hart `csrs` — `smp.rs` only touches SIE).
3. **Copy sites requiring SUM during the window** — **36** `from_raw_parts`/`copy_nonoverlapping` in `kernel/src/task/syscall.rs` (syscall-arg buffers: paths, IPC message bodies, ELF bytes for `SpawnFromElf`, `ProcessInfo`, grant data), plus IPC delivery / event injection copies in `task.rs` and `scheduler.rs`. No single `copy_to_user` chokepoint exists.

**New finding that refines the risk (post-migration).** The original justification in the `main.rs` comment — *"the kernel's tech-debt VirtIO drivers fault at early-boot MMIO init"* (driver MMIO mapped USER=1) — is now **obsolete**: all VirtIO block/net/gpu/input drivers were migrated to Driver Cells (G2 loader redesign + prior). The kernel no longer touches USER-mapped device MMIO on the driver path, so the remaining SUM consumers are confined to **well-defined kernel entry paths** (syscall dispatch + IPC/event delivery), not scattered driver code. This makes a future scoped-SUM refactor *more* tractable than the plan originally assumed — but it does not shrink the 36-site + context-template surface.

**Go/No-Go decision: NO-GO for G1; split into its own G2 plan.** Rationale:
- **Scope doctrine (CLAUDE.md):** scoped-SUM replicates a hardware S/U isolation mechanism (SMAP-analogue) that the SAS+LBI model deliberately de-emphasizes — LBI already provides the primary cell-isolation guarantee. It is defense-in-depth, not a SAS/LBI leverage point.
- **Modest G1 benefit:** the value lands only for **G2 multi-tenant / untrusted-Tier-1b** adjacency (BS#1, spec 15 §1.4) — not present in G1.
- **Cross-cutting cost:** ~36 copy sites + the per-task context template + global enable + an anti-regression lint, with burst-typing/DHCP as the sensitive miss-a-site detectors. High blast radius for a defense-in-depth gain that G1 does not need.

**Design sketch (for the future G2 plan, if GO).** RAII `SumGuard` (set `sstatus.SUM` on `new`, clear on `Drop`), analogous to x86 `stac`/`clac`, wrapping each copy; centralize the 36 raw accesses behind `copy_from_user`/`copy_to_user` helpers that own the guard; change the per-task template `0x42120→0x02120` and `0x40120→0x00120` so SUM defaults **off**; drop the global `main.rs` enable; add a lint asserting no whole-lifetime enable remains. Verify by 3-arch boot + burst-typing + VFS-write + DHCP.

**Migration impact: none.** RC-4 closure (Phases 01–06) + verification/docs (Phase 08) are complete and shippable without this. Phase 07 is now formally deferred to a standalone G2 hardening plan.
