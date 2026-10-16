# Durable charge formal technical review — 2026-10-15

**Disposition: NOT APPROVED FOR IMPLEMENTATION / CP1.** This records independent technical review, not maintainer/operator consent. Targets: [semantic contract](durable-charge-ownership-adoption-contract.md), [canonical byte candidate](durable-charge-canonical-byte-candidate.md), [actual assignment evidence](durable-charge-assignment-evidence.md). No runtime adoption or failure qualification ran. Original locks/phases/checkpoint states remain unchanged.

## Formal reviewers and findings

| Reviewer | Observed finding | Integration disposition |
|---|---|---|
| FormalOwnershipReview | Issuer debit origin/custody wording contradicted B4/B5 | Corrected: debit provenance immutable forever; custody ends at B4 even when B5 lost |
| FormalOwnershipReview | C03 line34 permits Authority/issuer-owned release | **OPEN upstream blocker**: historical pinned C03 package unchanged; governed revision must select fixed-home Budget Authority ReleaseOnce, issuer settlement no-credit. No silent repinning or implied approval |
| FormalFenceReview | Fence may persist before assignment; abort cannot restore old epoch | Corrected via durable non-abortable FenceStarted before any fence I/O; abort serializes against FenceStarted, uncertainty recovers same A forward. DO-13 aligned on re-review |
| ByteContractReview | Candidate lacked FenceStarted transition | Added proposed kind11 and immutable map entry; kind4 references kind11; kind7 races kind11, never fence-after-abort |
| ByteLayoutReview | Watermark not bound to header predecessor | Added exact journal/genesis/digest/revision+1 equality for transition watermark, distinct nested source-snapshot role |
| ByteLayoutReview | Manifest could combine different O/debit/vector facts | Added exact O/D/reserve/fact/reference equality; post-resize current snapshot remains explicitly blocked pending schema |
| ByteLayoutReview | Outer/inner vector profile mismatch possible | Added exact VectorRef/manifest/admitted profile and role equality |

FormalOwnershipReview re-read the local corrections and found no remaining local semantic accounting blocker, but C03 mismatch prevents mutual approval. FormalFenceReview re-read B2F and found the original fence safety blocker resolved; its remaining DO-13 alignment correction was applied. Byte findings were integrated by the parent; no final independent byte-candidate PASS or maintainer approval is claimed.

Rollback for these findings before implementation is documented revision/rejection with historical evidence retained. No runtime side effects occurred. After any future durable custody/debit/fence operation, reverting bytes/code would not reverse effects: admission-closed authorized forward recovery is mandatory.

## Candidate and admission ceiling

DCO1 is a separately proposed packed integrity-record family, not a runtime replacement for OGJ1/ASTR/KMS/V1. Header216/trailer32, exact fixed structure layouts, canonical lists/maps, transaction identities and binding checks are documented. [Layout arithmetic receipt](../../build/durable-charge-byte-layout-checks.json) records structural computations only. It is not a codec implementation or proof of durable/authentic record acceptance.

Missing actual issuer/successor/verifier assignments, enforceable fence producers, storage-owner backend/floor admission, external proof/closure/completeness/current-resize carrier schemas and measured finite profile prevent complete exact CP1 candidate closure. The assignment scout inspected source/config/decision evidence and found engineering seams/development runs, not deployment grants; see assignment register for bounded search and required decision artifacts. No keys/devices/endpoints are invented.

Required next owner actions: governed C03 package revision and dependent manifest review; authorized public-pin/role/home/succession decisions; actual backend/floor tuple with failure evidence; complete authentic semantic carriers/current-allocation snapshot; independently finite storage/work/caller closure; explicit exact-digest CP1 decision before implementation and later CP2 evidence. Until then all adoption mutation/release routes remain closed.

[C03 amendment candidate](c03-release-amendment-candidate.md) now prepares exact successor wording and a governed integration sequence for the release/custody conflict. Preparation does not close the upstream finding, revise historical pins or constitute approval.
