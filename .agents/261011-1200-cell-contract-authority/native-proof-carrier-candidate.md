# Native signed-fact carrier — NCF1 candidate n1

2026-10-15. **New native implementation candidate, not an approved issuer assignment, complete CP1 packet, backend/floor admission or runtime cutover.** The standalone allocation-free `libs/durable-charge-codec/src/proof.rs` implements verification of these bytes. Host regression fixtures contain deliberately public deterministic test seeds only. No real native public pin, principal, floor, complete owner set or permission is supplied by this document or the library. Historical [DCO1 r1](durable-charge-canonical-byte-candidate.md), [allocation successor s1](durable-charge-allocation-byte-successor.md), C03, existing wire kinds, locks and checkpoints are unchanged.

This candidate supplies a separately named fixed signed-result carrier for the byte/cryptographic part of [native step2](native-codec-implementation-contract.md), informed by the [proof acceptance requirements](durable-charge-proof-acceptance-contract.md). It does not retroactively approve the unresolved external proof allocations referenced by DCO1. In particular NCF1 is **not** a DCO1 record kind, cannot be passed to the DCO1 decoder, and is not silently accepted as a Ref40 referent by the DCO1 dependency validator. Mapping this candidate to actual owner-specific prerequisite sets, retained proof containers, native producers and consuming gates remains part of the exact package decision. No native runtime caller is switched.

## 1. Representation and signature boundary

Every carrier is exactly **1660 bytes**: fixed header32, fixed body1564, Ed25519 signature64. All integers are unsigned little-endian; packed offsets below are absolute from byte0; no ABI padding, extensions, TLVs, variable opaque statements, alternate suites, implicit defaults or trailing bytes exist. The body is fully typed by the field table. ID32 and ID16 are exact binary identifiers/content digests, not text or free-form evidence. An incompatible field/purpose/layout requires another reviewed carrier revision.

Suite1 is ordinary Ed25519 through the existing no-default-features `ed25519-compact` version2 provider. The exact Ed25519 message is:

`ASCII("cellos/native/signed-fact/v1") || 0x00 || carrier[0:1596]`

The signature occupies `[1596:1660]`; it is not in its own preimage. This is Ed25519 over the complete message, **not** Ed25519ph or signing a DCO1 trailer alone. Verification streams the domain then the signed bytes through `PublicKey::verify_incremental`, `absorb`, `verify`, retaining the provider's noncanonical-scalar/weak-public-key rejection. The carrier includes the exact public pin, but trust comes from the independently supplied expected pin, never from that included value. There is no runtime signing function, private-key field, key generation, random source, online signing oracle or embedded native pin.

|Offset|Field|Bytes|Exact rule|
|---|---|---|---|
|0|magic|4|ASCII `NCF1`|
|4|version:u16|2|1|
|6|suite:u16|2|1, Ed25519|
|8|total_length:u32|4|1660|
|12|body_length:u32|4|1564|
|16|assigned role:u16|2|§3 finite enum|
|18|purpose:u16|2|§3 finite enum and permitted pair|
|20|lane:u16|2|1 Native, 2 Fixture; exact expected lane|
|22|side:u16|2|0 Local, 1 Source, 2 Destination|
|24|status:u16|2|§3 finite enum, exact expected value|
|26|request/result:u16|2|1 Result only; requests are not this schema|
|28|reserved|4|all zero|
|32|producer:Owner|104|principal32, incarnation32, fixed home32, epoch:u64|
|136|key_id|32|Admitted key-purpose identity|
|168|roster_id|32|Assigned producer/verifier roster identity|
|200|roster_revision:u64|8|Exact independently current expected roster|
|208|revocation_revision:u64|8|Exact expected revocation/rollover basis|
|216|audience|32|Exact intended verifier identity|
|248|admission_decision|32|Exact owner-authorized decision identity|
|280|admission_revision:u64|8|Exact admission revision|
|288|policy_id|32|Exact governing policy|
|320|policy_revision:u64|8|Exact governing policy revision|
|328|application|32|Exact application scope|
|360|workload|32|Exact workload scope|
|392|scope_purpose|32|Exact method/scope identity, distinct from finite proof purpose|
|424|journal|32|Owner-local authoritative journal/ledger|
|456|genesis|32|Admitted initialized journal identity|
|488|volume|32|Actual assigned storage/volume identity, not a pathname|
|520|floor_namespace|32|Exact floor namespace/incarnation identity|
|552|obligation O|16|Exactly the bound record's O|
|568|adoption A|16|Exactly the bound record's A|
|584|local transaction T|16|Exactly the bound record's original owner-local T|
|600|origin_journal|32|Immutable debit origin, or explicit inapplicability below|
|632|origin_genesis|32|Immutable debit origin|
|664|origin_transaction|16|Original debit initiating T|
|680|origin_revision:u64|8|Original debit revision, separate counter domain|
|688|create_key|16|Original nonreused create key where O applies|
|704|operation_key|16|Distinct resize/adjustment/other operation key, zero if inapplicable|
|720|release_key|16|Stable release key, zero if inapplicable|
|736|transition_map|32|Immutable A transition-map identity; zero if A is absent|
|768|request_schema|32|Exact canonical operation/request schema|
|800|request_length:u64|8|Exact request byte length, including zero-length request if admitted|
|808|request_digest|32|Exact canonical request content binding|
|840|effect|32|Exact effect/method binding|
|872|profile|32|Complete admitted dimensions/units/profile identity|
|904|destination|32|Exact immutable destination/artifact/incarnation binding identity|
|936|requesting_endpoint|32|Authenticated requesting endpoint incarnation|
|968|responding_endpoint|32|Authenticated responding endpoint incarnation|
|1000|correlation|32|Exact operation/session/export/request correlation identity|
|1032|provenance:Owner|104|Separate immutable provenance owner identity/epoch|
|1136|custodian:Owner|104|Separate current custodian identity/epoch|
|1240|writer_epoch:u64|8|Exact bound journal header writer epoch|
|1248|authority_fence:u64|8|Current expected Authority enforcement fence|
|1256|controller_epoch:u64|8|Separate controller epoch, not a writer/fence alias|
|1264|custody_revision:u64|8|Expected custody decision revision|
|1272|allocation_revision:u64|8|Expected allocation revision, or explicit zero for no allocation revision|
|1280|input_revision:u64|8|Immediate already committed predecessor revision|
|1288|output_revision:u64|8|Exact bound record header revision, checked input+1|
|1296|input_floor_revision:u64|8|Input floor role, distinct domain|
|1304|output_floor_revision:u64|8|Output floor role, distinct domain|
|1312|input_record:RecordBinding|40|Prior encoded length:u32, kind:u16, reserved:u16=0, trailer32|
|1352|record:RecordBinding|40|Exact output/subject retained DCO1 length/kind/trailer|
|1392|record_schema|32|Exact DCO1 schema ID derived from bound kind|
|1424|coverage_roster|32|Exact independently assigned coverage/owner roster identity|
|1456|coverage_revision:u64|8|Exact coverage snapshot/cutoff revision|
|1464|coverage_count:u32|4|Exact expected count; zero is not absence/completeness proof|
|1468|checkpoint|32|Exact checkpoint/coverage/retained-dependency content binding|
|1500|backend_admission|32|Exact backend/persistence/recovery policy admission reference|
|1532|failure_model|32|Exact admitted failure/threat/rollback class identity|
|1564|public_key|32|Exact externally pinned Ed25519 public key|
|1596|signature|64|Ed25519 signature of domain and bytes0..1595|

Each Owner is principal32 at relative0, incarnation32 at32, home32 at64, positive epoch:u64 at96. Owner equality includes all104 bytes; producer, provenance and custodian are separate roles, not inferred aliases. The producer need not equal the journal writer/custodian; an admitted floor producer may be different. Their exact relationship is checked by the consuming owner policy, not invented by the codec.

All ID32 fields and required transaction/owner fields are nonzero except the explicitly absent debit origin/transition-map groups. O/A presence follows the exact DCO1 kind. When O is zero, the entire debit origin (including revision) and all three operation keys are exactly zero; this represents an aggregate journal/adoption statement, never a guessed representative O. When O is nonzero, origin ledger/genesis/T/revision and create key are mandatory nonzero. Operation/release key zero explicitly means not applicable, not authoritative nonacceptance. A zero requires transition_map zero; nonzero A requires a nonzero immutable map binding. All ordinary u64 epoch/revision fields reject zero and `u64::MAX`; origin revision is zero only with absent O; allocation revision can be zero only as an explicit no-allocation expectation and still rejects MAX. Checked input+1 equals output. No relative ordering is inferred between floor, allocation, custody, controller or journal counters. A floor currentness/advancement relation cannot be deduced by comparing unlike counters.

## 2. Independently supplied context and typed result

`VerifyContext` contains a fully typed expected `Bindings`, exact assigned public key, explicit finite `max_carrier_bytes`, and explicit finite DCO1 decode `Limits`. It has no production default or constructor that reads expected authority from the presented bytes. Consumers must obtain the role/purpose, current policy/roster/revocation, producer/incarnation/home, scope, custody/fence/revision, exact retained references, pin and coverage basis from their actual admitted protocols. A caller selecting an arbitrary expected key/context has only authenticated its arbitrary selection, not established authorization.

Verification checks the finite format/discriminants, permitted role/purpose pair, every header/body field against the expected context, reserved zeros and required identities/counters; both exact reference shapes; public-pin equality; then independently decodes the actual retained output bytes under the supplied limits. This reparse is necessary because public `Record` structural metadata is not an unforgeable seal. Bound length/kind/schema/trailer and header ledger/genesis/O/A/T/writer/revision/predecessor must match the decoded retained record, with checked contiguous journal revision. The Ed25519 signature then authenticates the exact canonical carrier and domain. Input reference content, complete predecessor/floor chains and graph/owner completeness still require independent retained-referent validation; naming a reference cannot prove its existence.

For every kind with an embedded Scope (all supported kinds except32/34/52), carrier application/workload/scope-purpose/lane/policy-revision must also equal the actual payload Scope. Kind32's profile must equal the carrier profile. Kinds1/10/48/49/50/57/58 additionally compare the actual DebitOrigin and create key; kinds48/58 compare the actual allocation/custody revisions and full current custodian. Indirect snapshot revisions and role-specific outcome/coverage relations still require retained-referent and actual consumer-state validation; they are not guessed from a matching signature.

`VerifiedFact` has private construction and borrows the original carrier, independently decoded record bytes and exact expected bindings. Its typed accessors expose role, purpose and status. It establishes **only that this exact assigned producer signed this exact statement**. There is deliberately no `permit`, `release`, `activate`, mutation callback or success-to-boolean conversion. Authenticated Unresolved remains Unresolved, not absence; even authenticated Committed is only the assigned signer's assertion until the real backend/owner qualification and consuming gate establish its admitted meaning.

## 3. Finite roles, purposes and result statuses

These are new candidate-local semantic identifiers, not production issuer grants or existing DCO1 kind allocations. Native lane and Fixture lane are distinct and always checked. An offline budget issuer can attest only its assigned issuer purpose; it cannot attest native current gate/commit/floor/outcome truth merely by possessing an issuer key. Each real producer/role/lane grant needs its own independently admitted context, even if the same principal is eventually explicitly assigned several roles.

|Role ID/name|Allowed purpose IDs/names|
|---|---|
|1 OwnerCommit|1 CommitResult|
|2 NonregressionFloor|2 Floor|
|3 WriterFence|3 InstallFence|
|4 CompleteCoverage|4 CompleteCoverage, 16 CompleteReconstruction|
|5 Destination|5 DestinationBinding, 7 LocalReservation, 8 WholeVectorApplied|
|6 Closure|6 FullClosure|
|7 ResourceOutcome|8 WholeVectorApplied, 9 ShrinkQuiescence, 10 NoApplication|
|8 StateEffectOutcome|8 WholeVectorApplied, 9 ShrinkQuiescence, 10 NoApplication|
|9 Succession|11 AuthorizedSuccession|
|10 Issuer|12 ImmutableMaximumProfile, 15 GenesisAuthorization|
|11 Lookup|13 CurrentLookup|
|12 BootLifecycle|14 BootGone|

Status IDs: 1 Committed; 2 TerminalNonaccepted; 3 Unresolved; 4 RecoveryFailed; 5 LowerBound; 6 FenceInstalled; 7 Complete; 8 Bound; 9 Quiescent; 10 Authorized. Unknown values reject. Status is always exactly expected; a status alone grants no role-specific semantics. Operation-specific consumers must require their admitted status/role/purpose and complete owner set, not accept every defined status as success. An authentic negative fact still requires initialized complete history, current floor and closed late-acceptance gates under the proof-acceptance contract.

Each role uses a **statement about an already retained DCO1 subject/output record and its immediate predecessor**. Other semantic content (complete vectors, owner sets, outcome/closure coverage, immutable destination binding, input/output floor policy, request schema/effect and retained dependencies) is bound through exact carrier identities plus that record/checkpoint's retained content. These are not arbitrary signed byte arrays and do not supply an omitted external owner-specific proof schema. No carrier hashes a later containing evidence set or its own resulting digest. A later floor/fence/coverage statement about a committed record must be ordered separately so commit/floor/result references remain acyclic.

### Exact adapter coverage versus native role-specific facts

|Coverage|Implemented byte/crypto property|Not supplied by this adapter|
|---|---|---|
|Every listed role/purpose|Fixed canonical statement, exact expected grant/pin/context, real Ed25519 verification, exact retained DCO1 subject and predecessor binding|An actual role grant, native producer, truthful outcome or mutation permission|
|OwnerCommit / CommitResult|Authenticates assigned producer's statement of exact DCO1 result bytes and input/output revisions|Qualified barrier/commit acknowledgement, recovery lookup or physical durability|
|NonregressionFloor / Floor|Authenticates expected namespace/volume/floor-revision/policy statement about that exact committed subject|Current-floor retrieval/advancement, authentic unique chain, anti-rollback or native floor proof schema|
|WriterFence / InstallFence|Authenticates expected owner/fence/cutoff/reference statement|An enforced gate serializer, complete old-work set or native fence acknowledgement schema|
|CompleteCoverage / CompleteCoverage or CompleteReconstruction|Authenticates expected roster/count/checkpoint/reference statement|Authoritative enumeration, complete initialized owner history or native coverage/checkpoint schema|
|Destination, ResourceOutcome, StateEffectOutcome, Closure, BootLifecycle|Authenticates purpose-specific expected destination/effect/incarnation/context and exact subject statement|Native publication, use/quiescence, pin/DMA/lease closure, Gone, all-owner outcomes or their exact retained owner-specific schemas|
|Succession, Issuer, Lookup|Authenticates exactly assigned purpose, policy/roster/revision/scope/subject statement|Actual succession decision, admitted immutable issuer maxima/genesis or initialized current-state lookup protocol|
|DCO1 kind51 external proof Ref entries|No automatic NCF1-to-Ref acceptance or registration|A separately reviewed wrapper/family/schema/role allocation and typed admission adapter; these remain absent|

Thus the complete adapter here is a **signed exact-DCO1-subject statement verifier**, not a universal resource/floor/closure verifier. Defining finite purpose tags prevents purpose substitution; it does not implement or claim the missing native role-specific state machines. Consumers cannot take a valid signature, count or checkpoint ID as those missing facts.

## 4. Error context and finite work

Errors are typed local Rust errors, not a production numeric ABI or assertions of transaction nonacceptance:

- `Limit`: presented carrier exceeds explicit caller ceiling, before signature work.
- `Length`: not exactly1660, contradictory lengths, or internal fixed-layout mismatch.
- `Magic`, `Version`, `Suite`: wrong family/version/cryptographic suite.
- `Reserved`, `Discriminant`, `RolePurpose`: noncanonical padding/reference reserved field, unknown finite enum/result discriminator, or a known but unassigned role/purpose pair.
- `Identity`, `Counter`: required identity absent, invalid applicability group, zero/exhausted counter or noncontiguous local input/output revision.
- `Context`: any signed expected field differs, including validly signed foreign/stale producer, purpose, lane, scope, roster, owner, fence, revision, endpoint or reference.
- `PublicPin`: included key differs from the independently supplied exact pin, or zero pin.
- `Record`: retained subject bytes cannot be independently decoded under explicit limits, or exact schema/length/kind/trailer/header/predecessor binding fails. Structural decode exhaustion is conservatively in this category, not absence.
- `Signature`: provider rejects the signature/public key, wrong message/domain, malformed scalar or forged signature.

The carrier has no counts controlling allocation or variable loops. Exact size is checked before accessing fields; body work is fixed, record work is bounded by explicit caller Limits before hashing, and signature work is one fixed Ed25519 verification. The implementation uses no heap or unsafe code. Context/record checks can reject before cryptographic work. No error claims that the underlying transaction did not commit; all uncertainty continues at its original identity under the owning journal protocol.

## 5. Regression scope and unresolved runtime boundary

`tests/proof.rs` authors real deterministic Ed25519 fixture signatures and checks borrowed typed statements; all signed-byte/signature tampering; validly re-signed changes to every binding; valid wrong role/purpose/public pin; wrong signing domain; noncanonical scalar; unknown format/suite/enums/reserved; missing/oversized carriers; explicit finite work exhaustion; stale expected roster/revocation/owner/fence/counters; fabricated public Record metadata; actual corrupt retained bytes; exhausted counters; and preservation of authenticated Unresolved status. Test signing exists only in the host integration test fixture. No build, test, formatter or native verification execution is claimed by this implementation document; integrated verification belongs to the parent task.

Actual native producer/key-purpose/public-pin grants, current-roster/revocation retrieval, qualified owner-local committed-result/backend/floor protocols, initialized complete owner/dependency history, actual enforced mutation serializers, finite measured profiles, exact package CP1 and later runtime CP2 remain unavailable and mandatory. Native VMware SATA/AHCI CellosFS mount observation is transport/mount evidence, not physical durability, admitted floor or owner completeness. Runtime claim/adoption/resize/release remains closed until the complete exact package decision and actual native enforcement/qualification. Neither test fixtures nor this separately new carrier reassign any missing role or approve the old DCO1 candidate.
