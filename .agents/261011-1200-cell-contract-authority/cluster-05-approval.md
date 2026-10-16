# Formal approval record — Cluster 5 (D12, C01, C02, C06–C11)

## Authority and recorded decision

Recorded 2026-10-15. Decision source: explicit user instruction **"phê duyệt cụm 3, 4 rồi 5 theo thứ tự"**.

This document records the formal governance approval of **Cluster 5 (D12, C01, C02, C06, C07, C08, C09, C10, C11)** under the Law-1 dual-checkpoint framework.

## Approved packages and candidate identity

| Delta | Title | Owner | Approved candidate revision | Candidate digest (SHA-256) |
|---|---|---|:---:|---|
| **D12** | Local/remote endpoints and cluster carriers | P08 | **r1** | `5782ff15d4f243d9dc5ffe0ab69ad7593759bdfdda9ce6064b4a05f2e4f436bd` |
| **C01** | Application/workload policy and authority | P08-C/P08-B | **r1** | `fd7a2d7b823744bb83e847e4c6ebf64939ab692c30727601986232e2a47e64b7` |
| **C02** | Budget vectors and immutable maxima | P03/P08-C | **r1** | `9b4465aa1e5f38e7b2eb0b5924a2edc13864d8598f6a82409ae94ce5b0811aa9` |
| **C06** | Concrete/pool endpoints and Ready provenance | P08-B/P08-C/P06 | **r1** | `3392e823c5d1cec5a20ffa2bf24f3950f158e2f14ed77e5326dd412588c28ec8` |
| **C07** | Immutable origin and issuer-sealed single target | P04/P08-A/P08-B | **r1** | `445f0e00d7d5f4429742e5f7222e2449b52938f39fdf5423892071d4c4d19fff` |
| **C08** | Receiver-start challenge freshness | P08-B/P08-C/P08-D | **r1** | `d3de4a3dddbcf7b84f8e726544e8387b12a742b5c04b4cbc76ef10a527742921` |
| **C09** | Weighted routing, bounded queues and shares | P08-B | **r1** | `f733d6f3098df23bb5890eb864425ac7dd4710d8fb0624dbedadbdcc4c85fde8` |
| **C10** | Generation-complete telemetry and scaling | P08-D | **r1** | `166365b3cb7372f99ff975af9b06ed795b9d6d76b1974d32af6fe4c9a809828e` |
| **C11** | Placement and transport-session boundaries | P08-C/P08-A | **r1** | `f04011d1c4c006e0fd2313c6d7e867a746ed0a5d46ccebc2f4539f4e64f4c526` |

## Checkpoint disposition for Cluster 5

1. **Checkpoint 1: SCOPED USER CONSENT RECORDED; COMPLETE-PACKAGE GATE PENDING**
   - The user approved the presented transport/workload/budget/routing/placement design scope. Actual admitted principals, finite measured profiles, proof sources and complete producer/caller closure remain prerequisites; no multi-node implementation qualification or lock release follows.
   - **D12 / C01 / C02 / C06–C11 r1 candidates**: Structured multi-node transport carriers, deterministic placement formulas, challenge freshness boundaries, weighted flow shares, and generation-complete telemetry models.

2. **Law-1 Checkpoint 2 (Implementation Qualification): PENDING**
   - Checkpoint 2 qualification requires:
     1. Multi-node cluster harness implementation under Phase 08 (Distributed).
     2. Integration with physical network drivers and KMS/Silo keys.
     3. 200-client live concurrency testing across admitted replicas in Phase 10.

## Governance boundaries and exclusions

1. **Lock preservation:** All 38 scoped locks in `.agents/cell-contract-0024-locks.md` remain active and in force.
2. **Runtime ABI freeze:** No cluster endpoints or distributed wire formats are activated in production without full Phase 08 implementation and CP2 evidence.
