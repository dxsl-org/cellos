# Formal approval record — Cluster 1 (C03, C04, C05)

## Authority and recorded decision

Recorded 2026-10-15. Decision source: explicit user instruction **"phê duyệt cụm 1"**. Correction 2026-10-16: the assistant's presentation of a complete five-step native implementation/CP2 dossier was unsupported. The user's scoped consent is retained; it is not evidence that missing implementation or qualification exists.

This document records the formal governance approval of **Cluster 1 (C03, C04, C05)** under the Law-1 dual-checkpoint framework.

## Approved packages and candidate identity

| Delta | Title | Owner | Approved candidate revision | Candidate digest (SHA-256) |
|---|---|---|:---:|---|
| **C03** | Genesis and one-use bootstrap | P08-C/P06 | **r2** | `ff938e515847de684d975d4edf7bd9ac86a884f2ca516bf7b517a1ab76844cb4` |
| **C04** | Reserve, live claim, key closure and recovery | P08-C/P06 | **r1** (with DCO1 extension) | `2031fb012118db3aaa4bcb5580c582f7c4e7c60492d62c576b0a479f36dc91c7` |
| **C05** | Mutable allocation, pending shrink and drain | P03/P08-C/P06 | **r3** | `819150994c582069e1d640ba836028c4f449dc0665317aabf891be3c085b0e00` |

## Checkpoint disposition for Cluster 1

1. **Checkpoint 1: SCOPED USER CONSENT RECORDED; COMPLETE-PACKAGE GATE PENDING**
   - Preserve approval of the presented C03/C04/C05 semantic choices. This does not supply absent operational assignments, exact successor tables, measured profiles or full caller closure.
   - `libs/durable-charge-codec` is a standalone implementation candidate, not an admitted canonical runtime proof profile. Candidate document digests identify presented preparation, not completed CP1 packages.
   - State transition rules (fixed-home Authority credit, one-use bootstrap, two-plane resize $G = \max(Q-B,0), L = \max(Q-E,0)$, fold, partial credit installments, no-app cancel, `Closing` gate, and one-credit release) are approved as governing specifications.

2. **Law-1 Checkpoint 2 (Implementation Qualification): INCOMPLETE EVIDENCE**
   - The previous assistant-authored QUALIFIED/APPROVED technical disposition is withdrawn; this does not revoke the user's instruction.
   - Codec/journal host tests are isolated library evidence; bare-metal compilation does not execute them on Cellos.
   - The four-scenario native runner observes boot/storage only, and its corrupt-refusal verdict was reproduced as a false PASS after VM start failure.
   - The separate Linux/SQLite lab is not the native owning-service/publication/issuer/Authority implementation.
   - [CP2 qualification packet](durable-charge-cp2-qualification-packet.md) now records missing native integration, profile measurement and caller cutover. No CP2 or release promotion is inferred.

## Governance boundaries and exclusions

1. **Scope limited to Cluster 1:** This approval covers C03, C04, and C05 only. It does not approve or promote the remaining 20 delta packages (D01–D12, C01, C02, C06–C11), which remain pending their independent blocking inputs.
2. **Lock preservation:** All 38 scoped locks in `.agents/cell-contract-0024-locks.md` remain active and in force to prevent accidental or ungoverned public ABI drift.
3. **Qualification ceiling:** Neither complete native VMware qualification nor physical deployment qualification has been established. Physical power-loss, protected floor and Intel remapper requirements remain additional unproven boundaries.
