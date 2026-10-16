# Durable charge allocation bytes — successor candidate s1

2026-10-15. **Exact structural/semantic proposal, NOT ADOPTED, NOT CP1-approved, NOT implemented and NOT authority evidence.** This document selects the missing same-O pending-entry decomposition, one outstanding resize, current-allocation and partial-credit carriers. It does not revise historical [DCO1 r1](durable-charge-canonical-byte-candidate.md), [C03](exact-deltas/C03.md) or [C05 r2](exact-deltas/C05.md). Its separate proposed schema names/kinds require governed acceptance. Actual principals, authentic external proof schemas, public pins, backend, floor and finite profile remain unavailable as recorded in [assignment evidence](durable-charge-assignment-evidence.md). No lock, checkpoint, consent, runtime permission or implementation follows.

## 1. Selected model and deliberate C05 refinement

One immutable obligation `O`, debit origin `D`, original kind1 reserve and create key survive all ordinary resizes. A resize is a pending entry on that O, **not another O or another debit origin**. Distinct nonreused resize, adjustment and final-release keys identify operations; each local transition has its own immutable transaction T. There is at most one outstanding resize, including its pending shrink credits or cancellation. New distinct resize keys refuse while it is outstanding. Exact repeats reconcile, never allocate another delta.

For every dimension, use the same admitted profile, complete dimension set, unit and dimension revision. `B` is conservative unreleased base charge, `P` pending global positive debit, `E` confirmed effective destination allocation and `Q` immutable whole-vector target. `C=B+P` is the one global charge. All subtraction is guarded, all addition/increments/length calculations checked; no saturation, wrapping or cross-dimension netting. The immutable maximum vector `M` is separately issuer-authorized; every E/Q is componentwise at most M. B can exceed E while safe credit is pending.

At reserve, compute **global incremental debit** `G=max(Q-B,0)` and **destination local growth** `L=max(Q-E,0)` independently, then set P=G. A local grant for L is required even if G=0. Existing conservative B is not destination admission authority. For the conservative arithmetic state B=(7,8), E=(7,3), Q=(6,5), G=(0,0) but L=(0,2). In s1 that B/E state still has an outstanding shrink and therefore a new resize refuses until settlement; this example explains the independent guards, not permission to bypass serialization. s1's initialized/settled Stable states have B=E; it admits no historical resized checkpoint that silently introduces a different Stable state.

**Explicit upstream conflict:** C05:20 and :28 currently say Authority and destination reserve `delta_plus=max(Q-A,0)` with A the effective admitted allocation. Replacing A by B silently would understate local growth. This successor proposes a governed refinement: Authority durably accounts the full prospective charge `max(B,Q)=B+G`, explicitly rebinds already-charged coverage `L-G` (componentwise nonnegative because B>=E), and debits only G; destination independently reserves all L. C05's no-use-before-reservation, immutable M, no negative netting, whole-vector safe application and Unknown rules are preserved. Its literal equal new-debit formula is **not** claimed satisfied when B>E. C05 owners must approve this two-plane refinement and successor package; otherwise this candidate cannot be admitted. No base package is changed here.

## 2. Reused envelope and exact naming

Reuse DCO1 r1 §§2–3 unchanged: 216-byte header, 32-byte trailer, `total_length=248+payload_length`, version1, flags0, original record digest domain, original schema-ID derivation, little-endian packed integers, Ref40, DebitOrigin88, Owner104, Scope108, Keys48, VectorRef72, Watermark144 and Binding144. Ref is an exact retained record length/kind/trailer binding, not authority. All existing reserved-zero, bounded-parse, nonzero-identity, no-trailing-data, role/profile, cycle and unknown-schema rejection rules remain. Payload offsets below start at zero, hence add216 for absolute record offsets. All sizes are bytes. No alignment padding exists.

For each new exact name below, schema ID is `SHA256(ASCII("cellos/durable-charge/schema/v1/") || ASCII(name))`, without NUL, exactly as r1. New kind allocation is a **proposal within DCO1 only**, not an existing public opcode or production registration:

| Kind | Exact schema name | Payload length | Header O / A |
|---|---|---|---|
|48|`allocation_snapshot_v2`|1060|nonzero / zero|
|49|`resize_request_v2`|1212|nonzero / zero|
|50|`allocation_adjustment_request_v2`|764|nonzero / zero|
|51|`allocation_evidence_set_v2`|168+48n|subject O or zero / zero|
|52|`allocation_key_registry_v2`|76+64n|nonzero / zero|
|53|`ownership_manifest_v2`|464+440n|zero / nonzero|
|54|`adoption_transition_map_v2`|136+20n+32m+76r|zero / nonzero|
|55|`adoption_transition_v2`|644+40n|zero / nonzero|
|56|`allocation_lookup_result_v2`|720|nonzero / zero|
|57|`allocation_release_request_v2`|516|nonzero / zero|
|58|`allocation_release_result_v2`|1060|nonzero / zero|

Numbers12–31,37–47 and59+ are not allocated by this proposal. Kind1 reserve,32 vector_manifest,34 retained_facts and36 capacity_manifest retain their original schemas and meanings. Kind33 and adoption transitions2–9/11 remain r1-only; they cannot accept kind53 instead. Kind10 release_once remains a full unresized-r1 closure record; it is **never** a partial shrink/cancellation record. A successor O uses kinds48–58 consistently for current state, adoption and final release; no dual interpretation of r1 bytes.

For new mutation kinds48/55/58 and committed lookup56, an input Watermark is the immediate already-committed input to its local journal, matching header ledger/genesis/previous digest with header revision=input revision+1. Auxiliary requests/manifests/registries/evidence use ordinary journal predecessor chaining, independent auxiliary T and no invented embedded input watermark. Their header revisions are not allocation revisions. Every stored record still needs authentic committed-result acceptance; header/trailer validity is insufficient. No self-reference or dependency cycle is permitted.

## 3. Exact allocation snapshot and final-result payload

Kinds48 and58 share this **new** payload layout, with distinct schema names and state constraints:

|Offset|Field|Bytes|
|---|---|---|
|0|Scope|108|
|108|DebitOrigin|88|
|196|create_key|16|
|212|original_reserve:Ref(kind1)|40|
|252|current_custodian:Owner|104|
|356|custody_decision:Ref|40|
|396|previous_allocation:Ref(kind48)|40|
|436|allocation_revision:u64|8|
|444|custody_revision:u64|8|
|452|transition:u16|2|
|454|state:u16|2|
|456|reserved:u32=0|4|
|460|VectorRef(B)|72|
|532|VectorRef(P)|72|
|604|VectorRef(E)|72|
|676|active_resize:Ref(kind49), or zero|40|
|716|active_adjustment:Ref(kind50/57), or zero|40|
|756|step_operation:Ref(kind1/49/50/55/57)|40|
|796|step_evidence:Ref(kind51)|40|
|836|key_registry:Ref(kind52)|40|
|876|retained_control_capacity:Ref(kind36)|40|
|916|input_watermark:Watermark|144|

States:1 Stable,2 Reserved,3 ApplicationUnknown,4 AppliedCreditPending,5 Closing,6 Closed. Kind48 permits1–5 only; kind58 permits6 only. Transition tags:1 Initialize,2 ReserveResize,3 LocalReady,4 ApplicationUnknown,5 ApplyWholeVector,6 BeginAdjustment,7 FinishAdjustment,8 AdoptCustody,9 BeginFinalRelease,10 FinishFinalRelease. Kind58 is tag10 only; kind48 permits1–9. Undefined tags reject.

Every snapshot authenticates exact current custodian and an admitted positive custody revision. Initial custody_decision is kind51 with initial-authority role; subsequent custody changes reference committed kind55 phase5 or9. All ordinary resize/credit/release steps preserve custody fields. AdoptCustody changes only custody decision/owner/revision plus necessary successor control-capacity/evidence and journal/predecessor fields, never B/P/E, operation bindings or registry. Its custody revision increments by one. Allocation revision increases by one on every successor snapshot, including LocalReady, Unknown, intent and custody steps; it never equals journal revision by convention. Only Initialize has zero previous_allocation and allocation revision1, and only with explicitly admitted initialization evidence. All other snapshots reference the exact selected prior kind48; no successor follows Closed kind58. No arbitrary checkpoint or chain truncation is admitted by s1.

Each O has one authoritative current-state index in its fixed-home ledger. A snapshot commit atomically compares its expected current snapshot, expected journal revision, writer/custody authority and key status; publishes the new current index, charge change and operation result together. Partial publication is not success. This is a required backend contract, not an assertion that a backend exists. Later journal commits, object IDs, largest revisions or a caller-provided snapshot cannot select this index. Recovery needs the authenticated committed journal, complete chain and admitted nonregression floor.

The authoritative journal serializer captures the immediate input watermark only when a transition's first immutable bytes are ready to publish, and prevents any later commit in that journal from overtaking an uncertain publication until its exact T has an authoritative committed/noncommitted decision. In particular a definitive expected-input conflict is detected before immutable transition bytes are issued at that gate; stale operation requests refuse before issue. Authoritative same-T commit-status queries on an uncertain publication are read-only, externally authenticated queries, not new kind56 journal commits. If a publication is proven uncommitted and the journal/input remains unchanged, it may retry identical T/bytes. This deliberately serialized candidate trades availability for an unambiguous fixed-T contract; it does not silently regenerate a new watermark/body for an old T. Fence/forward recovery uses the separately bound continuation rules.

## 4. Exact operation requests

### 4.1 Kind49 resize request

|Offset|Field|Bytes|
|---|---|---|
|0|Scope|108|
|108|DebitOrigin|88|
|196|create_key|16|
|212|resize_key|16|
|228|original_reserve:Ref(kind1)|40|
|268|input_snapshot:Ref(kind48)|40|
|308|requesting_custodian:Owner|104|
|412|Binding(exact resize request, incarnation and destination)|144|
|556|VectorRef(input B)|72|
|628|VectorRef(input P)|72|
|700|VectorRef(input E)|72|
|772|VectorRef(Q)|72|
|844|VectorRef(G)|72|
|916|VectorRef(L)|72|
|988|VectorRef(immutable M)|72|
|1060|admission_evidence:Ref(kind51)|40|
|1100|control_capacity:Ref(kind36)|40|
|1140|expected_allocation_revision:u64|8|
|1148|T_reserve|16|
|1164|T_unknown|16|
|1180|T_apply|16|
|1196|T_local_ready|16|

Input must be the current Stable snapshot, P=0, no active operation; every input vector and revision must equal it. Admission evidence subject is that input snapshot, proving assigned issuer/max/profile, current fences and exact destination binding as required by real owner schemas. All T fields and header auxiliary T are nonzero and pairwise distinct. They are fixed forever for this request. Resize key is not the create key, any past resize/adjustment/release key or transaction namespace alias. Request bytes alone reserve nothing.

### 4.2 Kind50 adjustment request

|Offset|Field|Bytes|
|---|---|---|
|0|Scope|108|
|108|DebitOrigin|88|
|196|create_key|16|
|212|adjustment_key|16|
|228|mode:u16 (1 ShrinkCredit,2 NoApplicationCancel)|2|
|230|reserved:u16=0|2|
|232|original_reserve:Ref(kind1)|40|
|272|resize_request:Ref(kind49)|40|
|312|input_snapshot:Ref(kind48)|40|
|352|VectorRef(credit S)|72|
|424|VectorRef(result B)|72|
|496|VectorRef(result P)|72|
|568|VectorRef(result E)|72|
|640|qualification:Ref(kind51)|40|
|680|control_capacity:Ref(kind36)|40|
|720|expected_allocation_revision:u64|8|
|728|T_begin|16|
|744|T_finish|16|
|760|reserved:u32=0|4|

For mode1, input is AppliedCreditPending with no adjustment: P=0, E=Q, `0<=S<=B-Q`, at least one positive component, result B=B-S, result P=0, result E=E. Qualification subject is the resize request and authenticates enforcement/quiescence for exactly S's retired allocation coverage, all affected owners, exact incarnation, current fence/watermarks and no incompatible retained work. Different installment keys may credit disjoint remaining amounts only after prior results are known. They do not reuse a successful adjustment key or a release key.

For mode2, input is Reserved or ApplicationUnknown, with no adjustment. Qualification subject is the resize request and proves **no component ever applied** and authoritative nonacceptance or durable closure/quiescence of every possibly admitted destination reservation/effect. S must equal all current P; result B=input B, result P=0, result E=input E. S=0 is permitted here, because retiring a no-op/growth-covered resize still needs an idempotent terminal decision. No subset cancellation and no timeout-derived negative fact. Any evidence of partial/whole possible application disqualifies this mode.

T_begin, T_finish and the auxiliary request T are distinct. A new request cannot substitute for an uncertain begin or finish. Required Owner is obtained from input snapshot and current authenticated custody, not invented from an adjustment key. Credit authority is only the fixed-home ledger, never the issuer, controller or destination proof producer.

### 4.3 Kind57 final release request

Payload: Scope108@0; DebitOrigin88@108; create_key16@196; release_key16@212; original_reserve:Ref@228; input_snapshot:Ref(kind48)@268; VectorRef(full credit B)@308; closure:Ref(kind51)@380; retained_control_capacity:Ref(kind36)@420; expected_allocation_revision:u64@460; T_begin16@468; T_finish16@484; reserved16(all zero)@500. Exact length516.

Input is Stable, P=0, no outstanding resize/adjustment and the exact current O. Closure subject is input snapshot; all-owner effect/resource/state quiescence, authentic Gone where required, restart-safe creation/publication key closure, no retained grants/DMA/pins and current floor/fence must qualify the whole O. A resize Unknown must first reconcile through its own original key into a resolved Stable state; final release cannot conceal it. The two release T and auxiliary T are distinct and nonreused. Full credit equals remaining B, not original reserve amount and not the sum of historical charges. Retained key/credit-protection capacity is separately charged in the preexisting control envelope and is **not** refunded by releasing this O.

## 5. Defined internal containers, not opaque evidence

### 5.1 Kind51 evidence set

Scope108@0; subject_O16@108; subject:Ref@124; entry_count:u32@164; entries at168+48i: role:u16@0, reserved:u16=0@2, proof:Ref@4, reserved:u32=0@44. Exact length168+48n. Header O equals subject_O (zero for adoption-level evidence); header A=0. Subject must be a prior kind1,48,49,53,55 or58 appropriate below, with matching scope/O. Entries sort `(role, Ref.trailer)`; duplicates reject. Required roles for each use must be complete; empty sets never qualify an operation. Every kind56 completeness_evidence set must have subject equal to its exact current_snapshot (kind48 or58) and contain the complete role12 lookup/floor proof set, including when current state is Closed.

Roles:1 initial-custody/genesis authorization;2 issuer immutable-max/profile authorization;3 current writer/destination fences;4 destination local L reservation and exact same-key claim;5 whole-vector applied Q result and atomic use/obligation-fit acknowledgement;6 shrink-credit coverage/enforcement/quiescence;7 resize no-application/nonacceptance/closed-key quiescence;8 full-O closure/Gone/publication-key fence/all-owner quiescence;9 complete reconstruction/dependency set;10 adoption authorized succession;11 enforced old-writer fence and admitted-work watermark;12 initialized complete lookup/floor. A role may require several independently produced proofs; one entry does not mean one owner suffices.

These are complete **container** bytes, not fabricated proof bytes. Each proof Ref must name an externally approved exact record kind/schema, issuer, authentic suite/pin/purpose/audience and scope/method-specific complete required set. Such external allocations and signatures are unavailable. Unassigned proof kinds or incomplete/unapproved semantic sets reject; no untyped blob, arbitrary digest, success boolean or this container itself can substitute. Evidence-set digest/parse success grants no authority. This leaves explicit external qualification, not an undefined internal structure.

No cycle: request49 admission evidence has subject=input48, not request49; ready/application/cancel/credit evidence has subject=already-committed request49; release closure has subject=input48; initial evidence has subject=reserve1. Adoption's authorized-succession set has subject=manifest53; later fencing/reconstruction sets may have subject=already-committed phase1/3/4 record55. Referenced proof bytes cannot refer back to the evidence set or a future snapshot that includes it.

### 5.2 Kind52 complete issued-key registry

O16@0; original_reserve:Ref(kind1)@16; create_key16@56; entry_count:u32@72; entries at76+64i: operation_class:u16@0 (1 resize,2 adjustment,3 final release), reserved:u16=0@2, operation_key16@4, request:Ref@20, reserved:u32=0@60. Exact length76+64n. Sort strictly by raw key bytes; duplicate key rejects even across classes. Classes reference49/50/57 respectively. Every request must bind the exact O/D/create/reserve; adjustment must bind an issued resize. The create key is implicit and never a list entry or reusable operation key.

Registry is the full monotonically increasing issued-key set, not only active requests. Initialize uses count0. ReserveResize, BeginAdjustment and BeginFinalRelease append exactly their new request/key; every other transition preserves it byte-for-byte. An auxiliary orphan registry/request is not an issued operation until selected by a committed allocation snapshot. Retired/terminal keys stay present forever in s1. Their outcome is determined by the authenticated snapshot transition chain, including kind58, not a self-referencing result field in this registry. Current snapshots include the current issued request; terminal results are the snapshots themselves. This intentionally avoids registry→current-result→registry cycles.

No compaction, omission, TTL, key reuse or hash-only checkpoint is selected. Chain, terminal results, registry, original reserve, debit provenance, manifests, floor/fence/evidence dependencies and capacity references remain retained. Saturation refuses future work before dispatch. A later bounded retirement/checkpoint scheme would require another reviewed schema and actual replay-safe namespace fence, not a local deletion policy.

### 5.3 Reused manifests

Vector kind32 is unchanged: full profile, all dimensions including zero, original units/revisions, exact36+24n bytes. Every VectorRef includes matching outer profile schema. Zero vectors use actual canonical kind32 records, never absent references. Retained_facts kind34 and capacity_manifest kind36 are unchanged. Required facts include all source Unknowns, key registries/result history, evidence/fences, pending local reservations and outcomes, external reconciliation obligations and control envelopes. Their approved completeness/producer rules remain external requirements; directory enumeration is insufficient.

Control capacity must predate each operation, cover both success and unresolved branches, all proposed auxiliary records, referenced proof storage, retained keys/results, journal/work/recovery bounds and any adoption overhead. A reference does not make its referent free. A kind36 record cannot fund itself; it refers to a preexisting independently charged control reservation as r1 requires. Metadata capacity is not silently added to or credited from the resource vector B/P. Exact profile limits and charged ownership must be approved and measured; u32 lengths/counts are codec ceilings only.

## 6. Complete lifecycle and conservation

All rows require current authenticated custody/writer gates, exact same-home ledger/genesis/O/D/create identity, retained capacity and atomic expected-revision/key checks. No dispatch is allowed merely because a request/auxiliary manifest was written.

|Transition|Input and durable result|Evidence / identity / credit|
|---|---|---|
|Initialize (tag1)|An **unresized** kind1 reserve with resize/release Keys zero and pending vector0. B equals its base vector; E=B; P=0; Stable; registry empty; both active refs zero. step_operation=reserve.|Requires authentic confirmed original whole-vector allocation and initial custody/genesis/profile/floor evidence. The original reserve owner is immutable provenance, not authority by itself. An unresolved original create stays under its reserve/Unknown contract and cannot be upgraded into a resizeable Stable snapshot. No genesis/reset inferred from absence. No debit or credit is repeated.|
|ReserveResize (tag2)|Stable → Reserved; B/E unchanged; P=G; active_resize=request49; active_adjustment=0; append issued resize key. step_operation=request49.|T=T_reserve. Atomically reserve the new global debit G and complete prospective bound before any possible resize dispatch. Lost commit acknowledgement means same-T lookup, no dispatch and no replacement key.|
|LocalReady (tag3)|Ordinary Reserved → Reserved; only an accepted map-bound successor refresh may also use ApplicationUnknown → ApplicationUnknown. Preserve B/P/E, state, active references, issued keys and all prior evidence/possible work; step_operation=request49.|Ordinary T=T_local_ready; successor refresh T is the unique kind54 continuation tag3 T. Durably retain role4 current local reservation/claim for **all L**, binding exact request, incarnation, global reserve result and current destination gate, plus role3 fences. Mandatory before potentially applying dispatch, including L=0. Refresh does not erase old claims or authorize duplicate dispatch. Local refusal/uncertainty is retained and reconciled; it never implies cancellation credit.|
|Possible dispatch / Unknown (tag4)|Reserved → ApplicationUnknown; B/P/E and active request unchanged.|T=T_unknown, at most once per request. Evidence retains every possible accepted claim/effect and known fences. Dispatch requires committed LocalReady (or authentic same-T lookup) and admission atomic with fence enforcement. A repeated dispatch is not justified by Unknown; authoritative same-key destination lookup/reconciliation is required.|
|Below-use pending|Reserved or Unknown remains conservatively charged; no positive/negative component exposed.|C05's destination safe-fit test covers all dimensions and all retained grants/use. No kill-to-fit, revocation or partial component installation. No ledger transition is required merely to poll; durable owner outcomes/dependencies remain retained.|
|ApplyWholeVector (tag5)|From Reserved or Unknown after LocalReady: `B'=B+P`, P'=0, E'=Q. If B'=Q → Stable and clear active_resize; otherwise AppliedCreditPending and retain it. active_adjustment=0; step_operation=request49.|T=T_apply. Role5 authentic whole-vector result binds exact key/incarnation/old and new allocation revisions, all-dimension fit and atomic install. C'=C, no credit. Evidence proves application, never merely accepted intent.|
|BeginAdjustment (tag6)|Mode1 from AppliedCreditPending; mode2 from Reserved/Unknown. Keep B/P/E and active_resize unchanged; set active_adjustment=request50; append key; step_operation=request50.|T=T_begin. Qualification and exact result vectors fixed durably before requesting/committing credit/cancellation. It blocks apply or conflicting operations. Mode2 qualification includes closure of delayed resize messages and local claims; this is not full creation-key closure.|
|FinishAdjustment (tag7)|Apply the exact result vectors in50 atomically, clear active_adjustment. Mode1 remains AppliedCreditPending if B'>Q, otherwise Stable and clears resize. Mode2 → Stable and clears resize. step_operation=request50.|T=T_finish. Fixed-home atomic credit S and result/index publication. Mode1 preserves E=Q, P=0; mode2 preserves original B/E and returns only reserved P. Unknown credit acknowledgement reconciles the same adjustment/T, not another installment.|
|AdoptCustody (tag8)|After authenticated successor acceptance, preserve every vector/key/outcome, change custody decision/Owner and increment custody revision; previous state and active operations preserved.|step_operation=committed kind55 phase5/9. T is the exact immutable per-O adoption-materialization T in that adoption's kind54 map. Admission stays closed until all selected O materializations and gate checks are complete. No debit/credit.|
|BeginFinalRelease (tag9)|Stable → Closing; B/E unchanged, P=0, active_resize=0; active_adjustment=request57; append release key.|T=T_begin. step_operation=request57; retain exact closure evidence and key fence. No fresh resize/adoption mutation can race past Closing; recovery may transfer reconciliation responsibility without reopening admission.|
|FinishFinalRelease (kind58/tag10)|Closing → Closed; B=P=E=0, active refs0, exact previous snapshot and release request retained.|T=T_finish. Atomically record Closed, credit remaining B exactly once and select terminal kind58. Creation/resize/adjustment/release keys and all outcome/fence dependencies remain retained; this is the only full-O successor release.|

A snapshot's step_evidence is always an appropriate kind51 set: initialization role1/2/5/12, reserve role2/3, ready role3/4, Unknown role3 plus all known role4/9 dependencies, apply role5, adjustment role6 or7, custody role9/10/11, full release role8. Begin/finish retain the exact qualification set bound by their immutable request. Partial/contradictory owner outcomes cannot be reclassified as no application. A reported partial vector installation violates the proposed destination contract: keep the conservative charge and mutation closed under the same key for authorized forward reconciliation; do not fabricate E=Q or credit.

Successor continuation evidence retains every original qualification proof/result binding and adds current admitted successor fence/floor proofs in a new kind51 set where needed; it does not alter S/Q, erase the original set or assume an old epoch receipt grants new authority. A LocalReady refresh under the continuation map is permitted in Reserved or ApplicationUnknown, retaining the state and all possible prior work; it replaces no original claim/result and is required before any newly authorized destination dispatch. Unknown continuation cannot turn Unknown back into certain nonapplication. An original-epoch ledger commit not yet resolved blocks custody materialization; late destination-only results after the frozen boundary are reconciled by the materialized successor under the original logical key.

**Algebraic obligations, per dimension:** reserve increases C by G; ready/Unknown/adoption change C by0; application changes `(B,P)` to `(B+P,0)` and changes C by0; a qualified shrink installment changes C by `-S` with `0<=S<=B-Q`; cancellation changes C by `-P` and leaves B/E unchanged; final closure credits only remaining B. A sequence's total debits minus all qualified credits equals final C relative to its input. A repeated T/key contributes zero new debit/credit and returns the original result. Never credit both the original P cancellation and its folded B; these branches are mutually exclusive under the current-state serializer.

Examples:5→7 reserves2, applies at charge7, terminates without shrink credit. (5,8)→(7,3) reserves(2,0), applies with B=(7,8), E=(7,3), then credit(0,2) leaves B=(7,6), still outstanding; credit(0,3) leaves B=E=(7,3), Stable. Cancellation before any application returns only(2,0), preserving B=E=(5,8). These are arithmetic illustrations, not execution/authority evidence. A later resize uses a fresh key against the resulting Stable snapshot; old requests always resolve to their old terminal snapshots and cannot become that new resize.

A confirmed destination application whose ledger ApplyWholeVector commit is uncertain retains the old charged B+P locally until exact same-T lookup selects the result. A committed credit whose reply is lost is already reflected in the ledger's authentic current index, but callers cannot spend from a guessed result; admission stays closed until qualified same-key lookup. Do not issue an Unknown snapshot over a possibly committed competing apply/credit; resolve that exact journal transaction first. If it is authoritatively not committed, retry identical bytes/T only against its still-valid input. If custody/input changed, only the explicitly mapped forward-continuation rule in §7.2 may proceed; otherwise remain closed. No changed watermark/body under the same T. Table T assignments name original-request transitions; after adoption, an eligible continuation uses its accepted map's distinct T instead.

## 7. Ownership-manifest and adoption successors

### 7.1 Kind53 ownership_manifest_v2

Prefix exactly: Scope108@0; source_owner104@108; target_owner104@212; source_watermark144@316; entry_count:u32@460. Entries at464+440i:

|Entry offset|Field|Bytes|
|---|---|---|
|0|O|16|
|16|DebitOrigin|88|
|104|original_reserve:Ref(kind1)|40|
|144|current_allocation:Ref(kind48/58)|40|
|184|base_vector:Ref(kind32)|40|
|224|pending_vector:Ref(kind32)|40|
|264|effective_vector:Ref(kind32)|40|
|304|key_registry:Ref(kind52)|40|
|344|retained_facts:Ref(kind34)|40|
|384|control_capacity:Ref(kind36)|40|
|424|expected_allocation_revision:u64|8|
|432|expected_custody_revision:u64|8|

Entries strictly ascend raw O, no duplicate O/debit entry; nonempty transferred scope. Header A binds the complete manifest. Every O/D/create/reserve equals the immutable reserve and selected snapshot. Vector refs equal the selected snapshot's VectorRef manifest refs **not original reserve vectors**; profiles/units match. Registry/capacity/revisions equal snapshot fields. Retained_facts must identify all dependency roots and all relevant source states, including closed-key protection; omitted closed O still required by the approved complete-source rule cannot disappear merely because its B is0. Closed kind58 entries have zero vectors but nonempty retained protection. Source watermark is an authenticated complete selected-source snapshot/floor, not the helper manifest's own predecessor and not a max observed revision.

The source serializer proves no selected O/manifest-relevant dependency changed between source watermark and preparation. Helpers may commit without changing transferred state. Any manifest-changing outcome before FenceStarted invalidates that preparation: source must commit the abort branch and use a fresh reviewed A/map/manifest before another prepare. After FenceStarted the frozen manifest cannot mutate; late outcomes are retained under original keys for authorized successor reconciliation, not appended into the frozen bytes. No materialization snapshot feeds backward into its own adoption manifest.

Allocation-ledger freeze is stricter than an external effect-gate cutoff. Before source preparation selects the manifest, every previously admitted kind48/kind58 transaction affecting selected O must have an authoritative committed or permanently noncommittable outcome; an uncertain transaction blocks preparation. Preparation holds the selected allocation-index freeze. Under that same source allocation serializer, FenceStarted atomically verifies the unchanged selected indices, proves no unresolved admitted allocation transaction remains, and durably closes all old-custodian kind48/kind58 commits for the transferred scope before any fence I/O. No already-admitted allocation transaction may commit afterward. Abort before FenceStarted may release the preparation freeze under the still-valid original epoch; after FenceStarted only mapped successor materialization/continuation is permitted after acceptance. Source adoption-protocol commits remain allowed, but do not change selected allocation snapshots. Late destination/resource/state/effect outcomes may be retained as evidence only, never published as an old-custodian allocation/credit/final-release snapshot. Consequently a non-Closed frozen O cannot become kind58 before its mapped AdoptCustody; a Closed entry was already terminal when the manifest was selected.

### 7.2 Kind54 adoption transition map

Scope108@0; A16@108; transition_count:u32@124;20-byte transition entries at128+20i: side:u8@0 (0 source/local,1 destination), phase:u8@1, reserved:u16=0@2, T16@4. Sort `(side,phase)`; duplicate keys/T reject. Bootstrap transition list has exactly `(0,1),(0,3),(0,4),(0,6),(0,7),(1,2),(1,5),(1,8)`; same-ledger list exactly `(0,9)`. At128+20n encode materialization_count:u32; at132+20n encode32-byte materialization entries `O16@0,T16@16`, strictly ascending O. Include exactly every non-Closed O in manifest53, no other O; Closed protection needs no new snapshot.

At132+20n+32m encode continuation_count:u32; at136+20n+32m encode76-byte continuation entries `O16@0,request:Ref@16,transition:u16@56,reserved:u16=0@58,T16@60`. Sort `(O,request.trailer,transition)` with no duplicate tuple. Request must be an active request in that O's frozen snapshot/registry. Allowed continuation tags are3/4/5 for request49,7 for request50 and10 for request57; include exactly all still-possible steps for each outstanding operation, including mutually exclusive outcomes. For request49 in Reserved/ApplicationUnknown with no adjustment, include tag3 for successor-local readiness refresh even if old LocalReady committed, tag4 only if Unknown is not already committed, and tag5 while application is unresolved. AppliedCreditPending needs no request49 continuation; a subsequently issued new adjustment has its own request/T. Request49 continuations impossible under a begun cancellation are omitted. Header A equals payload A. Exact length is136+20n+32m+76r. All T values in all lists are nonzero, pairwise distinct and distinct from helper and original operation T. These canonical bytes, not an opaque reconstruction referent, bind per-O materialization and forward-continuation transactions. No r1 map is reinterpreted.

A continuation T is a **new forward transition under accepted custody**, never a retry with changed bytes under an old T. Its logical O/D/create/resize/adjustment/release key and exact result vectors remain original. Before using it, authoritative lookup must establish whether each original or previous-custody competing T committed. A committed original result must be incorporated exactly once; a possibly committed original result blocks continuation. An original T that is proven uncommitted must additionally be permanently unadmittable under the enforced old epoch and closed expected input. Only then may the unique map-bound successor T finish that same operation against its current materialized predecessor and writer epoch. Old T replies retain their original status; logical-key lookup finds the one serialized result, regardless of whether original or successor T won. A later custody transfer uses another explicitly bound map and fences its predecessor's T in the same way; no arbitrary fresh T is allowed.

The tag3 readiness refresh is the one explicit nonterminal refresh case: an already committed old LocalReady is retained, but its old-epoch grant is not reused as current successor authority. The refresh's distinct map T binds fresh current-gate evidence without adding global charge or replaying destination application. All terminal apply/credit/release results remain strictly one-per-logical-operation; none may use this refresh exception.

### 7.3 Kind55 adoption transition

Scope108@0; A16@108; phase:u16@124; reserved:u16=0@126; source_owner104@128; target_owner104@232; manifest:Ref(kind53)@336; map:Ref(kind54)@376; prepared/control_capacity:Ref(kind36)@416; evidence:Ref(kind51)@456; input_watermark144@496; dependency_count:u32@640; dependencies:Ref40[]@644. Exact length644+40n. Header T equals the map's phase/side T; header A equals payload A. Source/target/scope/manifest/map equal every predecessor's immutable values. Dependency order is the explicit order below, not digest order:

|Phase|Journal side|Exact dependencies (n)|Required effect|
|---|---|---|---|
|1 PrepareSource|source|none (0)|Serialize complete source snapshot, reserve protocol capacity, freeze A/manifest; no activation.|
|2 PrepareDestination|destination|phase1 (1)|Authenticate exact preparation and durably retain reconstruction/capacity; nonspendable ready only.|
|3 FenceStarted|source|phase1,phase2 (2)|Durable irrevocable branch intent, serialized against phase7 before any fence I/O.|
|4 AssignSource|source|phase3,phase2 (2)|Verify all enforced old-owner fences/watermarks, commit one target; no source resume/credit.|
|5 AcceptDestination|destination|phase4 (1)|Authenticate assignment/floors/fences and complete reconstruction; sole accepted custody, same original debit.|
|6 SettleSource|source|phase5 (1)|Retain nonspendable provenance/redirect; no credit even if acknowledgement lost.|
|7 AbortSource|source|phase1 (1)|Terminal pre-FenceStarted abort under the same serializer; preserve original charge.|
|8 AbortDestination|destination|phase7,phase2 (2)|Durable abort tombstone before prepared-capacity reclaim.|
|9 AdoptSameLedger|local|none (0)|Authenticate recovery succession, enforced old-writer fences, complete manifest/current index; one expected-revision winner, no balance change.|

Phase3/7 are exclusive. Possible phase3 commit forbids abort, old-owner resumption or target change; reconcile exact A/T forward. Phase5 acceptance does not wait for phase6; source provenance is never spendable custody. Phase9 uses source=old and target=successor in the same admitted fixed-home ledger/genesis, never a home relocation. Phase1/2 evidence includes authorized-succession and reconstruction/capacity requirements; phase4 has role11; phase5/9 have role9/10/11 with actual current floors. Evidence must be noncyclic as §5.1 requires. Missing assignments or proof bytes keep these transitions unavailable.

Accepted adoption changes the authoritative custody decision chain even before per-O AdoptCustody materialization. Until every selected O is materialized under its bound T, fresh mutation stays closed; only exact reconciliation/materialization is allowed. Source snapshots' custodian fields are historical at that point and cannot reactivate source authority. A crash after acceptance is resolved by same-A acceptance lookup plus manifest; a crash after one materialization uses exact per-O T/current-index lookup, not another adoption or a second debit. Closing O may be materialized for recovery but remains Closing. Closed kind58 protection is transferred by accepted manifest/facts without creating a successor allocation or reopening O. No cycle requires a Closed record to refer to a future adoption.

## 8. Exact same-key lookup and repeated operations

Kind56 payload: Scope108@0; DebitOrigin88@108; create_key16@196; operation_class:u16@212 (1 resize,2 adjustment,3 final release); status:u16@214 (1 Pending,2 Terminal,3 ProvenNotIssued); operation_key16@216; selected_snapshot:Ref(kind48/58)@232; current_snapshot:Ref(kind48/58)@272; request:Ref(kind49/50/57)@312; result:Ref(kind48/58)@352; input_watermark144@392; completeness_evidence:Ref(kind51)@536; query_Binding144@576. Exact length720. Header O is the queried O. Header auxiliary T identifies this committed lookup record, never an operation's mutation T. The binding authenticates exact query/request digest, destination/incarnation and purpose; it cannot change the original request binding.

Pending: selected_snapshot is the retained snapshot issuing/beginning the exact key, request equals registry entry, result=zero, current snapshot still carries that operation unresolved (or frozen/adopted reconciliation binding). Terminal: selected_snapshot=result is the original authenticated terminal transition for that key, request remains exact; current_snapshot may be a much later operation or Closed result. Resize terminal is ApplyWholeVector when no shrink remains, the final FinishAdjustment clearing that resize, or successful no-application cancellation. Each adjustment terminal is its exact FinishAdjustment. Release terminal is kind58. Historical lookup returns historical vectors/result **and** authentic current snapshot/floor; it cannot select the historical state as current or grant credit again.

ProvenNotIssued: selected_snapshot=current_snapshot, request/result zero; the complete initialized registry/chain at that exact authenticated floor proves the queried key was never issued, and same request/key namespace checks still apply. This is not proof that a destination effect never happened, not permission to cancel another issued key, and not proof a reserve T was never committed unless the authoritative committed transaction map separately proves it. Registry gaps, corrupt state, unavailable floor or truncated history cannot yield this status. There is no successful Unknown-as-absence response: unavailable/ambiguous authoritative lookup is a non-success reconciliation-required condition retaining all charge and capacity. Exact public error/output-copy ABI remains a separate owner input, not an invented number in this proposal.

A repeated mutation first authenticates same-key/T lookup and exact bytes. Same key with changed target, request digest, input revision, incarnation, profile or operation class rejects before effect. Already-issued keys never reserve anew; later valid operations use fresh keys against the newly authenticated Stable snapshot and preserve all earlier key/results. A lost output copy changes neither commit nor status. A durable request orphan alone is not issued; ambiguous snapshot publication must be authoritatively resolved before calling it an orphan. Requests whose input is stale cannot be patched under their old key/T.

## 9. Reserve bindings, credit protection and authority boundaries

1. Original reserve O/header, Scope, D and create key match every successor record. D remains the original debit/precommit identity even if custody was adopted. Initial kind1 vectors describe initial charge only; all later adoption/lookup bindings use current snapshot vectors. No reserve rewrite, recreated O, alternate genesis or guessed custody.
2. All request input refs and expected allocation revisions equal the authoritative current snapshot at admission. Every selected snapshot predecessor must be exact. Each snapshot proves its predecessor is committed and authoritative at its expected input; auxiliary helper commits do not make a stale O current. Global journal serialization and per-O selection are both checked.
3. Every result is keyed by original O/D/create/operation key and immutable per-step T. Atomic balance/index/key-result publication is required at the fixed-home charge ledger. The issuer may retain debit provenance and settle adoption only; [C03 release amendment](c03-release-amendment-candidate.md) still requires governed approval. Destination application/closure evidence authorizes a verifier decision only within its admitted purpose; it does not itself credit an allowance.
4. Retired resize keys prohibit delayed application after cancellation, while the O/create key remains live for future operations. Final closure additionally installs durable creation/publication closure for the exact incarnation. Keeping only the create key or only a last outcome is insufficient. All owner-side same-key maps/fences and ledger-side original results survive process death/restart/adoption.
5. A possible local reservation, claim, dispatch, whole-vector application, installment credit, final release or adoption commit is retained under its original identity until authentic same-key/T resolution. No timeout, missing response, empty directory, reboot or hash-valid record proves a negative fact. A conflicting fork/stale floor closes the affected scope for authorized forward recovery.
6. Fresh effect dispatch requires both durable global prospective accounting and durable destination local reservation, admitted writer/fence at each real gate, current immutable max and complete control capacity. Authentication check and mutation admission must linearize with fence installation at that gate; a once-valid cached receipt cannot bypass a later fence.

## 10. Self-audit findings and remaining external requirements

### Locally resolved in this candidate

- Selected same-O pending entry and one outstanding resize; no second debit origin or hidden concurrent P.
- Allocated distinct proposed kinds/schema names and exact payload offsets/length formulas; reused rather than redefined the r1 envelope/structures, vectors, facts and capacity. Kind10 is not overloaded.
- Defined current snapshot/predecessor/registry, reserve-to-current equalities, explicit allocation versus journal versus custody revisions, stable transition T and operation keys, canonical active/terminal states, full adoption successor/map and lookup containers.
- Preserved durable reserve-before-dispatch, separately reserved local growth, whole-vector safe application, Unknown retention, positive fold with no credit, qualified installment credits and all-or-nothing no-application cancellation. Arithmetic examples conserve charge; no execution witness is claimed.
- Retained historical keys/results/dependencies indefinitely, defined repeated-future-operation and same-key lookup behavior, prevented self-referential proof/result graphs, and kept final O closure separate from shrink/cancellation.
- Explicitly identified C05's equal-delta conflict and proposed its governed two-plane refinement rather than silently claiming compatibility. Base files, DCO1 r1 and historical C03 remain unchanged.

### Genuinely external or governed admission requirements — still blocked

- **C05 and C03 governance:** actual owner approval of the C05 incremental-global-versus-local distinction and C03 fixed-home-only release wording, successor package revisions/digests/dependent manifest repins, exact-digest CP1 consent and caller closure. This document supplies candidates, not those approvals.
- **Real authority packet:** authorized issuer/genesis/custodian/successor/fence producers, exact scope/home/incarnation/purpose/lane, public verifier pins/suites/revocation, admitted ledger/floor and owner decisions. Engineering owner labels are not operational principals.
- **Exact external proof bytes:** committed-result authentication, complete source/lookup/reconstruction and nonregression evidence, immutable M/profile, local reserve/key-map closure, whole-vector apply/use-fit, all-owner credit coverage/closure, retirement and fence/watermark carriers with assigned per-method producer/verifier sets. Kind51 defines containers only; unavailable proof schemas block qualification and usable codec admission.
- **Actual backend and gate contracts:** durable atomic revision/index/balance/key publication, truthful D1/D2 acknowledgement and old-root preservation, crash/torn-write/copy-loss behavior, replay/fork/rollback refusal and real destination fence linearization. Native VFS, host models and unsigned digests do not establish them.
- **Finite admitted profile and complete runtime ABI:** dimensions/units/maxima, independently finite journal/registry/proof/retention/recovery/work limits and measured high-water costs; actual request/result/refusal/error/output-copy/consumer symbols and all cfg/image/caller migrations. Indefinite retention is logically defined but requires finite refusal limits, not unbounded production allocation.
- **Qualification evidence after approval/implementation:** reserve/ready/application/credit/release and adoption crash cuts, same-key replay/lost replies, conflicting resize, mixed vectors/below-use fit, local growth with G=0, no-app delayed-message closure, stale snapshot/floor, partial-vector contract violation, integer/counter exhaustion and capacity refusal, plus actual backend/owner acceptance. No builds, tests, formatters, deployed proof or durable execution were run or claimed for this documentation-only candidate.
