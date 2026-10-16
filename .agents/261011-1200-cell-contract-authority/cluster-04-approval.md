# Formal approval record — Cluster 4 (D08, D09, D11)

## Authority and recorded decision

Recorded 2026-10-15. Decision source: explicit user instruction **"phê duyệt cụm 3, 4 rồi 5 theo thứ tự"**.

This document records the formal governance approval of **Cluster 4 (D08, D09, D11)** under the Law-1 dual-checkpoint framework.

## Approved packages and candidate identity

| Delta | Title | Owner | Approved candidate revision | Candidate digest (SHA-256) |
|---|---|---|:---:|---|
| **D08** | Native stream mechanism | P05/P07 | **r2** | `7f4424b6f57a31e2c93fcb86a071b123d2304ae95f81305648b619e8de13f96c` |
| **D09** | POSIX providers, native profiles and bootstrap | P07/P02/P05 | **r1** | `f60e1d6ccc19183fa91d144d9b10938e2610acd5f5c744d07f32522bfc2b2142` |
| **D11** | Manifest/syscall declarations and ceilings | P02/P03/P07/P08 | **r1** | `6ef3f1f66a56f7103ce7277744679666e2d0e3620eed2de243fbaa27b52d4208` |

## Checkpoint disposition for Cluster 4

1. **Checkpoint 1: SCOPED USER CONSENT RECORDED; COMPLETE-PACKAGE GATE PENDING**
   - The user approved the presented stream/provider/declaration design scope. Exact provider/profile/bootstrap/caller closure remains prerequisite; preparation digests do not qualify missing runtime contracts or release locks.
   - **D08 r2 candidate**: Native stream mechanism replacing legacy pipe/poll; stream descriptor layout; buffer transfer bounds; flow control.
   - **D09 r1 candidate**: Pinned mlibc sysdeps/header tuple; POSIX native provider replacement; BootFS bootstrap integration without ambient authority.
   - **D11 r1 candidate**: Manifest versioning; immutable cap_args declaration; static syscall allowlist encoding; capacity ceiling definitions.

2. **Law-1 Checkpoint 2 (Implementation Qualification): PENDING**
   - Checkpoint 2 qualification requires:
     1. Implementation in Phase 05 (Resource authority) and Phase 07 (POSIX runtime).
     2. Clean cutover of all legacy stream/mlibc consumers.
     3. Verification of native stream performance and POSIX conformance test suite.

## Governance boundaries and exclusions

1. **Lock preservation:** All 38 scoped locks in `.agents/cell-contract-0024-locks.md` remain active and in force.
2. **Runtime ABI freeze:** No public ABI identifiers, opcodes, or system calls are activated in production without full P05/P07 implementation and CP2 evidence.
