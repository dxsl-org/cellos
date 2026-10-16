# Formal approval record — Cluster 3 (D04, D06, D10)

## Authority and recorded decision

Recorded 2026-10-15. Decision source: explicit user instruction **"phê duyệt cụm 3, 4 rồi 5 theo thứ tự"**.

This document records the formal governance approval of **Cluster 3 (D04, D06, D10)** under the Law-1 dual-checkpoint framework.

## Approved packages and candidate identity

| Delta | Title | Owner | Approved candidate revision | Candidate digest (SHA-256) |
|---|---|---|:---:|---|
| **D04** | Operations, reply tokens, wait/consume | P04 | **r2** | `d0d8f527d6e4b691252efb1359f62d0c942c197572a521ccc3e96d021e416d2d` |
| **D06** | Grant, mapping and device quiescence | P05/P04/P08/P09 | **r2** | `2a979b42804a22fc799046b7923157748cf97d4d94874e82462fef4e42140f6b` |
| **D10** | Ring negotiation, correlation and authority | P05/P04/P02 | **r1** | `c0d8dc8a7a2077d6a60bc5edda34ed8cf4439e2de8b965f493023134ea413de1` |

## Checkpoint disposition for Cluster 3

1. **Checkpoint 1: SCOPED USER CONSENT RECORDED; COMPLETE-PACKAGE GATE PENDING**
   - The user approved the presented operation/grant/ring design scope. Exact finite profiles, producer/caller closure and complete successor contracts remain prerequisites; no lock release follows from preparation digests.
   - **D04 r2 candidate**: Explicit 48-byte submit/scoped-wait records; non-reused operation tokens; single-source completion queue; terminal record retention; error/output ownership rules.
   - **D06 r2 candidate**: 64-byte use request; bounded borrowed-buffer lifetime; device DMA/MMIO fence quiescence tracking; no unquiesced memory reuse.
   - **D10 r1 candidate**: Ring negotiation protocol; correlation IDs; ring buffer bounds and channel authority gating.

2. **Law-1 Checkpoint 2 (Implementation Qualification): PENDING**
   - Checkpoint 2 qualification requires:
     1. Implementation in Phase 04 (Operations) and Phase 05 (Resource authority).
     2. Clean cutover of all legacy async/grant call-sites.
     3. Runtime evidence of operation queue exhaustion refusal, concurrent reply delivery, and DMA quiescence before buffer reclaim.

## Governance boundaries and exclusions

1. **Lock preservation:** All 38 scoped locks in `.agents/cell-contract-0024-locks.md` remain active and in force.
2. **Runtime ABI freeze:** No public ABI identifiers, opcodes, or system calls are activated in production without full P04/P05 implementation and CP2 evidence.
