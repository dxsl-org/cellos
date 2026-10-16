# Durable charge blocker closure register

## Decision ceiling

This register distinguishes contract approval/preparation from operational admission. The [explicit user decision](durable-charge-user-approval.json) approves the presented contracts/choices and nonproduction VMware measurements, with exact reviewed-document pins and full predecessor C03 r1/C05 r2 bytes retained. Current preparations integrate those choices as C03 r2/C05 r3 with updated manifest pins. No runtime, complete CP1/CP2, phase or lock promotion follows; actual external assignments and qualification evidence are not inferred. Historical candidate wording such as “unapproved” describes its preparation state; this scoped decision supersedes that status only for the presented contract/choices.

## Blocker disposition

| Blocker | Local deliverable / exact disposition | Required remaining evidence |
|---|---|---|
| Resize decomposition and current charge source | User-approved same O/D, pending entry, one outstanding resize; current snapshot rather than original reserve vector | Authentic current-state index/floor, complete exact package approval and implementation remain required |
| Current snapshot/partial-credit internal byte ambiguity | Presented kinds48–58 and their layouts/lifecycle/lookup/adoption choices approved by user; independent findings resolved | Complete CP1 package and real external proof/backend/profile admission still required |
| Mandatory proof semantics/coverage/order | Presented local proof acceptance requirements approved by user | Exact authentic external carrier bytes, actual per-role producer/verifier assignments and qualification remain OPEN |
| C03 issuer credit alternative | CLOSED for semantic selection/integration: user approves fixed-home-only credit; C03 r2 applies choice, manifest repinned, predecessor retained | Complete authentic genesis/consume/proof/backend/profile/caller inputs and CP1/CP2 still OPEN |
| C05 equal global/local new-debit formula | CLOSED for semantic selection/integration: user approves G=max(Q−B,0) versus L=max(Q−E,0); C05 r3 applies choice, manifest repinned, predecessor retained | Actual local/global reservation, proof, profile/caller and CP1/CP2 evidence still OPEN; charged allowance is not local authority |
| Actual issuer/home/successor/public pins/fence gates | LAB CLOSED: real guest-generated per-scenario Ed25519 role assignments and centralized lab fence/gate producers are pinned in [final lab assignments](durable-charge-vmware-lab-final-assignment.json) | NATIVE/PRODUCTION OPEN: these lab roles share one guest/root domain and are not Cellos deployment/protected authority grants |
| Actual backend/floor and physical failure behavior | LAB CLOSED for ordinary selected virtual-volume crash/restart: dedicated ext4 VMDK + actual SQLite DELETE/FULL, thirteen hard-stop/recovery pairs | OPEN for native DCO1 backend, independent protected floor, physical power loss, actual device/cache/IOMMU qualification |
| Finite profile and caller closure | Lab caps, retained-tail/aggregate-cap refusal and measured runtime/database/resource values exercised; old P01 accounting blockers retained unchanged | OPEN for attributable native workload/control-service profile and complete production caller/artifact closure; lab samples/caps are not production bounds |
| Runtime implementation and fault witnesses | LAB CLOSED: isolated Linux JSON/SQLite end-to-end runtime, eight normal witnesses and thirteen hard-stop/recovery pairs PASS; [evidence](durable-charge-vmware-lab-evidence.md) | NATIVE OPEN behind actual G0–G2/complete CP1: DCO1 packed codec and Cellos publication/resource/state/effect integrations, later native/production CP2 |

## Candidate set

- [Allocation byte successor s1](durable-charge-allocation-byte-successor.md)
- [Current-allocation semantic alignment](durable-charge-current-allocation-candidate.md)
- [Proof acceptance contract and PA-01–PA-12 evidence matrix](durable-charge-proof-acceptance-contract.md)
- [C03 governed amendment](c03-release-amendment-candidate.md)
- [Actual assignment evidence](durable-charge-assignment-evidence.md)

## Independent technical review disposition

| Reviewer / original defect | Applied correction | Focused re-review |
|---|---|---|
| AllocationAudit: Closed current snapshot could not be a kind51 lookup-evidence subject | Allow kind58; kind56 evidence subject equals current_snapshot and includes complete role12, including Closed | Original finding resolved |
| AllocationAudit: lifecycle table rejected mapped Unknown readiness refresh | Tag3 explicitly permits only accepted map-bound ApplicationUnknown → ApplicationUnknown refresh, preserving vectors/state/keys/prior work | Original finding resolved |
| ProofAudit: late source final-release commit could invalidate mandatory materialization map | Resolve all admitted source allocation transactions before manifest selection; freeze selected indices; FenceStarted atomically closes old allocation commits before fence I/O. Only external outcomes may arrive late | Original finding resolved |
| ProofAudit: B1 permitted re-preparation after FenceStarted | Abort/fresh preparation only before FenceStarted; after possible commit retain same A/target and reconcile forward | Original finding resolved |

Both reviewers re-read the focused corrections and reported their original findings resolved. This is a technical finding disposition, not approval of authentic proof bytes, authority/backend admission, or complete runtime qualification. The previously missing local snapshot/decomposition/partial-credit/adoption/lookup candidate definitions are supplied; all OPEN rows above retain their stated evidence requirements.

The successor cannot be enabled partially through an r1 parser, generic signed blob or unsigned structural fixture. Rejection before implementation changes documentation only. Any future durable debit/fence/effect requires admission-closed authorized forward recovery, not binary rollback or recreation of the ledger.
