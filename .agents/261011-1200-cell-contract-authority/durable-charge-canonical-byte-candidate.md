# Durable charge canonical record/byte candidate — r1

2026-10-15. **Proposed bytes for separate review; NOT CP1-approved, NOT implemented, NOT deployable.** Governing semantic contract: [ownership/adoption](durable-charge-ownership-adoption-contract.md). Actual assignments are recorded separately in [assignment evidence](durable-charge-assignment-evidence.md). Missing real issuer/verifier/floor/backend inputs block admission; no test signature or arbitrary principal fills them. This candidate allocates no existing public opcode, V1 field, KMS/ASTR field or production schema. All38locks,tenpending phases and23CP pairs remain unchanged.

## 1. Cutover decision and scope

Select a standalone proposed integrity-record family `DCO1`, not reserved bytes in OGJ1, CellosFS V1, ASTR or KMS. This is a review proposal only. The exact file/container/storage API, authoritative authenticator and any OGJ1 embedding require separate decisions. There is no dual parser, compatibility shim, runtime fallback or implicit migration selected. Existing bytes remain unchanged.

Rationale: complete charge/adoption records and manifests exceed OGJ1's experimental256byte canonical body. No evidence proves the four-transaction/two-checkpoint layout can hold this family. This document does **not** truncate obligations, silently enlarge that experiment or propose unreviewed fragmentation. Persistent implementation remains blocked until exact bounded layout, semantic schemas, real assignments, CP1 and preservation/failure evidence exist. Proposed integer widths are codec ceilings, not production/profile admission limits.

## 2. Canonical encoding rules

All integers unsigned little-endian; packed bytes only, no ABI padding. `u16/u32/u64` mean2/4/8bytes. Identity/digest fields are raw byte arrays, never text hex. IDs32 are opaque32bytes; O/A/T and per-operation keys are opaque16bytes. Names of entities are not on-wire authority. Every required ID is nonzero; absent fields are exactly all-zero and permitted only by the per-kind rule. Zero predecessor is allowed only with externally admitted initialization, never implicit genesis. Counters cannot wrap or reset. Optional values require their specified discriminant; no guessed defaults.

Reject unknown versions/kinds/flags, nonzero reserved bytes, wrong exact lengths, overflow, duplicate fields/identities, unsupported units/profile/schema, noncanonical lists, trailing data and missing referenced content. Validate bounded lengths before allocation or hashing; borrow bounded slices where possible. Encoder failure publishes no partial successful record. No numeric public error mapping is assigned.

### 2.1 Header and trailer

| Offset | Field | Bytes | Constraint |
|---|---|---|---|
| 0 | magic | 4 | ASCII `DCO1` |
| 4 | version:u16 | 2 | 1 |
| 6 | kind:u16 | 2 | §5 |
| 8 | total_length:u32 | 4 | `248 + payload_length`, checked |
| 12 | flags:u32 | 4 | 0 |
| 16 | schema_id | 32 | §2.2 |
| 48 | journal_ledger | 32 | Actual owner-local journal identity |
| 80 | journal_genesis | 32 | Admitted initialized identity, not self-authorized |
| 112 | obligation_id:O | 16 | Nonzero for single-obligation records;0for manifest-level adoption records |
| 128 | adoption_id:A | 16 | Nonzero for adoption records;0for reserve/release |
| 144 | local_transaction:T_i | 16 | Immutable per-owner/per-transition identity |
| 160 | local_revision:u64 | 8 | Positive contiguous owner-local revision |
| 168 | journal_writer_epoch:u64 | 8 | Nonzero, authentic gate checks external |
| 176 | previous_record_digest | 32 | Validated previous committed record trailer in this journal |
| 208 | payload_length:u32 | 4 | Exact payload bytes |
| 212 | reserved:u32 | 4 | 0 |
| 216 | payload | N | Kind-specific exact schema |
| 216+N | record_digest | 32 | `SHA256(domain || header[0:216] || payload)` |

Domain is exactly ASCII `cellos/durable-charge/record/v1` followed by one NUL byte. Total length is216+N+32. This digest authenticates **nothing** by itself. The predecessor/reference convention is the trailer, excluding it from its own preimage; this new proposal does not resolve/change existing OGJ1's separately blocked reference wording.

### 2.2 Schema identities and references

For each kind, proposed schema ID is `SHA256(ASCII("cellos/durable-charge/schema/v1/") || ASCII(kind_name))`, no terminal NUL. Kind names are exact lowercase names in §5. Changing any field/order/meaning requires a newly reviewed schema/version, not reuse of this label. These schema IDs cannot be treated as admitted until exact candidate acceptance.

`Ref` is40bytes: `encoded_length:u32` at0, `record_kind:u16` at4, `reserved:u16=0` at6, validated entire record **trailer value**32bytes at8. Referenced bytes must be retained and independently parsed with exact length/kind/digest, journal/genesis, permitted role/schema and complete semantic binding. Digest equality alone is not a proof. Cycles in manifest/reference dependencies reject. Ref may not identify the referencing record itself. No hash-only mutable dependency is allowed. A reference does not eliminate storage/capacity charge for its referent.

## 3. Fixed structures used by payloads

Offsets are relative to the start of each structure. Concatenation has no separators/padding. Every body uses exactly its listed structure sequence, not an unordered map.

| Structure | Field sequence and relative offsets | Size |
|---|---|---|
| `DebitOrigin` | origin_ledger:ID32@0; origin_genesis:ID32@32; initiating_transaction:T16@64; initiating_revision:u64@80 | 88 |
| `Owner` | principal:ID32@0; incarnation:ID32@32; fixed_home:ID32@64; epoch:u64@96 | 104 |
| `Scope` | application:ID32@0; workload:ID32@32; purpose:ID32@64; lane:u32@96; policy_revision:u64@100 | 108 |
| `Keys` | create_key:ID16@0; resize_key:ID16@16; release_key:ID16@32 | 48 |
| `VectorRef` | profile_schema:ID32@0; vector_manifest:Ref@32 | 72 |
| `Watermark` | ledger:ID32@0; genesis:ID32@32; revision:u64@64; record_digest:ID32@72; floor_evidence:Ref@104 | 144 |
| `Binding` | request_schema:ID32@0; request_length:u64@32; exact_request_digest:ID32@40; effect_fingerprint:ID32@72; destination_binding:Ref@104 | 144 |

All Owner epochs and Watermark revisions must be positive. Owner equality compares all fields, not principal alone. Source/target owner relation must match the exact admitted succession. Header writer epoch is the local journal writer; a destination-ready journal writer does not grant transferred-scope authority.

`Keys`: create key required on every obligation; resize key nonzero only for resize-derived reservation/context; release key nonzero only for release. A resize operation has its own O/D for the positive delta where the approved charge schema selects that decomposition, or a distinct pending entry on the original O; **this decomposition is not selected here**. Therefore any resize-bearing DCO1 persistent candidate remains blocked until one canonical decomposition is selected with C05 ownership review; equal-key changed-binding rules cannot be bypassed by this open choice.

## 4. Complete manifest and semantic dependency boundary

Manifest records use explicit canonical entries, not directory enumeration. Record reference schemas below are proposed structural containers. Their external semantic evidence is accepted only under the assigned verifier/purpose roster, not because a digest-valid container exists.

### 4.1 `vector_manifest` (kind32)

Payload: profile_schema:ID32@0, entry_count:u32@32, then entries of24bytes each at36+24i: dimension_id:u16@0, unit_id:u16@2, reserved:u32=0@4, amount:u64@8, dimension_revision:u64@16. Entries strictly ascend dimension_id, no duplicates. Exact length `36+24*count`; count must equal the full admitted profile's dimension set. No unknown dimension, unit conversion, omitted zero dimension, negative value or saturation arithmetic. Zero amount is valid; profile/schema identity is nonzero. Profile dimension/unit assignments and finite count are **not admitted** here. Every sum/delta uses checked arithmetic and the same profile/units.

### 4.2 `ownership_manifest` (kind33)

Payload prefix: Scope108@0; source_owner104@108; target_owner104@212; source_watermark144@316; entry_count:u32@460. Entries at464+280i, exactly280bytes: O16@0; DebitOrigin88@16; reserve_record:Ref@104; base_charged:Ref@144; pending_positive:Ref@184; retained_facts:Ref@224; expected_custody_revision:u64@264; reserved:u64=0@272. The three vector/fact references have distinct roles and cannot be interchanged. Base/pending reference kind32; retained_facts kind34; reserve kind1. Entries strictly ascend O bytes lexicographically, no duplicate O or debit entry; singleton adoption still requires the full manifest and facts. Exact length `464+280*count`. Empty transferred scope rejects. Manifest digest/length and count bind every transition; no changed entry is accepted under the same A.

### 4.3 `retained_facts` (kind34)

Payload: O16@0; fact_count:u32@16; then48byte entries at20+48i: fact_role:u16@0, reserved:u16=0@2, Ref@4, reserved:u32=0@44. Sort by `(fact_role, Ref.trailer)` and reject duplicate tuples. Proposed fact roles:1Unknown transition/outcome,2closed-key/credit protection,3capacity reservation,4resource/pin/lease/DMA acknowledgement,5state/effect fence/watermark,6external reconciliation obligation,7adoption predecessor,8operation key/result history. These are reference roles, not new public proof kinds or permission grants. Each obligation requires an externally approved completeness rule identifying required facts/negative facts and actual owner producers; zero count cannot establish no obligations. Missing fact/producer/schema keeps the entire adoption mutation-closed.

### 4.4 Explicit unresolved semantic evidence

`floor_evidence`, `destination_binding` and fact referents include real issuer/fence/state/resource proofs whose complete accepted schemas, authenticators and per-method required sets are unavailable. DCO1 does not encode opaque evidence as success: Ref to unassigned kind/schema or unapproved issuer rejects. Those exact external carrier bytes and verifier assignments must be supplied at CP1. Until then this is an exact structural candidate with **unresolved semantic carriers**, not a complete executable codec/CP1 package. Internal transaction-map/capacity containers below specify structure, not real deployment facts.

### 4.5 `transition_map` (kind35)

Payload: Scope108@0; A16@108; entry_count:u32@124;20byte entries at128+20i: side:u8@0 (`0source/local`, `1destination`), transition_kind:u8@1, reserved:u16=0@2, transaction_id:ID16@4. Exact length128+20*count; sort `(side,transition_kind)` and reject duplicate keys or transaction IDs. A equals header A. Bootstrap map has exactly source kinds2,4,6,7,11 and destination kinds3,5,8, in canonical sorted order, including mutually exclusive abort/fence-start branches; same-ledger map exactly `(0,9)`. Auxiliary manifest/map/capacity commits use independent transactions outside this operation-transition map, never reused as a transition. Every lookup authenticates journal/side; map identity cannot change after prepare.

### 4.6 `capacity_manifest` (kind36)

Payload: Scope108@0; Owner(control-envelope custodian)104@108; Ref(preexisting charged control reservation)@212; entry_count:u32@252;24byte entries at256+24i: class:u16@0, unit:u16@2, reserved:u32=0@4, amount:u64@8, allocation_revision:u64@16. Exact length256+24*count; strictly ascending unique class. Proposed required classes1journal bytes,2record slots,3outcome slots,4tombstone slots,5decode/recovery bytes,6manifest entries,7validation work,8reconciliation work. Units/class assignments and finite amounts require measured profile/owner approval; unsupported or omitted classes reject, never imply zero. Control reservation must predate this record and cannot refer to a record depending on this manifest. Real storage/metadata/CPU overhead not represented by these classes requires a revised approved schema, not hidden uncharged work.

## 5. Proposed record-kind payload tables

Kind IDs are local to this unapproved DCO1 family. No current wire integer is changed. Each `Ref(role)` requires exact role/kind/ledger/schema checks, not merely shape. The table names dependency blockers explicitly; it does not claim an undefined referent is implemented.

| Kind / exact name | Header O/A rule | Payload structure sequence in exact order |
|---|---|---|
| 1 `reserve` | O nonzero,A zero | Scope; DebitOrigin; Keys; Owner(reservation provenance); Binding; VectorRef(base); VectorRef(pending positive); Ref(protocol capacity); Ref(initial retained facts) |
| 2 `prepare_source` | O zero,A nonzero | Scope; Owner(source); Owner(target); Ref(ownership_manifest); Ref(immutable transition-transaction map); Ref(protocol capacity); Watermark(source) |
| 3 `prepare_destination` | O zero,A nonzero | Scope; Ref(prepare_source); Ref(ownership_manifest); Owner(preparation-journal writer); Ref(prepared capacity); Watermark(destination) |
| 4 `assign_source` | O zero,A nonzero | Scope; Ref(prepare_source); Ref(prepare_destination); Ref(ownership_manifest); Owner(target); Ref(old-owner enforced fence); Ref(fence_started); Watermark(source) |
| 5 `accept_destination` | O zero,A nonzero | Scope; Ref(assign_source); Ref(ownership_manifest); Owner(accepted custodian); Ref(complete reconstruction); Ref(current enforced fences); Watermark(destination) |
| 6 `settle_source` | O zero,A nonzero | Scope; Ref(accept_destination); Ref(assign_source); Ref(ownership_manifest); Watermark(source) |
| 7 `abort_source` | O zero,A nonzero | Scope; Ref(prepare_source); Ref(ownership_manifest); Ref(transition_map); Watermark(source) |
| 8 `abort_destination` | O zero,A nonzero | Scope; Ref(abort_source); Ref(prepare_destination); Ref(ownership_manifest); Watermark(destination) |
| 9 `adopt_same_ledger` | O zero,A nonzero | Scope; Owner(old provenance); Owner(successor); Ref(ownership_manifest); Ref(transition-transaction map); Ref(enforced successor fences); Ref(complete reconstruction); Watermark(local) |
| 10 `release_once` | O nonzero,A zero | Scope; DebitOrigin; Keys; Ref(original reserve); Ref(complete closure manifest); Owner(current authorized custodian); VectorRef(credit); Watermark(local) |
| 11 `fence_started` | O zero,A nonzero | Scope; Ref(prepare_source); Ref(prepare_destination); Ref(ownership_manifest); Ref(transition_map); Owner(target); Watermark(source) |
| 32 `vector_manifest` | O zero,A zero | §4.1 |
| 33 `ownership_manifest` | O zero,A nonzero | §4.2 |
| 34 `retained_facts` | O nonzero,A zero | §4.3 |
| 35 `transition_map` | O zero,A nonzero | §4.5 |
| 36 `capacity_manifest` | O zero,A zero | §4.6 |

Operation transition transactions belong to the immutable A map and correct source/destination journal; auxiliary commits are separately identified as §4.5 specifies. Repeated transaction/key with changed bytes rejects. Source FenceStarted(kind11) and Aborted(kind7) are mutually exclusive under one expected-revision serializer. No fence I/O before observed durable kind11 acknowledgement or authenticated exact committed lookup. Assignment(kind4) requires committed kind11 and completed enforced-fence evidence. Any uncertain kind11 remains non-abortable/mutation-closed until same-A authoritative resolution. Prepare alone never activates.

Self-reference prohibition: Watermark in transition payloads identifies the already committed input/base revision, never the record currently being encoded. Output committed revision/digest come from the verified header/trailer and real commit proof, not self-inclusion. Header `previous_record_digest` is the actual prior owner-local committed record. Any completeness/closure proof that requires the current record must be split into a later result record under a separately reviewed schema, never a cyclic hash.

Committed kind7 is itself the terminal nonassignment decision serialized against kind11, not a reference to its own proof. Kind11 commits non-abortable intent before fence I/O; kind4 follows only that branch. Destination abort requires authentic committed kind7 and stores kind8 tombstone before prepared-capacity reclaim.

No release is authorized by this byte shape. ReleaseOnce remains solely at the fixed-home debit ledger and requires exact original D/O/create key, nonreused release transaction, expected revision and all-owner closure/quiescence. Expected revision is the Watermark input revision; successor header revision must match its qualified journal's checked commit ordering. Output-copy/reply loss queries the original transition, never another credit.

### 5.1 Mandatory cross-record equality checks

Every top-level transition Watermark is its containing journal's immediate input predecessor: ledger equals header journal_ledger, genesis equals header journal_genesis, digest equals header previous_record_digest, and header local_revision equals watermark revision+1 with checked arithmetic. Nested source_watermark in an ownership manifest instead binds the exact authenticated source preparation snapshot; it is not the auxiliary manifest journal's predecessor. Reject wrong-role substitution. Initialization revision0 is not introduced by this candidate; an admitted positive-revision initialized predecessor is required.

Each ownership entry O equals referenced reserve header O and both header/payload O of retained_facts. DebitOrigin equals the reserve's exact DebitOrigin. Entry base/pending refs equal the corresponding reserve VectorRef references, and each outer VectorRef profile_schema equals its referenced vector_manifest profile_schema. Base/pending/credit profiles, dimensions and units must equal the admitted profile for that obligation. No cross-O or cross-profile fact/vector borrowing is allowed. These immutable-reserve equalities limit this r1 candidate to unresized reservation snapshots: post-resize custody requires an explicit authenticated current-allocation snapshot schema and is BLOCKED until that schema/decomposition is reviewed; never claim an old reserve reflects updated charge.

## 6. Authentication, storage and activation blockers

This record family is an integrity-bearing journal input. Authoritative acceptance additionally needs an authenticated committed-result carrier binding protocol/schema/purpose/lane, issuer/key/public-pin/revocation revision, audience, exact DCO1 length/trailer, owner-local revision/floor and endpoint/incarnation/request. No suite/key ID/signature length is guessed. Authentication suite and exact signed bytes cannot be finalized without actual admitted producers/verifiers. Unauthenticated data may be structurally inspected but cannot mint internal usable permission.

A file read or valid trailer is not durable commit acknowledgement. Actual D1/D2, old-root preservation, complete manifests, nonregression, writer fencing, atomic expected-revision comparison, replay-safe retention and capacity refusal remain storage/owner gate requirements. Native normal VFS and private host CoW experiment are not admitted DCO1 backends. No production limits are selected from u32 lengths. Refuse before allocation when a required approved count/byte/work limit is absent.

## 7. Review and verification contract

Structural verification may compute packed offsets/sizes, schema IDs, lengths and integrity-only synthetic vectors; it cannot prove authority or adoption. Required future negative witnesses include: modified kind/schema/purpose/ledger/D/O/A/T; wrong source/target/epoch; same-ID changed bytes; misordered/duplicate/incomplete manifests; missing referents; cycle/self-reference; integer exhaustion; wrong exact counts/units; forged/recomputed integrity; assignment/abort race; stale gate fence; D1/D2/reply loss and one-credit closure. All durable/authority witnesses remain pending.

Real assignments and unresolved external proof/completeness/closure schemas block exact CP1 completion. Review may accept portions of this proposal only with explicit exact revision/digest and named owners; this document records no such consent. Rejection changes docs only. After any future durable operation, retain original keys/debit/epochs and use authorized forward recovery; binary/schema rollback cannot undo effects or make old custody live.
