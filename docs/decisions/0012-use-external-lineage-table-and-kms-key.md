# ADR-0012: Use an external lineage table and KMS key for signed-time epochs

- **Status:** Accepted
- **Date:** 2026-08-31

## Context

A DynamoDB point-in-time restore creates a new table from an older allocator
state. If that table can continue under the old `source_epoch` and response
signing key, it can produce fresh, valid signatures from a forked sequence and
Unix floor. Device-local floors reject some regressions but cannot identify one
global allocator branch or prevent two device populations from following
different same-epoch forks.

The lineage authority must therefore remain outside the allocator table and its
restore set. It must select one active allocator incarnation, make every signed
allocation and exact-receipt recovery depend on that selection, and make every
restore or fork advance the source epoch. This remains a `DEV_REFERENCE` AWS
control-plane contract; it is not a hardware rollback root.

AWS documents that DynamoDB transactions can include items from multiple tables
in one account and Region. A restore creates a new table rather than overwriting
an existing table. DynamoDB ARNs are name-based, so a deleted and recreated
same-name table can reuse the ARN text; `DescribeTable.TableId` is the resource
incarnation identity that must also be pinned.

## Decision Drivers

- Reject restored and forked allocator tables before recovery, allocation, or
  signing can release a fact.
- Select at most one active allocator incarnation and one response signing key
  for each source epoch.
- Keep lineage state outside every allocator backup, restore, and alias switch.
- Make transition history independently authenticated and reviewable.
- Preserve fail-closed behavior through crashes at every transition edge.
- Avoid requiring unavailable STM32/TPM hardware for the Phase 5 software lane.
- Keep AWS administrators and the transition operator explicit in the
  `DEV_REFERENCE` trusted computing base.

## Considered Options

### Option A (chosen): Dedicated DynamoDB control table plus KMS lineage key

A retained, deletion-protected lineage table holds one active signed head. A
dedicated P-256 KMS lineage key signs canonical epoch transitions but is never
usable by the signed-time Lambda for `kms:Sign`. DynamoDB CAS selects one child
head, while the signature authenticates the transition record.

This option composes with the existing one-Region DynamoDB transactions and can
be exercised before hardware admission. It does not make AWS administration a
production-grade rollback root.

### Option B: STM32 TPM/NV counter authority

A protected TPM counter could authorize each epoch transition with a stronger
non-AWS root. It is rejected for Phase 5 because it couples cloud recovery to
unavailable Phase 4–6 hardware and a new online/offline protocol. It remains a
candidate for production admission.

### Option C: S3 Object Lock checkpoint chain

Object Lock can retain immutable objects, but a correct single-child CAS,
retention/recovery ceremony, and runtime head lookup would add a second storage
protocol without improving the current development evidence ceiling. It is not
selected.

## Decision

Phase 5 will use two distinct DynamoDB tables in the same AWS account and Region:

1. the restorable allocator table, containing registrations, allocator state,
   and receipts; and
2. the lineage table, created and retained outside the allocator stack's restore
   and switch operations.

The reviewed manifest pins both table names and both `DescribeTable.TableId`
UUIDs. A restored or recreated table has a different `TableId` and is rejected
until an authorized transition records it. Pinning an ARN alone is insufficient.

The lineage table contains exactly one head item at
`lineage#cellos-dev-time-v1/head`. Its value is one canonical signed transition
with:

- schema and source ID;
- `source_epoch`;
- the previous signed-transition digest, or 32 zero bytes for genesis;
- allocator table name and `TableId`;
- response KMS key ID and canonical DER-SPKI SHA-256 digest; and
- reason: `initialize`, `restore`, `fork`, or `key_rotation`.

The signature is strict low-S DER ECDSA P-256 over SHA-256 of the canonical CBOR
transition fields. The digest chained by the next transition is SHA-256 of the
complete canonical signed transition. Genesis is epoch 1 with a zero parent.
Every later transition must be exactly `prior_epoch + 1`, bind the exact prior
digest, and use a newly created response signing key. Epoch reuse, skipped or
maximum epochs, same response key, malformed signatures, or alternate parents
fail.

A dedicated retained P-256 KMS lineage key authenticates transitions. The
runtime role receives `kms:GetPublicKey` only for this key and never
`kms:Sign`. A separate lineage-transition role, under the project permissions
boundary and explicit operator authorization, may sign transitions. That role
cannot invoke the signed-time handler or sign normal time responses. It may
update only the exact lineage head and allocator source-state key needed by the
reviewed transition ceremony. The response key and lineage key are never the
same resource.

The manifest carries the signed transition plus the lineage key ID and public
key digest. Startup loads and pins the lineage public key, verifies the
transition, verifies both table identities with `DescribeTable`, and derives one
immutable runtime lineage contract. No network request, allocation, recovery,
or KMS response signature is allowed before that contract is established.
Post-genesis cold start authenticates the current signed head directly; it does
not require the prior Lambda version or prior transition bytes to become
available. Exact direct-parent adjacency is enforced when the child is admitted
against the current contract and when its head CAS is built. Immutable release
evidence must retain each parent/child transition pair so the digest chain stays
auditable after the single live head advances.

Each transactional snapshot and receipt-recovery read includes the exact lineage
head from the separate table and compares its canonical bytes with the manifest.
Each allocation `TransactWriteItems` begins with a `ConditionCheck` for that same
head. DynamoDB evaluates that check atomically with allocator CAS and receipt
creation across the two tables. Transaction authorization uses the underlying
`GetItem`, `PutItem`, and `ConditionCheckItem` permissions. Runtime receives
`DescribeTable` for identity verification, `GetItem` on the lineage table only
when enclosed by `TransactGetItems`, and `ConditionCheckItem` only when enclosed
by `TransactWriteItems`; it receives no lineage-table `PutItem`, `UpdateItem`,
or `DeleteItem`. A losing, stale, or
mutated branch therefore releases no response and cannot replace the selected
head.

## Transition Ceremony

Every restore, fork, or response-key rotation uses this order:

1. Set API reserved concurrency to zero and drain in-flight invocations.
2. Disable the current response KMS key and prove the old qualified Lambda
   version cannot sign.
3. Select the candidate allocator: restore/create it under a new name and record
   its new `TableId` for restore/fork, or retain the exact current table for a
   response-key-only rotation. Never switch the lineage table.
4. Before any data mutation, apply the reviewed CloudFormation
   change-set/resource-import step that makes logical `SignedTimeTable` and the
   transition-role's exact allocator `UpdateItem` resource resolve to that
   candidate. Do not move the API alias. Verify the live candidate `TableId`.
5. Create a new response KMS key and immutable manifest candidate with epoch
   `N + 1`.
6. Have the authorized transition role sign the canonical child transition.
7. CAS only the candidate's exact allocator source-state record from epoch `N`
   to `N + 1`, preserving source sequence and Unix floor.
   Wrong/missing/restored-again state is a hard stop.
8. CAS the lineage head from the exact parent digest/epoch to the child; one
   competing child may win.
9. Publish a code-signed immutable Lambda version pinned to the child manifest,
   table IDs, and new response key; then move the alias and restore concurrency.

Before step 8 the old head remains authoritative, although the disabled key and
quiesced API keep it sealed. A crash after step 7 leaves the old/current branch
sealed and the candidate unselected (or its state one epoch ahead for a pure key
rotation). Recovery repeats the same exact state CAS or proves it already
completed, then resumes the parent-head CAS. After step 8 every old/restored
branch fails its lineage check; a crash leaves the service unavailable until the
exact child is deployed. Availability is never recovered by reverting the head
or re-enabling an old key.

A code-only Lambda rollback that retains the same allocator `TableId`, source
epoch, response key, and lineage head does not create a transition. Any table
restore, table recreation, fork selection, or response-key replacement does.

## Security Boundary

The transition role, KMS key-policy administrator, IAM administrator, and
principals able to apply the candidate-table change set, disable deletion
protection, or mutate the lineage table are explicit `DEV_REFERENCE` TCB. The
transition role's exact-key allocator epoch-migration write is part of that TCB.
Runtime receives `DescribeTable` plus transactional `GetItem` and
`ConditionCheckItem` on the lineage table and cannot sign with the lineage key.
Policies and live negative probes must deny lineage mutation and lineage-key
signing to deployment and runtime principals. A malicious authorized AWS
administrator can still violate this development contract; this ADR does not
convert AWS control-plane governance into a production rollback root.

The lineage table is retained, deletion-protected, PITR-enabled, excluded from
allocator restore procedures, and pinned by `TableId`. Restoring or recreating
it is a hard stop requiring a new reviewed authority decision; it is never a
normal recovery action.

## Evidence Ceiling

Pure transition, codec, signature, table-identity, transaction, restored-table,
and alternating-fork tests are `SOFTWARE_HARNESS` evidence. Completion still
requires an authorized AWS account/Region and live evidence that:

- restored and recreated allocator tables have different `TableId` values;
- one lineage CAS wins and every losing/old branch fails before KMS signing;
- the runtime role cannot sign with the lineage key, mutate the lineage head,
  forge an admitted head, or regain use of a disabled old response key;
- the transition role cannot sign time responses or mutate runtime code/IAM;
- crashes at each ceremony edge recover only by completing the selected child
  or remain sealed; and
- old response keys stay disabled.

None of this satisfies a production rollback, hardware root, or release gate.

## Consequences

### Positive

- A DynamoDB allocator restore no longer restores its authority to sign.
- One external CAS head selects one allocator table incarnation and epoch.
- Signed transitions provide a portable, reviewable chain when immutable
  release evidence retains every parent/child pair independently of table
  contents.
- The existing response schema and protected-device epoch/floor checks remain
  unchanged.

### Costs and Risks

- Normal allocation and recovery now depend on a second DynamoDB table.
- Restore requires a new response key, manifest, signed transition, immutable
  Lambda version, and explicit operator ceremony.
- The development trust base includes lineage-table and lineage-key
  administrators.
- Immutable release evidence must retain every signed parent/child transition;
  the live lineage table intentionally stores only the selected head.
- Loss or corruption of the lineage table seals the service; allocator backup
  restoration cannot repair it.

## Links

- [ADR-0007](./0007-development-first-hardware-constrained-execution.md) — all results remain bounded to `DEV_REFERENCE`.
- [ADR-0011](./0011-use-cloudflare-roughtime-for-dev-signed-time.md) — selects the upstream authenticated-time provider profile.
- [Phase 5 plan](../../.agents/260826-1605-phase4-dev-reference-authority/phase-05-nonce-bound-signed-time-service.md) — owns implementation and live fault evidence.
- [DynamoDB transactions](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/transaction-apis.html) — multi-table transactional scope.
- [Using IAM with DynamoDB transactions](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/transaction-apis-iam.html) — transaction subactions use underlying item permissions; lineage runtime access is restricted to enclosed `GetItem` and `ConditionCheckItem`.
- [DynamoDB point-in-time restore](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/pointintimerecovery_restores.html) — restore creates a new table and does not carry all settings.
- [DynamoDB `CreateTable`](https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_CreateTable.html) — table naming and resource creation contract.
