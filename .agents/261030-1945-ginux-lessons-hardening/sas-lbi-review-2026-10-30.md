# SAS/LBI compatibility review — 2026-10-30

Scope: the 9 phases of this plan, reviewed against Tier-1 SAS/LBI contracts (`docs/security-model.md:14-16,97-119`, Spec 25 §§1–7, ADR-0024:56,84) and source. The two independent reviewer subagents (`SasEntryReview`, `SasAuthorityReview`) failed with a usage-limit error and returned no findings. This record is the direct review, so it has one reviewer's evidence ceiling.

Legend: OBS = read in source/spec at the cited line; INF = inference.

| # | Sev | Phase | Problem in the previous draft | Evidence | Applied change |
|---|---|---|---|---|---|
| 1 | Major | 07 | Universal invariant `CapSet(child) ⊆ boot_ceiling(path) ∩ CapSet(spawner)` contradicts the code: there are three spawn branches (Root/Ceiling/User), and reviewed launch-profile ceilings are not a subset of the spawner's flattened bitset | OBS `governed_spawn.rs:127-149`; Spec 25:44-48 (intersection over issuer, delegator, ceiling, receive policy, operator policy) | Provenance is issuer-derived per Spec 25; equal-rights delegation allowed; process ancestry removed |
| 2 | Major | 07 | "Strict subset" per hop and a kernel-side check of service handles would duplicate VFS/NET ACLs | OBS Spec 25:62-64 (no second VFS/NET ACL; kernel attests only) | Issuer-local negatives run through the real issuer; kernel audit claims only kernel objects |
| 3 | Critical | 09 | Previous fix "caller CellId equal to stored owner, generation ≥ writer" blocks legitimate replacement by a different Cell, and treats a larger generation as recovery authority | OBS Spec 25:22-28 (logical identity rebound only by publication authority), 114-118 (state authority fencing epoch) | 09A: destination-bound transfer approved by state authority + writer epoch; split into 09A/09B/09C |
| 4 | Critical | 09 | The live gap is confirmed: non-argv stash syscalls use the global key table with no holder check | OBS `syscall.rs:7490-7496,7523-7528,7545`; `state_stash.rs:24-26,103-147`; argv is task-owned `:38-101` | 09A admitted as an early independent slice; argv path kept separate |
| 5 | Major | 09 | "Different address-space root / DMA lease id / stack address" as freshness proof is invalid under SAS and with allocator reuse | INF from shared SAS root (security-model.md:115-116) and allocator reuse after quiescence | Freshness by incarnation, object generation, CSpace contents, operation ownership |
| 6 | Major | 09 | A provider-local idempotency table dies with the worker, so it cannot prove exactly-once | OBS Spec 25:118 (store request identity + outcome + revision atomically at the authority) | Dedup state must live at the actual state authority |
| 7 | Major | 04 | SMAP was a hard gate that required migrating SAS accesses to `user_copy`, which adds copies to Tier-1 paths | OBS security-model.md:115-116 (zero-trap SPSC ring / grant, direct MMIO/DMA for SAS) | SMEP/SMAP/UMIP become per-profile options; borrowed bounded audited windows allowed; no copy regression |
| 8 | Major | 04 | NMI log-and-return (D-04a) was unrequested policy expansion that needs nested-NMI/GS/CR3 proof | INF; Spec 25:38 (kernel failure = node recovery) | Fatal policy kept; diagnostics through a lock-free emergency path only |
| 9 | Major | 04 | IST stacks must be mapped in every CR3 that can take an entry, including private Tier-2 CR3s before the software switch | INF from per-domain CR3 (Tier 2) plus `idt/entry.rs` CR3 switch after entry (`entry.rs:116-120`, seen earlier) | Mapping requirement added |
| 10 | Major | 08 | FLR-only re-grant penalized SAS driver availability; AHCI/NVMe have documented controller resets | INF (AHCI HBA reset / NVMe CC.EN reset) | Controller-specific reset strategy; quarantine only when proof is missing; DMA-frame reclaim and device re-grant are separate proofs |
| 11 | Major | 08 | Admission cut + IOMMU alone does not fence borrowed SAS slices, grants/rings, delayed workers or IRQ callbacks | OBS Spec 25:56-58,88 | Quiescence set enumerated; partial acknowledgement = quarantine |
| 12 | Medium | 02 | Raw `static mut` exemptions by class (`per-cpu`, `hardware-table`) ignore NMI/IRQ reentrancy; risk of mechanical Spinlock wrapping | INF; NMI cannot take locks | Ownership/context/lifetime dimensions; no automatic lock migration |
| 13 | Medium | 03 | Perf/KVM gate blocked security fixes (04 depended on 03); metrics mixed SAS and Domain paths | Plan DAG | 04 depends only on 01; tier-separated metrics |
| 14 | Medium | 05 | `require_text`/`reject_text` are prose snapshot tests; the POSIX ledger framed POSIX as if it were the native ABI | ADR-0018; Spec 25:176 | Structured status checks; POSIX as optional adapter per profile |
| 15 | Medium | 06 | A universal magic/version/size header would add dynamic validation to typed same-build SAS calls; mutating after signing confuses signature and parser failures | OBS `manifest_parse.rs:31-48` (v1 accepted) | Boundary-specific contracts only; correctly signed malformed fixtures |
| 16 | Minor | 01 | "SHA-256 detects tampering" was wrong for an unkeyed hash | Crypto fact | Input rewritten: keyed MAC/signature for tampering |

## What still holds from ginux

- IST + paranoid GS/CR3 entry: a hardware-level gap on every tier.
- Ordered DMA/IRQ quiescence: LBI cannot fence a device.
- Fresh incarnation + fenced writer on replacement: matches Spec 25 §7.
- Boundary validation before effect; inventory of writable globals; tier-aware perf evidence.

## Residual uncertainty

Findings 8–10 are INF: they rest on architecture knowledge, not on an observed failure. A second independent review is still recommended when reviewer quota returns.
