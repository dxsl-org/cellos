---
title: "Runtime capability revocation completion (close the J-Kernel stale-authority hole)"
description: "sys_cap_revoke (219) already ships but only clears TCB cap fields — every derived authority stays live. G1 narrows the syscall to be honest; G2 builds eager Class-2 teardown (DMA/IOMMU, grants, MMIO, BDF) + victim notification."
status: queued (P00 complete; P01-P05 pending)
priority: P1
effort: 6
branch: main
tags: [security, capability, revocation, iommu, sas, lbi, kernel-boundary]
created: 2026-07-12
law1: P05 only (new cap_mask ABI bits in libs/api) + P04 AppEvent variant/envelope discriminant (ostd + spec/17, NOT libs/api)
---

# Runtime Capability Revocation Completion

> **D35 portfolio ruling (2026-08-01):** child of Trust & Identity; keep its teardown
> ordering and ABI gates separate. P00's honest narrowing landed in `4b8f1543`; P01-P05
> remain queued behind the active Midori convergence program.

**Design authority:** `.agents/260712-1836-mythos-g123-analysis/dossier-3-revocation-sas.md` (locked). Turn into phases; do not re-litigate.

**The hole (LIVE):** `sys_cap_revoke` (`syscall.rs:1420-1474`) clears `Option<Cap>` TCB fields + ANDs the mmio/blkregion masks, logs an audit event — and does nothing else. Every resource already *derived* from the revoked cap stays live: shared grants, mapped MMIO, IOMMU/DMA domains, BDF ownership. This is exactly the stale-authority retention `spec/16` cites the J-Kernel proof for (LBI prevents forgery, not revocation).

**Two classes, two mechanisms (locked):**
- **Class 1** (syscall-mediated: block_io, network, spawn, RequestMmio-future, RegisterService) → LAZY re-check is correct + free. The TCB-field clear already suffices. No teardown.
- **Class 2** (ambient hardware/memory already handed out: mapped MMIO, IOMMU/DMA domain, already-shared grant) → LAZY CANNOT WORK (access bypasses the kernel) → MUST eager tear down.

**Sequence after P-TRUST** (`.agents/260712-1100`) — shares the DMA-anywhere invariant and adds `pcie_driver`/`platform`/`supervisor`/`cell_store_region` to `CapSet`, which P05 revokes.

## Two-speed structure

| Phase | Speed | Title | Law 1 | Effort |
|-------|:-----:|-------|:-----:|:------:|
| [P00](phase-00-narrow-revoke-honest.md) | **G1** | Narrow `sys_cap_revoke` — reject what it cannot truly revoke (MMIO/privileged) with `NotSupported` + audit | none | ~20 LOC |
| [P01](phase-01-iommu-unmap-dma.md) | **G2** | Implement `iommu::unmap_dma` (per-range zero + IOTLB flush) + expose selective DMA-domain teardown | none | S |
| [P02](phase-02-selective-grant-reclaim.md) | **G2** | Selective grant reclaim via existing `shared_to` link (`reclaim_grants_for_task`, no death) | none | S |
| [P03](phase-03-mmio-bdf-teardown.md) | **G2** | MMIO page-table unmap + `release_for`; BDF release — selective per revoked device class | none | M |
| [P04](phase-04-widen-revoke-appevent.md) | **G2** | Widen `sys_cap_revoke` to dispatch Class-2 teardown (MMIO bits) + `AppEvent::CapRevoked{mask}` (envelope `0xF2`) | AppEvent/spec-17 | M |
| [P05](phase-05-cap-mask-abi-privileged.md) | **G2** | Add `cap_mask` bits (PCIE_DRIVER/PLATFORM/SUPERVISOR) to `libs/api`; wire their revoke to teardown | **YES (2x)** | S |

## Dependency graph

```
P-TRUST (260712-1100) ──▶ P00 (G1, standalone-shippable) ──▶ P01 ──▶ P02
                                                              │       │
                                                              ▼       ▼
                                                             P03 ──▶ P04 ──▶ P05
```

- **P00** ships independently (honest narrowing); everything else waits on the teardown primitives.
- **P01** first — until `unmap_dma` is real, "DMA can be torn down" is a lie (dossier).
- **P04** re-widens revoke to the already-encodable MMIO bits + notifies victims; needs P01-P03 primitives.
- **P05** last — needs libs/api ABI bits (Law 1) AND P-TRUST's CapSet privileged bits already present.

## Key design decisions (locked)

- **No CDT.** Monotonic-downgrade `intersect` means caps only weaken → no reverse-grant to chase. The one surface needing a derivation breadcrumb (shared grant) already has it: the grant table's `owner → shared_to` link.
- **Reuse `cleanup_cell`, don't fork it.** Class-2 teardown = "make cell-exit teardown callable SELECTIVELY per-cap," not a parallel implementation. `cleanup_cell` (`iommu.rs:58`), `release_for` (`resource_registry.rs:190`), `release_bdfs_for` (:234), `reap_grants_for_task` (`syscall.rs:192`) are the exact building blocks.
- **Fail closed.** A revoke that cannot fully tear a surface down must reject (`NotSupported`), never label-change (that is the whole point of P00).

## Open questions

1. **Hypervisor cap** — dossier does not classify it. Ambient (H-ext CSR access is not syscall-gated) → treat Class-2-ish. P00 default: reject `HYPERVISOR` revoke conservatively; confirm no caller needs it. (flagged in P00)
2. **riscv64/aarch64 MMIO unmap** — MMIO is boot-mapped user for all cells (SAS single page table); x86 maps on-demand via `map_mmio_user_x86`. x86 reverses cleanly with `unmap_page_x86`; riscv/aarch64 need a new per-arch clear-user helper. (flagged in P03)
3. **Which revoked bit triggers grant reclaim?** Grants carry no source-cap tag. P02 scopes reclaim to grants *owned by the target* and invokes it on any Class-2 revoke. (resolved in P02)
