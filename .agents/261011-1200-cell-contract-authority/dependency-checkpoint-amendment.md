# Dependency-scoped checkpoint amendment

Recorded 2026-10-14. Status: proposed governance amendment. User selected dependency-scoped checkpoints instead of retaining the monolithic evidence dependency. This selects the approach only: no exact delta, implementation or lock release is approved. The 23 existing CP1/CP2 records and 38 locks remain unchanged.

## Problem and decision

[Measured-profile inputs](measured-profile-candidate.md) require measurements of aggregate CPU, retained operations and actual control/journal producers. [Issuer/persistence inputs](issuer-persistence-candidate.md) require concrete authoritative producers and backend/fence qualification. Requiring all these implementation measurements before authorizing every prerequisite implementation creates a cycle.

Split authorization by exact dependency, never by relabeling missing measurements. Each prerequisite has its own complete bounded candidate and explicit owner review before implementation. Its measured evidence feeds downstream candidate preparation. Downstream interfaces, production admission and protected Build gates remain closed. This amendment does not itself satisfy a prerequisite candidate.

## Ordered checkpoints and artifact contracts

| Slice | Exact pre-implementation candidate | Post-implementation evidence | Downstream use |
|---|---|---|---|
| G0 Private measurement | Exact source diff, private build/image scope, hook/lock/identity/overflow/close protocol, structural storage bound and observer access review; reuse existing instrumentation contracts | Normal-image exclusion, actual coherent generation/allocator/physical transitions, overflow/refusal controls and source/image/decoder receipts | Event-complete attribution, not production budget selection |
| G1 Local accounting | Exact charging algorithm/timebase/error, switch/tick boundaries, period/burst/priority recurrence and bounded overshoot, storage/admission/retirement; full caller/error matrix and independent finite storage basis | Actual distinct-CPU execution, boundary/replenishment/throttle/refusal and retained lifetime traces | Local CPU/thread/resource profile; no application issuer authority |
| G2 Ordinary persistence | Exact non-format recovery, original write/sync error propagation, accepted/unknown I/O semantics, immutable journal encoding, barriers/recovery/compaction; named backend tuple and storage-owner decision | Crash/tear/short-write/flush/reply-loss at every D1/D2 edge, reopened recovery, floor/fence nonregression and bounded retained history | Ordinary journal qualification only; not protected rollback resistance |
| G3 Authority/lifecycle | Authorized issuer roster/public pins and genesis, complete keyed kernel/destination/Authority interfaces, canonical records and full-vector charge/fence/unknown protocol; measured G0–G2 basis | Real reserve/live claim/key publication/closure/resize/restart/delayed-create evidence with charge retained under uncertainty | Actual control overhead and cluster candidate basis |
| G4 Downstream exact candidates | Complete D/C byte/layout/version/error/lifetime/migration/profile tables and resolved reference/provider denominator using qualified upstream receipts | Original implementing phase supplies its own CP2 evidence | Maintainer CP1 on exact revision/digest; later CP2, never inherited approval |

G0 and G1/G2 candidate design can be prepared independently. G1/G2 implementation starts only after the relevant exact prerequisite decision. G3 requires admitted trust/backend inputs and qualified prerequisites; it is not a fixture route around them. G4 packages remain pending until their own dependencies are resolved.

## Authorization and evidence rules

- WHEN an owner reviews a prerequisite, the record SHALL identify exact candidate revision/digest, source paths, permitted image/lane, change and invariant boundaries, reviewer decision and dependent packages. A generic instruction or this lane selection is insufficient.
- IF public or persistent bytes change, the prerequisite SHALL obtain its own Law-1 CP1 before editing and CP2 before freeze; splitting does not waive either checkpoint.
- IF a slice needs a locked path, the controlling owner SHALL record the exact scoped handoff and reciprocal lock disposition before implementation. No blanket lock removal or automatic phase completion.
- Measurement prerequisites SHALL have independently proven finite instrumentation/storage bounds. Diagnostic all-path fit must not rely on the unmeasured production profile it is meant to establish.
- Missing issuer assignment, public pins, backend/floor tuple, hardware/time or external SDK evidence SHALL remain explicit blockers. Private keys are neither requested nor written into artifacts.
- Protected original AC-001..AC-011 Build entry, AC-012, exact-product NO-GO, relay-first/direct-LAN and production gates SHALL remain unchanged. Ordinary filesystem flush tests cannot satisfy them.
- Rejected or uncertain evidence SHALL retain raw captures and failure receipts. No favorable-run replacement, arbitrary limits, counter-reset release or test signer promotion.

## Amendment acceptance and current boundary

The selected dependency approach is recorded, but exact G0–G3 candidates and per-slice consent are not yet granted. Review this amendment against the controlling P01 and lock owners before changing their requirements. If rejected, retain the existing gates and documents. If adopted, record a dated controlling amendment and explicit per-slice ledger entries; do not overwrite historical approvals.

Evidence still requiring external inputs: A1 issuer admission roster; A2 provisioned protected authority/time and hardware witnesses; A3 admitted storage/device/floor tuple; A6 authorized source/state sets; external parts of A8. The repository and local QEMU lane cannot manufacture these inputs. This amendment separates reachable implementation preparation from unavailable deployment evidence; it does not assert candidate closure.
