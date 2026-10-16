# C03 release authority amendment — candidate, not adopted

## Status and scope

Proposed governed successor to the release-related wording in `exact-deltas/C03.md` r1. The historical package remains unchanged. This proposal does not assign a principal, approve persistent bytes, repin the package, authorize implementation or promote any checkpoint/lock. Engineering integration owners remain P01/P06/P08-C; those labels are not runtime authorities.

Problem: C03 `Unknown -> Closed` permits “Authority/issuer-owned release”, while the durable ownership contract permits only the fixed-home Budget Authority to credit the application allowance. Lost settlement replies must not create an alternate issuer refund path.

## Exact proposed semantic replacements

| C03 location | Successor wording |
|---|---|
| State transition `Unknown -> Closed` | Only authentic authoritative absence/Gone, durable key closure and local quiescence permit the fixed-home Budget Authority's durable idempotent ReleaseOnce for the exact original debit and closure evidence. The genesis issuer may settle provenance but must not credit/refund the application allowance. Timeout, empty lookup, task restart or partition is insufficient. An uncertain ReleaseOnce reconciles its original stable key; no replacement release key. |
| Units, bounds and charge owners — final ownership sentences | Issuer owns immutable debit provenance and precommit custody until durable acceptance by the assigned fixed-home Budget Authority. Acceptance transfers sole effective custody even when issuer settlement or its reply is lost; issuer retains nonspendable provenance/redirect only. Supervisor consumes; destination owns keyed local publication. The recovered fixed-home Budget Authority alone enforces the application balance and commits ReleaseOnce. Issuer settlement is no-credit. P01/P03/P08 provide measured control and durable high-water/retention bounds; no numerical profile selected. |

These replacements refine genesis custody and release only. They do not permit ledger-home relocation, a second maximum, issuer fallback refunds, timeout promotion or adoption mutation before admitted recovery/fencing.

## Required consistency and failure witnesses

1. The same immutable debit origin survives issuer precommit, destination acceptance, source settlement and final closure; each owner-local transaction has its own stable transaction identity.
2. Before acceptance, destination prepare is nonspendable; after acceptance, the issuer cannot resume custody merely because its settlement/reply is absent.
3. All pending/unknown publication remains charged. Closure evidence cannot by itself credit issuer and Authority independently.
4. Repeated ReleaseOnce, lost commit reply, delayed issuer settlement and replay after restart cause at most one credit in the fixed-home ledger. An uncertain outcome remains charged/reconciliation-required until authentic lookup establishes its exact durable result.
5. FenceStarted and abort serialize; after FenceStarted, recovery proceeds under the same adoption identity without restoring old-epoch permission. This amendment does not substitute a release for forward recovery.
6. Missing/corrupt/rolled-back durable state closes mutations. No fallback empty ledger, local quota deregistration, fixture response or boot-local epoch proves global credit authority.

These are required future runtime witnesses, not observed test results.

## Governed integration sequence

1. P01/P06/P08-C review this semantic choice against C03, C04 reserve/closure, C01 fixed-home identities and the durable ownership contract. Record objections and an explicit scoped decision; technical review is not operator authority.
2. Resolve the external issuer/home/successor public-pin, enforceable fence producer, backend/floor and finite-profile packet from the assignment evidence register. Do not fill absent fields with engineering owner names or development fixtures.
3. Prepare a successor C03 package revision containing both replacements and exact complete byte/proof/caller/error/retention changes. Preserve r1 and its receipts as historical evidence; do not silently overwrite a pinned predecessor.
4. Recompute the successor package and dependent manifest digests using the repository's governed checkpoint procedure. Review all changed dependencies and caller closure before asking for an exact-digest CP1 decision. A graph verifier PASS alone is not consent.
5. Only after the authorized exact decision and qualified G0–G2 prerequisites may implementation proceed. CP2 still requires actual backend, crash/restart/fence/ReleaseOnce traces and measured charged envelopes.

No step above is recorded as completed by this amendment. C03 remains an open upstream conflict until the governed successor is approved and integrated. The assignment register and incomplete semantic carriers remain independent blockers even after that conflict is resolved.

## Companion evidence

- [Formal review and unresolved findings](durable-charge-formal-review.md)
- [Durable ownership/adoption semantics](durable-charge-ownership-adoption-contract.md)
- [Canonical byte candidate, incomplete proof/current-resize carriers](durable-charge-canonical-byte-candidate.md)
- [Actual assignment investigation and missing decision packet](durable-charge-assignment-evidence.md)
