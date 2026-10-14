# Scoped G1/G2 design grant review

Recorded 2026-10-14. Two independent read-only reviews compared candidate designs with actual source. This is design review, not runtime verification or production approval. User explicitly requested review and scoped implementation authority to produce actual measurements. Governing [G1r2](g1-local-accounting-candidate.md), [G2r3](g2-ordinary-persistence-candidate.md), [dependency amendment](dependency-checkpoint-amendment.md).

## Findings and disposition

| ID | Source-backed risk | Decision and correction | Gate |
|---|---|---|---|
| G1-F1 | Scheduler same-TID returns no raw tuple; no incoming callback runs (`scheduler.rs:2173–2177`, `task.rs:1585–1590`). | Accepted clarification: previous text already excluded staging same-TID; r2 adds explicit service_no_switch and stages only a concrete raw tuple. | Corrected design; actual hooks must prove it |
| G1-F2 | Incoming callback runs on new stack before registers restore (`context.rs:171–199`); allocating retirement or returning on accounting failure is unsafe. | Fixed failure record plus allocation-free non-returning fatal path after raw commitment; precommit handling separate. | Corrected design; failure smoke required |
| G1-F3 | Reusing sole Cell row loses predecessor charge/debt/identity. | Non-evicting64complete terminal rows and count; retain occupied row/refuse reuse at fullness. Payload41304bytes. | Corrected independent storage design |
| G1-F4 | Requiring actual reviewed implementation diff before permission to write creates a cycle. | Reviewed design/path/private-scope authority before edits; actual diff/caller/lock review before capture. | Corrected governance |
| G1-F5 | 256events cannot be certified from an unbounded cooperative-yield schedule. | Keep fixed buffer; event schema/producer/coalescing and bounded action schedule proof are mandatory before capture, split runs if necessary. | Pre-capture blocker, not edit blocker |
| G2-F1 | Broad R/B language includes unselected Unknown quota lifetime and crash-preserving allocation/journal mechanisms. | R-core/B separated from G2-J; no journal/Unknown ownership permission inherited. | Partial grant only; J blocked |
| G2-F2 | `grant_read.rs:68–73` can convert fault to empty GrantDone. | Caller/hash added; authorization then pre/post health, no copy/GrantDone on fault. | Corrected caller obligation |
| G2-F3 | Missing extent mappings/partial count can create false EOF. | Process every extent block, require exact expected in-file read count; corruption poisons/ERR_IO. | Corrected functional result obligation |
| G2-F4 | Rename `.is_ok()` treats non-NotFound error as absent; duplicate insert can follow. | Exact NotFound-only destination lookup; all other errors propagate; insert independently rejects duplicate name. | Corrected namespace obligation |
| G2-F5 | Unlink discards allocator.free_block errors. | Validate reachable ranges and propagate every release failure; poison/ERR_IO; failure does not refund quota. | Corrected error handling, not crash preservation |

All corrections are proposed implementation obligations, not claims the source bugs are fixed. No review worker ran tests/builds or wrote runtime code. G2-J has explicit unresolved exact preservation/retention/body/floor design and receives no implementation grant. HPET accuracy/liveness, end-to-end D/W, issuer/backend/device/floor admission and physical/protected proofs remain qualification gates.

## Scope and rollback

G1: Intel private accounting edits only, disabled in normal images; public telemetry/ABI/persistent state unchanged. G2: R-core/B development source/provisioning preparation only, disposable/staged explicitly selected images; V1 bytes and existing VFS wire unchanged. Unsupported target work remains parked; cross-target tooling changes may only preserve image-generation compatibility, never authorize non-Intel qualification.

Before execution, revert only slice-created source/config/tooling changes. After fresh disposable image provisioning, discard that image. After retained data mutation, preserve bytes/obligations and recover forward with admission closed; never restore destructive format-on-open-error as recovery. Review/authorization cannot reverse disk writes, externally accepted effects or floors.

Actual implementation diff review is required before runtime measurement interpretation. A successful design check or scoped grant does not complete P01 or downstream CP1/CP2.

## Corrected-design reviewer results

G1 follow-up: corrected design sufficient for narrow Intel private source-edit grant; no remaining edit blockers. Event schedule/static buffer proof and end-to-end D remain pre-capture gates. G2 follow-up found residual operative retention/reclamation clauses; r3 explicitly labels them G2-J-only and restricts R-core to poison-ends-mutation/read-only reopen/discard. Final follow-up: no remaining findings for R-core/B edit scope; journal/production/backend admission remains blocked. Neither result is actual implementation review or measured runtime PASS.
