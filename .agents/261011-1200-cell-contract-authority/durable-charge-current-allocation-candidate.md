# Durable current-allocation snapshot — semantic candidate

## Status

Unapproved refinement of the durable ownership contract. The [allocation-byte successor s1](durable-charge-allocation-byte-successor.md) now selects a same-O pending-entry decomposition and supplies proposed current-snapshot, partial-credit, lifecycle, adoption and lookup bytes. The [proof acceptance contract](durable-charge-proof-acceptance-contract.md) specifies mandatory semantic proof bindings and coverage. These are candidates, not authentic external proof carriers, actual authority assignments or CP1 approval. DCO1 r1 remains restricted to unresized reserve snapshots. No runtime state or historical pins change.

## Identity and authority

The original reserve permanently binds O, debit origin D, creation key, original operation/destination binding and original vector profile. It is provenance, not an assertion that its initial vector is still the current charge. A separate authenticated current snapshot binds that exact reserve identity and the current fixed-home charge-ledger revision, committed digest and admitted nonregression floor. An object ID, largest observed revision or valid hash alone cannot select the current snapshot.

The snapshot must also bind the current custodian/epoch, preceding allocation snapshot, allocation revision, immutable profile/dimensions/units, base charged vector B, pending positive vector P, effective resource allocation E, active resize key and its immutable request/target vector Q when present, retained outcome/closure references and the exact charged control-capacity reservation. Distinct allocation and journal revisions are explicit; no numerical equality between them is assumed. Adoption identity A binds this snapshot and all dependent facts in its complete manifest.

One outstanding resize per O is selected for this candidate. A distinct second resize refuses while the first remains unresolved; retries reconcile the same key and request. This is a proposed serialization restriction, not a claim about existing runtime. Concurrent resize support would require a separately reviewed decomposition; it cannot silently share P.

## Charge transitions

All arithmetic is dimension-wise, checked and profile-bound. Effective allocation never proves permission or credit by itself.

| Transition | Required durable accounting |
|---|---|
| Stable / credit pending | Settled Stable has P=0 and B=E. After application, E may be smaller than B while shrink credit awaits qualification; s1 names this AppliedCreditPending, retains the outstanding resize and refuses a distinct new resize until settlement. |
| Reserve resize Q | At a serialized input snapshot, set global pending debit P=max(Q−B,0), and independently reserve destination-local growth L=max(Q−E,0) under its admission gate; retain B and the exact resize key/request. Reserve metadata/work capacity before dispatch. C=B+P is globally charged once; L is local enforcement capacity, not a second global debit. Both reservations must be proven before possible application. |
| Possible application / Unknown | Keep C unchanged and retain all possibly admitted work/effect facts. Uncertain application does not prove E=Q or permit release. |
| Confirm whole-vector application | Only authentic owner-specific application evidence sets E=Q. Atomically fold B'=B+P and P'=0. C is unchanged; componentwise shrink is not credited by this fold. |
| Safe shrink credit | After all affected owners prove enforcement/quiescence for the exact retired coverage, the fixed-home ledger atomically records an idempotent installment S with 0<=S<=B−Q and reduces B by S. The resize remains pending until B=Q; only then is it Stable. This is not full obligation closure or key reuse. Unknown credit reconciles the same adjustment; no second credit. |
| Proven no application | Only authoritative nonacceptance/closure of every possible resize effect permits retiring the reserved positive delta under its stable operation identity. Timeout or a missing response is insufficient. Original B remains charged. |

A complete ordinary resize lifecycle must distinguish partial shrink credit from final O closure. The successor's proposed kinds50/48 encode adjustment request/result and kinds57/58 encode final release, rather than reusing r1 kind10. This selects the previously unresolved local decomposition; authentic qualification proof bytes, assignments, governed upstream acceptance and runtime evidence remain separate blockers.

If Q is below conservative B but above a previously reduced E, unused globally charged allowance is not a resource-owner grant: local L remains mandatory even when P=0. No negative P and no offsetting shrink in another dimension. C05's growth-from-current-allocation rule continues to govern destination-local admission; the global debit decomposition requires explicit successor review rather than silently redefining C05 r2.

## Adoption binding rules

1. Snapshot O/D/create key and reserve reference equal the immutable reserve. Profile, dimensions and units equal the admitted profile at every vector reference.
2. Ownership manifest base/pending references equal the selected current snapshot, not the original reserve vectors. The current snapshot authenticates its predecessor chain or an admitted complete checkpoint; a caller-supplied truncation is insufficient.
3. The selected source watermark authenticates a complete source snapshot containing this exact current allocation and all pending resize, application, credit and Unknown outcomes. Source prepare serializes against allocation changes: any intervening change invalidates preparation and requires a new reviewed attempt before FenceStarted. No attempt may mutate the frozen manifest after that boundary.
4. Destination acceptance preserves B/P, active operation keys, unresolved outcomes and total charge exactly. Adoption neither folds an unproved resize nor credits a pending shrink.
5. FenceStarted freezes the transfer branch as in the ownership contract. Late old-owner application/credit evidence is reconciled by the authorized successor under retained original keys; it cannot activate old custody or disappear from the complete manifest.
6. Missing current snapshot, chain/completeness proof, floor, accepted credit result or required bound keeps adoption mutation-closed. Never fall back to the original reserve as current allocation.

## Required byte follow-through

The [allocation-byte successor s1](durable-charge-allocation-byte-successor.md) supplies proposed snapshot/request/evidence/registry/adoption/lookup/final-release kinds48–58, exact fields/offsets, predecessor/operation references, revision rules, vector role checks and canonical active/terminal rules. Its ownership entries bind the current snapshot, not old reserve vectors. Exact authentic external proof schemas, verifier assignments, governed C03/C05 successor acceptance and finite admitted limits remain prerequisites; no historical r1 meaning changes silently.

## Review and verification ceiling

Arithmetic witnesses: old5→7 reserves2, charge7 before/after fold; old(5,8)→(7,3) reserves(2,0), charge(7,8) before/after application and only(7,3) after qualified shrink credit; current B=(7,8), E=(7,3), next Q=(6,5) has P=(0,0) yet still needs resource admission for dimension2. These are model arithmetic, not durable execution or proof of safe credit.

Required later runtime witnesses include resize/adoption serialization, frozen-manifest late outcomes, lost application/credit replies, stale snapshot substitution, partial-credit replay, incorrect profile and overflow. No such runtime witness is claimed by this document.
