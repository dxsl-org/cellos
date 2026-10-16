# Formal approval record — Cluster 2 (D01, D02, D03)

## Authority and recorded decision

Recorded 2026-10-15. Decision source: explicit user instruction **"phê duyệt cụm 2"** following the approval of Cluster 1 and review of the kernel handle, principal, and CSpace candidate packages.

This document records the formal governance approval of **Cluster 2 (D01, D02, D03)** under the Law-1 dual-checkpoint framework.

## Approved packages and candidate identity

| Delta | Title | Owner | Approved candidate revision | Candidate digest (SHA-256) |
|---|---|---|:---:|---|
| **D01** | Local object handles | P02 | **r2** | `8cda9469ca2bd4d36abfaf96b4ba93d38a35fd3ec929e71d1203a88926a938cd` |
| **D02** | Principal, Kernel identity and bound endpoint attribution | P02/P03/P06/P08 | **r1** | `5ad7af93f8b0c5fca665e717c4e648da475e0c844f88224b36ff86fcb9bfb72d` |
| **D03** | Shared Cell CSpace, spawn intersection and ancestry revocation | P02/P03/P05 | **r2** | `ca10b9a9933e478ec457242e3f9dae82bf99b797643c77d0f2963710edb6b939` |

## Checkpoint disposition for Cluster 2

1. **Checkpoint 1: SCOPED USER CONSENT RECORDED; COMPLETE-PACKAGE GATE PENDING**
   - The user approved the presented D01/D02/D03 design scope. Missing finite profiles, source/caller closure and exact operational assignments remain prerequisites; these preparation digests are not complete-package CP1 qualification or lock release.
   - **D01 r2 candidate**: Opaque non-wrapping 64-bit tickets indexing preallocated slot arenas; 48-byte LE handle control request (`Close`, `Derive`, `Revoke`, `QueryRevoke`); 32-byte status header (`Success=0` through `Pending=14`); 48-byte BootFS file request replacing legacy VIFS1 open/close/seek calls.
   - **D02 r1 candidate**: 40-byte LE principal record distinguishing `Kernel` singleton (minted only in kernel bootstrap context) from live `Cell { id: u64, generation: u64 }`; 24-byte full binding verification `(tid, Cell, generation)` at submit under scheduler lock.
   - **D03 r2 candidate**: Shared `CellAuthority` state per `(CellId, generation)` replacing per-task `CapSet` snapshots; single-lock linearization under existing scheduler lock for authority admission; 64-byte CSpace request covering all 9 capability classes; lineage ancestry revocation marking `AdmissionCut` and tracking quiescence via QueryRevoke without unbounded traversal.

2. **Law-1 Checkpoint 2 (Implementation Qualification): PENDING**
   - Neither Cluster 1 nor Cluster 2 has complete native implementation qualification; isolated libraries and boot/mount captures cannot establish it.
   - Checkpoint 2 qualification requires:
     1. Kernel implementation in `kernel/src/cell/` and `libs/api/src/abi/cap.rs`.
     2. Clean cutover of all callers of legacy `CapId`, `CapRegistry`, `CapRevoke`, and `caller_identity` (no compatibility shims).
     3. Runtime SMP verification on multi-CPU targets exercising concurrent handle derivation, spawn intersection, and non-blocking ancestry revocation with device quiescence.

## Governance boundaries and exclusions

1. **Scope limited to Cluster 2:** This approval covers D01, D02, and D03 candidate approval only. It does not approve or promote the remaining 17 packages (D04–D12, C01, C02, C06–C11), which remain pending their independent blocking inputs.
2. **Lock preservation:** All 38 scoped locks in `.agents/cell-contract-0024-locks.md` remain active and in force. Implementation of Cluster 2 must proceed through Phase 02 governance.
3. **Runtime ABI freeze:** No public ABI identifiers, opcodes, or system calls are activated in production by this candidate approval.
