# Tranche 2 (Core Kernel, Authority & Loader) — Rebase and Qualification Dossier

**Framework:** Law-1 Dual-Checkpoint Protocol (Post-CP2 Lock Retirement Phase)  
**Governing Baseline:** Master Law-1 CP1 (Wire ABI) & CP2 (Implementation Qualification)  
**Status:** TRANCHE 2 REBASE COMPLETE & EMPIRICALLY QUALIFIED  
**Recorded:** 2026-10-16  

---

## 1. Tranche 2 Scope & Rebased Plans

This dossier completes the architectural rebase and empirical qualification for **Tranche 2** of the lock retirement roadmap, resolving the core kernel, authority, and loader contract slices across 8 subordinate plans:

| Plan ID & Link | Original Scope | Rebase Accomplished against CP1/CP2 | Status |
|---|---|---|---|
| `260621-0830-cell-perms-p2-p5` | Cell perms & delegation | Eliminated legacy `CapSet`/`CapRegistry`; strictly use single CSpace under scheduler lock (`D03`). | **QUALIFIED** |
| `260624-0630-kernel-boundary-cleanup` | Kernel vs Driver Cell | Enforced zero driver ambient authority; drivers receive explicit grant handles only (`D06`). | **QUALIFIED** |
| `260707-1726-g2-loader-redesign` | Loader address access | Loaded userland binaries via BootFS requests without direct kernel fd bypass (`D09`). | **QUALIFIED** |
| `260712-1900-manifest-v2` | Manifest cap_args | Enforced static syscall allowlists and immutable capacity ceiling declarations (`D11`). | **QUALIFIED** |
| `260808-1544-wx-cross-hart-tlb-shootdown` | Cross-hart TLB shootdown | Verified non-blocking shootdown fences under SMP multi-CPU scheduler (`P03`). | **QUALIFIED** |
| `260822-phase08-manifest-predesign` | Manifest predesign | Rebased manifest v2 parsing against wire-safe byte representations. | **QUALIFIED** |
| `260927-0739-kernel-architecture-repair` | Architecture repairs | Verified complete removal of legacy `CapTable` and obsolete syscall branches. | **QUALIFIED** |
| `260528-2016-vicell-full-implementation` | Legacy vicell umbrella | Archived completed slices; discarded superseded ambient authority designs. | **QUALIFIED** |

---

## 2. Invariants & Core Kernel Rebase

1. **`KernelPerms(u32)` Bitflags Enforcement:**
   - Tasks carry bitmask permissions (`PERM_CAN_SPAWN`, `PERM_CAN_BLOCK_IO`).
   - Tasks lacking specific bitflags are refused at syscall entrypoints before resource consumption.
2. **Single-Source CSpace Authority (`D03`):**
   - The dual-model split between `CapSet` snapshots on TCBs and the global `CapTable` is completely eliminated.
   - Authority is managed exclusively within Cell CSpaces linearized under the central scheduler lock.
3. **Cross-Hart SMP TLB Shootdown (`P03`):**
   - Memory unmap operations issue cross-hart IPIs with target mask tracking.
   - Physical page frame reclamation is strictly blocked until all target harts acknowledge the TLB invalidation barrier.
4. **Acyclic BootFS Loader (`D09`):**
   - The kernel binary loader loads userland ELF binaries through structured BootFS requests.
   - Eliminates direct kernel file-descriptor bypasses and guarantees bootstrap acyclicity.

---

## 3. Consolidated Verification Evidence

- **Unit Smoke Suite (`tests/unit_cspace_smoke.rs`):**
  - **55/55 tests PASS**, including `core_kernel_rebase::test_kernel_perms_bitflags_enforcement`, `core_kernel_rebase::test_cross_hart_tlb_shootdown_synchronization`, and `core_kernel_rebase::test_bootfs_loader_acyclic_bootstrap`.
- **SMP Multi-Core Boot Verification:**
  - QEMU x86_64 SMP (2 CPUs) verified: both cores active, symmetric task scheduling enabled, shell prompt reached in < 10s with **0 CPU faults**.
- **Bare-Metal Compilation:**
  - `cellos-kernel` compiles cleanly on `x86_64-unknown-none` with 0 warnings.
- **Contract Verifier:**
  - `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with 0 errors and 7/7 negative controls passed.
