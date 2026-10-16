# Durable charge ownership and adoption — proposed contract r1

Date: 2026-10-15. **Draft for separate owner review; NOT APPROVED.** No runtime, persistent-byte implementation, endpoint, opcode, signer, issuer assignment or lock handoff is authorized. Ten pending phases,38locks and23downstream CP1/CP2 pairs are unchanged. This contract refines [G3 ownership gates](g3-lifecycle-authority-gates.md#5-unknown-ownership-and-journal-evidence-gate), [issuer ordering](issuer-persistence-candidate.md#62-cross-owner-ordering-not-a-fictional-distributed-atomic-transaction), C03/C04/C05 and [OGJ1 proposal](ogj1-codec-unknown-lifecycle-design.md); it does not replace their independent gates.

## 1. Problem, decision and scope

The standalone OGJ1 model retains process-local charge only; its recovery explicitly reports accounting unrecovered and mutation closed. A dead process cannot transfer an obligation through Drop, an empty lookup, or an in-memory dictionary. Durable ledger responsibility must outlive the process before an effect is possibly dispatched.

Decision: keep **one fixed-home authoritative charge ledger** per admitted application and separately fence the process currently authorized to mutate/reconcile it. Ordinary adoption changes that responsibility, not ledger identity, charge or balance. Bootstrap issuer-to-Authority adoption is distinct custody transfer: issuer precommit remains the immutable debit origin through and after adoption. At B4 the accepted destination alone becomes custodian; issuer thereafter retains nonspendable provenance/redirect records without custody or activation authority, even if B5 is lost. This is not home relocation, fresh genesis or a second allowance. No cross-owner atomic commit is assumed; uncertainty reduces availability, never retention.

This is a semantic contract, not allocated bytes or implementation APIs. All labels below are proposed record/operation names. Concrete widths, canonical encodings, authenticators, errors, per-dimension units and finite limits require exact owner-reviewed candidates before code. A digest binds content, not issuer authority. There is no assumption a complete ownership record fits the experimental256byte body.

## 2. Roles and authority separation

| Role | Required responsibility | Forbidden inference |
|---|---|---|
| Application budget issuer | Authorized genesis, immutable maxima, bootstrap/control precommit; retains pre-adoption charge responsibility until B4, thereafter only nonspendable immutable debit-origin/redirect records | B5 loss does not preserve issuer custody; supervisor/controller consent does not authorize maxima or genesis |
| Fixed-home charge ledger owner | Debit, outstanding obligation set, Closing/release and one credit, accepted adoption/writer revision | A cached controller balance cannot become another ledger |
| Recovery/fence authority | Establish authentic current writer epoch and nonregressing ledger/floor binding; exclude prior writer before granting mutation | Restart count, PID/TID, wall clock or timeout is not a fence |
| Current custodian process | Operate/reconcile only under current admitted writer authority; maintain durable identity and same-T lookups | Process lifetime is not obligation lifetime |
| Destination kernel/lifecycle owners | Keyed publication map, durable closed-key fence and exact incarnation/status correlation | Task absence or boot change alone does not free external obligations |
| Resource/state/effect owners | Exact use, retained pins/leases/results/DMA, state watermark/effect outcome and quiescence | Journal commit alone does not prove external effect outcome |
| Controller/supervisor | Request adoption, orchestrate existing authoritative queries, retain intent correlation | Cannot mint budget, choose a successor by timeout, or automatically assume custody |

Actual principals, public pins, authorized successor sets, fixed homes and fence/storage producers are **unassigned** here. P01/P02/P03/P05/P06/P08-C name accountable engineering owners, not admitted signing principals.

## 3. Identity and semantic record requirements

Every obligation has immutable O and debit origin D=(origin ledger, genesis, initiating debit/precommit entry). Distinguish stable `K_create`, unique per-operation `K_resize`/`K_release`, adoption A, and separate `T_i` for each owner-local reserve/claim/prepare/assign/accept/close/release commit. Retries reuse the exact transition `T_i`; later transitions reference unchanged D/O and parent keys, not a reused transaction with changed bytes. A binds an immutable per-side/per-transition transaction map. All identities use admitted nonreused namespaces. Below T/K shorthand means the applicable bound transaction/keys, never one reused transaction for the entire lifecycle.

Required canonical fields, without selecting sizes/byte tags:

| Record/fact | Complete semantic binding |
|---|---|
| Obligation reserve/intent | Protocol/schema/purpose/lane; application/ledger/genesis and D; O, initiating T and parent/create/resize keys; operation/effect/request digest; full prospective vector and units/profile plus separate effective/charged vector/positive delta; reserved journal/outcome/tombstone/recovery capacity; immutable reservation/debit-origin owner principal/incarnation/home/epoch; ledger revision/predecessor; destination node/boot/artifact/incarnation scope; reconciliation authority |
| Ownership preparation | A and immutable per-side/per-transition transaction map; bounded complete manifest even for singleton O, including dependent Unknown/outcome/closed-key/resize/capacity/pin/state-fence/owner facts and completeness/floor witnesses; old/new custodians/epochs; unchanged vectors/bindings/D; source watermark/digest/floor; target ledger/home; authorized succession decision/verifier purpose |
| Destination ready receipt | A and exact complete manifest identity; durable prepared capacity/state; destination preparation-journal writer identity/epoch and revision/floor only; no charge-ledger/custody/publication/state/effect mutation authority, activation or debit claim |
| Source assignment decision | A, ready receipt identity, source manifest/watermark, target owner/epoch, old-owner fence evidence, irrevocable assignment revision and authority binding |
| Adoption accepted/result | A, source assignment decision, same obligation identities/vector/debit origins, destination durable commit revision/floor; exact custody status; no fresh credit |
| Source settlement/redirect | A and accepted result; original debit remains consumed; old writer stays fenced; retained redirect/tombstone and namespace-retirement constraints |
| Resolution/release | O,T,K; authoritative effect/status and exact incarnation; current custody/writer; closure fence, all-owner quiescence and nonregressing floor; one release transaction identity and one ledger credit revision |

The accepted-state snapshot and replay history must include Unknown, prepared/assigned/accepted-but-unsettled adoptions, closed keys, outcomes, duplicate-credit protection, debit origins and epochs. Neither truncated directory enumeration nor a partial manifest establishes completeness. Exact per-record authenticators/producer assignments and mandatory-versus-method-specific fields remain byte-candidate inputs; missing required bindings refuse.

Reserve owner/epoch and D are immutable provenance, not current custody. Effective custody derives only from the unique authenticated nonregressing assignment/adoption decision chain. Recovery rejects forks; it neither rewrites reserves nor selects their stale owner. B3 names the only eligible successor but permits no fresh transferred-scope effects before B4 acceptance.

## 4. Conservation and retention invariants

For each dimension d, `charged[d] = sum(O.base_charged[d]) + sum(O.pending_positive[d])`, counted once within the admitted parent. Base is the last conservative charged allocation, not the target. Old5→target7 reserves2 once:5+2, never7+2. Committed whole-vector application folds positive into base and clears its pending term atomically without changing total. Mixed old(5,8)→target(7,3) remains charged(7,8) through application; only the later acknowledged fixed-home credit after safe enforcement evidence reduces base to(7,3). Unknown retains the conservative state and all accepted positive deltas, blocking conflicting resize. Adoption changes no term; recovery mirrors create no allowance. Finite protocol overhead is reserved once inside admitted control envelopes before preparation/B1 and included unchanged in all adoption totals, never debited again at completion.

1. Exactly one debit-origin identity and one logical outstanding obligation survive every transfer. Across issuer/destination ledgers, records are custody mirrors/redirects of that same charge, not independent budget. Pending issuer adoption continues to consume issuer maxima and cannot be reused.
2. At most one custodian may obtain usable mutation authority at a time. Both may retain records; record replication is not dual authority.
3. Before any possible effect, the existing owner must durably reserve O, the complete prospective vector and bounded reconciliation/history capacity. If no preexisting durable ledger/recovery owner can retain the intent on owner death, refuse dispatch. Volatile preparation before proven nonacceptance may unwind; possible accepted reserve remains Unknown under the same O/T.
4. Unknown retains its full conservative prospective vector. Mixed resize never nets growth against unreleased shrink. Adoption of Unknown changes who must reconcile, never its certainty or amount.
5. Release requires actual owner-specific closure/quiescence and one atomic ledger Closed/credit transition. Adoption completion, process death, cancellation, timeouts and absence do not release.
6. Each journal/publication/resource/state/effect owner SHALL linearize epoch validation plus mutation admission/commit against fence installation at the same authoritative gate. Check-before-pause followed by post-fence effect is forbidden. Fence acknowledgement binds the last admitted/committed watermark and unresolved old work; such work stays charged and blocks conflicting successor mutations until reconciliation. Once-valid receipts cannot bypass this gate.
7. Exhausted journal/manifest/outcome/epoch/retirement capacity refuses before new effects; no overwrite/TTL eviction or reset to create space.

## 5. Same-ledger custodian recovery/adoption

Use this path for service/supervisor process loss without moving the application's ledger home.

1. Close fresh mutation. Recover the same admitted ledger/genesis/floor and full retained obligation/adoption state, not a fresh empty ledger. Bad/corrupt/missing initialized state stays RecoveryFailed.
2. Authorized fence authority establishes successor epoch and excludes old mutation at every relevant commit/publication/state-effect gate. Persist successor/fence decision before permitting the successor to mutate. In-flight old requests that already committed are queried at their original T; those possibly accepted remain retained. If a destination cannot enforce the fence, no new conflicting effect is allowed there.
3. Reconstruct the complete outstanding charged vector, retained capacities, original operation outcomes and owner dependencies from the qualified ledger and authoritative lookups. Kernel same-boot keyed map participates for publication; boot-Gone is not all-owner quiescence.
4. Durably commit `CustodianAdopted(A, old_epoch, new_epoch, exact_manifest, watermark)` in the same ledger under the new epoch. Commit must reject concurrent candidates/stale expected revision; only one successor gets accepted. Lost response resolves by A lookup; never increments another epoch merely to retry the same adoption.
5. Only after observed adoption commit success **or** authenticated same-A lookup proving that exact committed adoption at a valid nonregressing revision/floor, complete reconstruction and all fence enforcement may successor receive usable mutation permission. Unknown/conflicting/regressed lookup remains closed. Unknown O remains charged and may block conflicting mutation.

An unreachable old process need not voluntarily sign, but only the preauthorized recovery/fence authority can supersede it using an enforceable nonregressing decision. No timeout self-election or guessed successor is permitted. Nonregressing fencing and the actual recovery authority are unmet external/implementation gates, not provided by the Python model.

## 6. Bootstrap issuer-to-Authority custody adoption

This narrowly handles the precommitted genesis/control charges in C03; general live ledger/home migration is out of scope. Global control operations wait for adoption. No successful handoff acknowledgement is inferred across journals.

| Step | Required durable fact and ordering | Authority allowed |
|---|---|---|
| B0 Issuer owns | Unique issuer genesis/precommit and full control envelope already debited before one-use bootstrap publication | Issuer holds original charge; destination cannot self-mint allowance |
| B1 Prepare source | Under source serializer, bind A to complete retained manifest, target/home and source watermark; reserve all protocol overhead and freeze changes to transferred scope | Source may perform only reconciliation preserving frozen bindings. Before FenceStarted, a manifest-changing resolution requires committed abort and a fresh reviewed preparation. After FenceStarted may have committed, retain late facts and recover the same A/target forward; never reprepare |
| B2 Destination prepared | Destination authenticates authorized assignment intent and capacity, writes immutable `AdoptionPrepared` and returns ready receipt | Prepared records are nonspendable mirrors; no activation or charge credit |
| B2F FenceStarted | Source first durably records non-abortable fence intent for exact A/target under the same serializer as Aborted, then installs old-custodian fences | Aborted cannot win after FenceStarted; partial/uncertain fence progress stays mutation-closed and recovers same A forward to B3, never old-epoch resumption |
| B3 Source assigned | From FenceStarted only, source verifies ready receipt, completes all enforced fences/watermarks and durably records irrevocable target assignment | Old custodian cannot resume; issuer charge consumed; no target activation until B4 |
| B4 Destination accepted | Destination authenticates committed B3 decision/current fencing, verifies exact retained manifest and capacity, durably installs accepted custody/result with original debit origins | Result may be looked up by A; usable Authority mutation still requires complete genesis/bootstrap reconciliation and current gate checks |
| B5 Source settled | Source authenticates B4 result and durably records redirect/settlement retaining original debit/tombstone | No credit and no reusable bootstrap envelope; same-A lookup returns result/redirect |

Abort is permitted only before durable `FenceStarted`. Source commits terminal `Aborted(A,exact_manifest,target,transaction_map)` under the same serializer/expected revision as FenceStarted: exactly one wins. No fence I/O may begin before that durable non-abortable intent; uncertain intent requires authentic same-A lookup and no abort/resumption. Only committed authenticated Aborted permits destination abort tombstone then prepared-capacity reclaim and source unfreeze under still-valid existing epoch, without refunding O. Lost reply queries the original A/abort T; delayed fence/assignment/accept for aborted A rejects. Once FenceStarted or Assigned may be committed, no abort or old-epoch resumption: recover same A/target forward; unavailable gates/target keep charge held and mutation closed. Another target needs separately reviewed forward reassignment, not supplied here.

If B4 succeeds but B5/reply is lost, destination remains the sole assigned custodian and source retains a nonspendable debit-origin record. Source must not refund or activate itself. Transfer does not make bootstrap unknowns disappear: each published control key and issuer obligation must be reconciled/adopted before ordinary controller claims. No transient window grants both owners mutation rights.

## 7. Crash and uncertainty disposition

| Cut / adverse input | Required recoverable status | Forbidden result |
|---|---|---|
| Before proven accepted reserve | Definite nonacceptance permits bounded local unwind; ambiguous reserve retains original O/T | Generic error interpreted as nonacceptance |
| After reserve, before effect or payload acknowledgement | Durable owner retains prospective charge; query same O/T, no effect replay | Destructor refunds on owner exit |
| D1/D2 failure or reply/output-copy loss | Exact committed status or retained Unknown/RecoveryFailed; reconcile same T | New key, second debit or successful empty recovery |
| Same-ledger fence persisted, adoption not acknowledged | Scope closed until same-A decision and reconstruction proven | Old writer resumed or concurrent successor granted |
| B1/B2 crash or ready reply lost | Source owns; prepared destination nonspendable; query A and authoritative assignment | Destination activates from ready alone |
| B3 write/reply uncertain | Both must query authentic source assignment at A; no competing owner allowed | Abort by timeout or source refund |
| B3 committed, before B4 | Assigned target only; mutation closed until accepted; original charge consumed | Source resumes because target unreachable |
| B4 payload/commit/D2/response uncertain | Query same A and its immutable destination-accept T at destination journal; exact authentic committed status required before activation; retain preparation/charge | Activate from possible acceptance, remove preparation or create a fresh acceptance identity |
| B4 committed, reply/B5 lost | Same-A accepted status; source pending redirect, no second activation/credit | Reissue under A2, credit issuer or discard adopted Unknown |
| Ledger/floor/epoch restored backward | Seal all affected mutation; preserve evidence for authorized forward recovery | Trust older plausible balance or reset epoch |
| Both processes restart / node reboot | Recover original durable decision chain, original boot/key and all external obligations | Treat reboot as total quiescence or new genesis |
| Duplicate A / changed manifest, wrong target or purpose | Exact repeat status only; changed binding rejects before effect | Overwrite original assignment or widen rights |
| Compaction/storage saturation | Preserve current and predecessor decision/Unknown/closed-key protection until admitted retirement fence | Evict pending custody or credit protection to free capacity |

Truthful barrier failure models and no-ack ambiguity must be exercised on the admitted backend. A digest-valid foreign record still rejects authorization. Corrupt/torn present records cannot be downgraded to absent preparation.

## 8. Proposed operation contracts and ownership handoff inventory

These names are requirements, not implemented symbols or wire allocations:

- `LookupObligation(O,T)` and `LookupAdoption(A)`: authoritative exact status, ledger revision/floor and original bindings; absence requires a proven complete initialized scope and never alone permits release.
- `PrepareAdoption(A,manifest,target,expected_revision)` / `PrepareDestination(...)`: authenticated, bounded, idempotent immutable preparation. No success until respective commit acknowledged.
- `AssignCustody(A,ready,fence,expected_revision)`: source-serialized one-target assignment; exact same-A lookup on uncertainty; cannot credit.
- `AcceptCustody(A,assignment,manifest)` / `SettleRedirect(A,accepted)`: independent destination/source commits; explicit no-cross-owner-atomicity; no new budget.
- `AdoptSameLedger(A,successor_epoch,manifest,watermark)`: exact admitted recovery/fencing facts plus reconstructed charges; only same-home succession, not relocation.
- `ResolveObligation`: gather authenticated outcome/closure/all-owner evidence without credit. `ReleaseOnce(D,O,K_create,K_release,T_release,closure_manifest,expected_revision)`: only the fixed-home authoritative Budget Authority ledger owner commits Closed/Released and one credit atomically. Duplicate/D1/D2/reply uncertainty queries the same nonreused release transaction. Issuer bootstrap settlement is no-credit provenance/redirect retention; it is not an alternative release-credit authority.

Before implementation, each operation must name real producer/consumer symbols, complete canonical request/result/error/output-copy table, locks/serialization/atomic revision checks, failure class, retained bounds and cfg/image/migration/removal callers. VFS namespace/quota cleanup, supervisor exit/restart, kernel publication/retirement and effect/state owners must be reviewed together; existing RAII cleanup is not a durable ownership primitive. Do not hold a scheduler lock while assuming storage/network progress. No new implementation paths are selected here.

## 9. EARS review obligations and future witnesses

These local DO identifiers are review obligations, not new Spec25 acceptance or approval IDs. All implementation witnesses remain **pending**.

| ID | Requirement | Required observable witness |
|---|---|---|
| DO-01 | WHEN an effect may be dispatched, the owner SHALL first durably retain O/T/full prospective vector and bounded recovery capacity. | Kill writer before/after reserve commit and dispatch; exact charged O survives with no duplicate effect |
| DO-02 | WHILE reserve/effect outcome is uncertain, the current owner SHALL retain full charge under the original identities. | Short write, D1/D2/reply/copy loss; same-T status, no refund/new key |
| DO-03 | WHEN a same-ledger successor adopts, all old mutation gates SHALL enforce the committed successor fence before fresh successor effects. | Old queued mutations race adoption at journal/kernel/state owner gates; no stale commit/publication |
| DO-04 | WHEN two successors compete, at most one adoption SHALL commit for the expected epoch/revision. | Concurrent authentic requests, lost winner reply, duplicate retry; one accepted owner |
| DO-05 | WHEN destination preparation completes, it SHALL remain nonspendable until authentic source assignment and accepted custody. | Attempt mutation with ready receipt only; refusal before effect |
| DO-06 | WHEN source assignment commits, source SHALL remain fenced and preserve original debit through destination outage. | Crash after B3 and partition target; no source resume/refund |
| DO-07 | WHEN B4 or settlement reply is lost, retries SHALL query the same A and never debit/credit twice. | Every B3/B4/B5 barrier and reply cut; same target/O/vector/debit origin |
| DO-08 | WHEN an owner exits/reboots, recovered custody SHALL preserve every Unknown/outcome/closed-key and retained capacity. | Both-owner restart with accepted unresolved effects; exact vector/manifest reconstruction before admission |
| DO-09 | IF any binding/manifest/floor/epoch is corrupt, foreign, regressed or incomplete, mutation SHALL remain closed. | Wrong ledger/target/purpose, missing record, valid-hash substitution, stale disk/fork and overflow controls |
| DO-10 | WHEN adoption completes, charged totals SHALL remain exactly equal per dimension across completion. | Protocol overhead reserved once before preparation/B1 and included unchanged in before/during/after totals; no extra debit or credit |
| DO-11 | WHEN release occurs, only authoritative closure/quiescence plus one atomic release/credit SHALL reduce charge. | Duplicate release/adoption, delayed create, resource pins/DMA/state writer; one credit, no late publication |
| DO-12 | IF retained decision capacity is exhausted, new work SHALL refuse before effect while existing custody/replay protection remains. | Saturation during prepare/assign/accept/recovery/compaction; no eviction or hidden unbounded allocation |
| DO-13 | WHEN preparation aborts, source SHALL durably serialize terminal Aborted against FenceStarted before any fence I/O or reclaim/unfreeze; no O credit. | Concurrent/delayed FenceStarted versus abort, uncertain commit/lost replies; one winner, no fence I/O after Aborted, no abort/old-epoch resumption after possible FenceStarted; destination tombstone before reclaim |
| DO-14 | WHEN a snapshot/retirement is proposed, it SHALL retain all adoption states/debit origins/fences until authoritative replay-safe retirement. | Crashes on both sides of checkpoint switch plus delayed old adoption/release requests |

Each future receipt binds exact source/image/toolchain/profile, backend/cache/device/failure tuple, issuer/public-pin/floor revisions, commands/exits, raw cuts, full state/charge expectations and observations. Software model witnesses never stand in for authentic deployment authority or actual backend durability.

## 10. Required decisions, verification boundary and rollback

Blocked inputs: actual authorized succession/recovery principals and public verifier pins; same-home enforceable fencing at every effect gate; issuer/destination storage/floor admission; complete charge/vector and manifest schemas; independently finite journal/recovery/decision-retention/work budgets; real caller migration; exact-byte CP1, then actual implementation/CP2 and owner acceptance. The OGJ1 trailer-reference ambiguity is still unresolved for persistent approval. No credential/private key is requested.

Review must decide: who may fence/adopt; which exact commit gate rejects a stale writer; how complete manifests and external obligations are proved; which failures prevent nonassignment proof; how control overhead and debit origins are counted; and what durable floor prevents old assignment/release replay. Where these answers are absent, the affected path is unavailable, not assigned to a fake default authority.

Verification of this document can check structure, links and gate posture only. No durable adoption, ownership transfer, crash protocol or conservation implementation has run for this contract. Existing OGJ1 corrected smoke proves only mutation-closed model recovery and live-process retention; it does not satisfy DO-01..DO-14.

Before implementation, rejection/revision changes documentation only and preserves evidence. After any eventual durable reserve/assignment/effect, binary rollback cannot undo custody, fences, debits or outcomes: close admission, retain both sides' original records and use separately authorized compatible forward recovery. Never restore an older ledger, refund unresolved O, reset epochs or reanimate the source owner. Ordinary home relocation, arbitrary multi-ledger transfer, runtime VFS/kernel integration, protected storage/time/TLS and production admission remain out of scope.
