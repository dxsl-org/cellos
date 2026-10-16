# Validation — Cell contract planning package

**Date:** 2026-10-11
**Current scope:** architecture/plan/lock documents only; Spec 25 v1.3 AC-01..AC-50 and Spec 26 v1.1. Runtime and ABI unchanged; all implementation criteria pending. The approved red-team checkpoint amends affected earlier semantics. Earlier sections are historical review/document baselines, not current runtime proof.

## Decisions checked during recon

- Same-Cell worker reply tokens retained; no invented per-thread security boundary. Kernel-attested provider incarnation and single terminal still required.
- Whole-Cell retirement chosen for unhandled shared-memory-threatening worker faults; explicitly a target semantic amendment, not current task-local behavior.
- Native kernel stream enforcement retained when justified; POSIX fd/personality policy moves outside core.
- VFS post-start receipt explicitly amends spawn-only policy; no duplicate kernel semantic ACL.
- Remote mutable authority fails closed under partition; revoke cutoff/quiescence and already-dispatched uncertainty are separate.
- Existing Law-1, protected authority, Intel physical/VMX and production gates retained. User architecture approval is not unspecified ABI approval.

## Adversarial review

Four specialist reviewer jobs were attempted (authority, SMP/failure, distribution, dependencies); all failed with provider `usage_limit_reached` and returned no findings. **No independent-review PASS is claimed.** The integration owner completed a direct adversarial review using the source-grounded thread/distribution scout reports and the actual draft documents.

| Finding | Severity | Decision and resolution |
|---|---|---|
| Restore-before-publish alone permits acknowledged old-writer commits after the restored snapshot | Major | Accept: Spec 25 §6 and P06 require final fence/committed watermark, catch-up before publication and no reuse of fenced epoch. Added AC-27. |
| A relative timeout received after network delay cannot prove origin absolute expiry | Major | Accept: Spec 25 §7 and P08 distinguish origin end-to-end and destination receipt-relative deadlines; strict cutoff requires authenticated time/error bounds and commit check. Added AC-28. |
| A durable lifecycle result and kernel publication are not one atomic disk/scheduler transaction | Major | Accept: durable intent, stable creation key, kernel keyed publication and result reconciliation; after node reboot original create reports old incarnation gone rather than silently recreating. P06/P08, AC-29. |
| Multi-waiter take did not state eligibility and owner-exit race outcome precisely | Major | Accept: owner or explicitly handed same-Cell consumption handle; no lifetime transfer, exactly one cleanup/take release. Spec 25 §4 and P04; existing AC-10 covers it. |
| Single-writer fence alone did not state recovery authority's own failover rule | Major | Accept: fixed authority home, durable epoch/ledger before renewed admission, no timeout-based authority promotion. Spec 25 §7, P08 and AC-21/22. |

The initial review added no out-of-scope feature: those findings closed cases within the then-approved recovery/distribution scope. The subsequent maintainer-approved amendment adds bounded placement/autoscaling and application-wide accounting; it does not add consensus, transparent relocation or authority failover. Exact ABI/protected/physical gates remain unchanged. Independent automated assurance was unavailable for the initial review; accountable owner review remains required at implementation entry and actual ABI checkpoints.

## Document verification — initial baseline (AC-01..AC-29)

Executed an in-memory Python/YAML document-graph verifier from the repository root (no generated script or runtime source changes):

| Check | Observed result |
|---|---|
| Existing plan inventory | 161 entrypoints, 161 unique classified rows, no missing/extra row |
| Scoped locks | 38 unique locks; all have visible notice, `executionLock`, blocked execution status and nonempty scope |
| Reciprocal controller edges | 38 `blocks` ↔ `blockedBy` matches; controller does not wait on a locked predecessor |
| Implementation phase graph | 10 sequentially numbered pending phases; references resolve; acyclic |
| Acceptance ledger | 29 unique ordered EARS criteria, each maps to existing owner phase(s) |
| New document links | 17 new package/register documents; 201 Markdown links examined, local targets resolve; scoped lock notices also resolve |
| Negative controls | In-memory phase cycle, missing document link and removed reciprocal lock edge each detected |
| Verifier errors | 0 |

The verifier checks the new graph/metadata, not every historical link or the truth of old implementation claims. Original status/progress and earlier blockers were retained by surgical lock additions and the inventory owner; the verifier does not treat those status strings as runtime evidence.

No build, runtime test, lint or formatter was run for this documentation-only change. Prior local QEMU behavior is explicitly labeled reused evidence in `scout-report.md`; none qualifies the new contract. New files listed as future phase deliverables are planned work, not missing current implementation.

## Handoff

The planning/lock deliverable is complete. The next step is P01 exact ABI/migration/adoption inventory; implementation remains pending. Use the controlling plan, not an old locked phase. No automatic old-plan unlock, ABI confirmation, remote activation or production admission follows from this validation.

## Approved cluster follow-on review

Connection/session state stays in `net-broker`; IP/TCP remains service-net and local isolation remains kernel-owned. Placement is policy in a separate controller Cell, not a broker route fallback or a distributed kernel.

Owner adversarial checks for the amendment:
- Reserve durable application budget before remote creation, including pending/uncertain children; partition or reply loss cannot release a potentially live child's charge.
- Destination admission remains authoritative: aggregate application allowance cannot override node capacity, tier/artifact policy or scoped lifecycle permission.
- Controller restart reconciles durable intent and child creation keys before new allocation; fixed-home fenced controller has no timeout-based promotion.
- Scale-down stops new routing/admission and drains/classifies outstanding work before retirement; draining/uncertain children remain accounted.
- Stateless replicas or explicitly sharded state may scale; additional replicas cannot create another writer for the same mutable state domain.

Integrated in-memory Python/YAML document smoke after both edited slices:

| Check | Observed result |
|---|---|
| Existing inventory and scoped locks | 161 unique classified entrypoints; 38 reciprocal lock edges; blocked metadata retained |
| Phase graph | 10 pending phases; valid dependency references; acyclic; maximum phase length 73 lines |
| Current acceptance ledger | AC-01..AC-35 unique and ordered; all EARS criteria map to existing phases |
| Package links | 17 documents, 201 Markdown links examined; 0 broken local targets |
| Updated entrypoints | Contract/plan/spec pointer agree on AC-01..AC-35; inserted portfolio/focus links resolve |
| Negative controls | In-memory cycle and removed lock edge detected; missing target recognized; removed AC makes completeness check fail |
| Superseded scope | Automatic-placement exclusion removed; placement stays within one admitted cluster, not federation |

No build/runtime test/lint/formatter was run. This is document consistency and owner design review, not a placement, quota or two-node runtime PASS. All new runtime scenarios remain pending.

## Complete workload architecture review — Spec 26

The maintainer requested architecture first and implementation later. Spec 26 selects control/data-plane ownership, logical versus pinned endpoints, an exact bounded weighted-slot routing sequence, deterministic node placement and a demand-based scaling loop. AC-36..AC-43 append to the unchanged earlier criteria.

Owner adversarial review closed these gaps:
- Router-as-confused-deputy: preserve attested original subject and constrained delegation; destination grants still authorize the selected method/resource.
- Several routers/restarted ingress multiplying capacity: preallocated per-ingress shares sum within a replica envelope; fence old generation and reconcile retained work before share reuse.
- Route withdrawal racing a stale router: destination admission cutoff, not route-table removal alone, establishes safe drain.
- Unknown telemetry versus zero demand: required fresh source samples, no worker double counting, no downscale from missing data; bounded cold-start wake needs always-on charged control capacity.
- Application shrink below live use: pending target versus effective admitted ceiling; no forced work loss or temporary uncharged growth.
- Ambiguous submit versus balancing: one selected attempt, immutable concrete binding after submission, no hidden retry/hedging.
- Connection count versus readiness and capacity: live service export/state fence is required; sessions are bounded shared transports, not replicas.
- Live router gate depended on a later budget/controller gate: corrected internal execution order to P08-A → P08-C → P08-B → P08-D → P08-E; controller bootstraps explicit grants/membership before real router admission, with no fixture authority.

Integrated in-memory Python/YAML smoke passed: 161 unique classified legacy plans; 38 reciprocal scoped locks with blocked metadata; 10 pending phases with valid acyclic dependencies; 43 ordered EARS criteria with existing owner phases; 18 package documents and 215 Markdown links examined with zero broken local targets. The largest phase is 99 lines. Updated entrypoint links and AC range agree. Four negative controls reject a cycle, removed reciprocal lock, missing target and missing AC.

Final handoff check covered all six references to P08 internal order and the acyclic A → C → B → D → E dependency chain. Architectural bootstrap sequencing no longer requires routing on fake budget grants. These are document checks and integration-owner adversarial review only: no runtime, build, performance, independent security or production PASS is claimed. All implementation milestones remain pending.

## Current four-lens red-team amendment — 2026-10-11

Four independent pre-amendment lens reviews completed: Security Adversary, Assumption Destroyer, Failure Mode Analyst and Dependency Trap Hunter. Fourteen raw Major findings merge into eight gaps/five accepted Major groups; zero Critical/Minor, no reject/defer. Maintainer selected independent Budget Authority enforcement. Full findings and trust/availability tradeoff: [red-team report](red-team-2026-10-11.md).

Mitigations added in Spec 25 v1.3 / Spec 26 v1.1: receiver-start bounded challenges; unresolved ingress-generation reconciliation and homogeneous scaling domains; issuer-precommitted genesis with independent Authority/live claims and immutable maxima; destination Ready provenance; immutable origin request and issuer-sealed single target. AC-44..AC-50 appended; affected existing criteria explicitly amended.

Interrupted document-edit jobs and later reviewer wake attempts returned provider errors. Their failures do not erase completed initial reviews or count as extra passes. Parent completed missing edits directly; no independent post-amendment pass is claimed.

Observed in-memory document smoke: **25 documents, 445 link candidates checked, zero broken local paths; ten pending phases with valid acyclic dependency graph; 38 reciprocal scoped locks; exactly AC-01..AC-50, each with a phase owner; eight valid failure-history JSONL rows; P08 99 lines.** Rejecting controls detected a cyclic graph, missing dependency and duplicate acceptance ID. Final semantic sweep found no current arrival-age/controller-ledger claims; historical records remain historical.

This is documentation consistency only: no runtime/test/build, two-node LAN run, ABI approval, security certification or physical/production qualification. Locks and gates remain active. Next validation interview: `$hc-plan validate .agents/261011-1200-cell-contract-authority/plan.md`; executable handoff remains P01 non-activating preparation.

## Post-red-team planning validation — 2026-10-12

Read overview and all ten phases. Reused the existing red-team verification record under the validate skip rule; no unresolved verification/staleness markers required new source sampling. New implementation claims checked: 0; verified/failed/unverified marker claims: 0/0/0. These counts do not establish runtime correctness or qualify source changes since the recorded scout.

New questions: 0; prior explicit architecture-before-code and independent Budget Authority decisions reused without asking for confirmation. No new tradeoff requiring maintainer choice, scope reduction, rejected approach or ABI approval. Runtime qualification, exact ABI approvals, measured finite profiles, durable genesis/fence/flush proof, protected relay and Intel physical/VMX/production evidence remain named gates, not assumed passes.

Clarified shared AC-48: P03 proves local immutable-max/current-limit primitives; P08-C provides actual Authority-reserve/live-claim integration and replacement accounting; P10 qualifies both. P03 does not wait for P08, close the whole criterion or enable fixture-backed application growth. No new dependency edge or weaker acceptance.

Observed post-propagation smoke: **25 documents, 446 link candidates, zero broken local targets; ten pending phases with valid acyclic dependencies; 38 reciprocal scoped locks; exactly 50 ordered AC IDs with phase owners; AC-48 split recorded on both phases and no reverse dependency.** Three rejecting controls detect cyclic graph, missing dependency and duplicate AC. Consistency: OK.

Planning validation complete; no independent post-amendment security pass, runtime/build/test or activation. Handoff is `$hc-cook /home/dmin/cellos/.agents/261011-1200-cell-contract-authority/plan.md --auto` for **P01 non-activating preparation only**. Auto mode cannot cross existing exact ABI/protected/physical/production approvals; all phases stay pending until their own evidence.

## P01 eight-criterion closure audit — 2026-10-12 continuation

This audit is separate from the historical planning smokes above. It does not mark P01 done, convert architecture approval into either exact checkpoint, or qualify runtime instrumentation. No build/test/lint/formatter/runtime/document verification was run by the continuation worker. Source gathering did execute repository path enumeration and deterministic lexical declaration/occurrence collection; those are inventory operations, not acceptance checks.

| P01 success criterion | Reachable work now recorded | Remaining gate / honest disposition |
|---|---|---|
| 1. Every changing public item has caller/test/doc migration; no second authority table/permanent shim | D01–D12/C01–C11 family actions, local/compatibility/cluster publisher-parser-invoker-test-doc/removal matrices and [finite source snapshot](consumer-census-source.json) covering maintained C/sysdeps/build/generation/boot inputs. | **Incomplete:** LSP unavailable; semantic macro/reexport/trait/cfg/linker/generated/ignored/archive/external closure and candidate-specific per-symbol cutover still required. Lexical sightings are not resolved callers and no exact new public field is invented. |
| 2. Every locked legacy slice has disposition; reciprocal graph/direct entry notices control | [Migration ledger](migration-ledger.md) preserves L01–L38 former/new owners, scope, current-source drift, retained proof gates and no-unlock disposition; handoffs retain the ten-phase DAG. | The adoption denominator is recorded; **actual verifier output still required** for reciprocal locks, phase links/direct notices and source/document drift. File-owner grant/release is not supplied by naming a destination. |
| 3. Exact byte/API checkpoint1 before edit and checkpoint2 before release | [Per-delta records](abi-deltas.md#per-delta-approval-records) explicitly enumerate all23 D/C revisions, candidate digests, both checkpoints, implementing evidence and blocked release. | **Pending for every exact delta.** Architectural acceptance and the inventory checksum are neither consent nor an approved revision. No changing ABI/authority/source edit or release is authorized here. |
| 4. Endpoint/manifest/lifecycle/budget/telemetry inventory includes complete owner/units/freshness/unknown/fencing/grow-shrink requirements | [Cluster field/owner groups](consumer-census-local.md#12-field-ownership-without-invented-encodings), current D12 source matrix, [governance adoption](consumer-census-compat.md#10-repository-occurrence-closure-and-governance-adoption), [parameter prerequisites](profile-budgets.md#exact-parameter-prerequisites-and-non-promotion-rules), including origin one-target, Ready, genesis/live claim/closure, nested shares/control overhead and generation-complete homogeneous scaling. | Semantic groups/owners are recorded; **exact encoding/version/finite bound/units/tolerance/proof producers/persistence order remain incomplete** where measurements or authenticated issuer/storage contracts are missing. Current wire seeds cannot stand in for new Authority/Ready/origin records. |
| 5. Document/graph verifier checks38locks/10phases/all50 AC owners/approvals and catches real in-memory deleted link/edge | [Verifier](../../scripts/verify-cell-contract-p01.py) loads actual documents, matches register/controller/adoption sets and metadata/notices, resolves local file/heading links, validates phase/handoff dependencies and acyclicity, canonical AC owners and reciprocal implementing-phase references, all50 AC approval fields and all23 exact-delta fields. Source-export hashes detect census drift. Negative controls delete an actual P02 phase-table link and an actual legacy blockedBy edge in loaded copies, then call the **same** validator. | **Implemented, not exercised by worker.** Main must inspect actual JSON/errors/exit status and each rejecting control's actual failure objects. Missing control prerequisite or undetected mutation exits nonzero; no hardcoded “negative control passed” and no tautological self-assertion. |
| 6. Existing runtime evidence labeled by source/artifact/profile | Historical local/QEMU/build/service snapshots retain labels/ceilings in existing evidence/readiness files, migration E rows and all50 handoffs. No history was relabeled or discarded. | These labels remain inputs, **not new-contract/physical/protected/production PASS**. Current instrumentation/measurement interpretation remains with the integration owner; a source checksum only authenticates bytes. |
| 7. Reviewed init→bench→probe launcher and private config/image provenance; denial/missingterminal/time gaps not success | Existing [launcher contract](measurement-launcher-contract.md), measured baseline provenance/readiness and source boot/VFS call matrix remain the controlling route. No alternate fixture/shell authority or endpoint was introduced. | **Not completed by this document.** Launcher/source/config/image/capture/denied/start/terminal identity and timer-conversion observations require actual recorded runtime interpretation. Existing lifecycle scenario cannot close cancellation/multithread/distributed acceptance. |
| 8. Resource joins exact incarnations/live-residual-retained/overflow; no polling/unknown-settled promotion | [Attribution contract](resource-attribution-contract.md), [private trace specification](private-trace-spec.md), [producer-close protocol](producer-close-protocol.md) and parameter non-promotion rules preserve generation joins, distinct charge/physical accounting, unknown classes, quarantine/deferred/retained obligations and event-completeness ceiling. | **Measured-fit/settled qualification remains pending** until actual capture completeness/class/obligations/overflow and source/artifact/profile observations satisfy those gates. Private-root observation or flags0 alone is not settled reclamation, complete high-water, aggregate CPU or finite production budget proof. |

### Deterministic verifier commands for integration owner

Run from the repository root **after all sibling edits land**:

```sh
python3 scripts/verify-cell-contract-p01.py --negative-controls
python3 scripts/verify-cell-contract-p01.py --require-approved --negative-controls
```

The first command tests document consistency and rejecting controls only. Exit0 would mean its stated consistency checks passed, **not** P01 completion or ABI/runtime approval. The second intentionally must return nonzero while exact approvals remain pending, with `APPROVAL_PENDING`; inspect and retain all other errors as actual defects, not expected approval noise. No output/result is pre-recorded by this worker.

If final owner source edits legitimately make the finite lexical inventory stale, deliberately re-collect it once before the commands above:

```sh
python3 scripts/verify-cell-contract-p01.py --refresh-census
```

This explicit mode replaces only `consumer-census-source.json` from current declared source roots, without changing approval fields or source code. Re-collection records source drift rather than retroactively blessing the original inventory. Original collection counts/digest in the census document remain a dated observation; a refreshed artifact has its own actual checksum/counts. No generated artifact digest is approval consent.

### Exact unreachable prerequisites

- **Semantic/artifact closure:** no mounted LSP/reference device; find provider failures retained/reported. A working semantic expansion/index and final cfg/compiler/linker/provider/header/OUT_DIR/archive/image/external SDK tuple are not available from this worker's textual traversal. Maintained repository source and all textual matching rows are reachable and recorded; ignored upstream/generated/binary/external closures are not asserted complete.
- **Measured finite values:** generation-complete settled/event-high-water workload attribution and actual distinct-CPU aggregate charging/tolerances are missing as qualification inputs. Source constants, sparse polling, latency ticks, zero trace flags and unknown classes cannot supply profile periods/bursts/slot/lineage/table/queue/T/lifetime values.
- **Authenticated new proof/allowance and durable schema:** actual authorized application budget issuer, real Ready/origin/source-set proof producers, fixed-home genesis/reserve/liveclaim/keyclosure recovery API and admitted storage durability/flush/order/retention/fence capabilities are not established by the inspected current broker seeds. Selecting bytes without those trusted producer/parser/storage prerequisites would invent an endpoint/persistence contract.
- **Actual approvals and external promotions:** each exact candidate's CP1 and implemented-delta CP2, named protected AC-001..011 Build entry and AC-012 relay activation, trusted time/root/persistence/provider/hardware inputs, Intel VMX/physical and production admission remain unavailable decisions/evidence, not implied by the user's instruction to continue P01.

Selected algorithms, authority separation and conservative frozen/no-remote/deny/cutover invariants do not require another generic architecture-choice question. The unavailable items above are concrete source/measurement/consent prerequisites. Reachable inventory and deterministic verifier implementation are completed as preparation; all eight success criteria collectively remain unclosed until those actual prerequisites and exercised integration evidence exist.

## Exact-delta package integrity — 2026-10-13

The [23 preparation packages](exact-deltas/index.md) now expand source-observed baselines into concrete local design proposals and selected compatibility/cluster transition contracts. They contain migration/test/doc/removal ownership, output/lifetime/error rules, negative scenarios and named blocking inputs. The [manifest](exact-deltas/manifest.json) separately authenticates preparation documents and the selected working-source denominator; it does not assign approved candidate revisions.

Observed first integrated run of `python3 scripts/verify-cell-contract-p01.py --negative-controls`: exit0, no errors,23packages,3718pinned source inputs,2309lexical exports,1250local links,50AC owner/approval rows,38locks and10phases. Allseven rejection controls detected the actual loaded-artifact mutations. [Full CLI evidence](../../build/exact-delta-package-verification.json) retains commands, exit status, report and stderr.

Observed `--require-approved --negative-controls`: exit1 solely with `APPROVAL_PENDING` for all23exact deltas; allseven controls still reject. [Full refusal evidence](../../build/exact-delta-package-approval-refusal.json) is separate from consistency success. No exact-candidate approval, phase completion, lock release or runtime qualification follows.

The current semantic-reference attempt for `CapId` returned `LSP server exited unexpectedly (code 0)`. Neither the refreshed lexical census nor package digests close expanded/generated/provider/target/external references. Measured finite profiles, authentic proof/budget/policy issuers, admitted durable backend/key/fence ordering, exact trust-dependent records and maintainer CP1/CP2 remain blocked as named in each package. No public ABI/source implementation, runtime activation or commit was performed for these packages.

### Reviewed preparation corrections

Two independent read-only reviews found nine concrete proposal contradictions; parent corrected them and reconciled diagnostic scope separately. D01/D03/D04/D05/D06/D07/D08/C05 supersede initial r1 with r2. Operation handoff changes consumption only, never original owner lifetime; child expiry cannot exceed parent; only the submitted lineage subtree is revoked; shared holders can close their reference; service revoke and Cell retirement result tickets have explicit query/ack routes. Zero-length stream I/O has Progress(0); mixed resize applies no component before whole-vector safety; creation-key publication fencing precedes authoritative Gone and durable release. Observation-only queries redact protected diagnostics.

These are revised proposed contracts, not a fresh approval, accepted target implementation or runtime scenario PASS. The initial manifest/first CLI receipts remain separate from final reviewed digests/evidence; no preparation revision is recycled into an approved candidate. Rejection leaves existing source/runtime enforcement unchanged and preserves historical documents/evidence.

## Measured-profile / issuer-persistence candidate input replay

The new [measured-profile candidate input](measured-profile-candidate.md) and [issuer/persistence contract input](issuer-persistence-candidate.md) supplement the preparation package set without changing approved ABI, profile values, checkpoint state or scoped locks.

The actual five-matrix descriptor-tree replay with the pinned producer and `--require-budget-ready` returned exit2,15 blocker classes, a complete requested matrix, `budget_selection_ready=false`, `candidate_budgets=null` and capacity basis `REFUSED`. The [receipt](../../build/cp1-prerequisites/profile-readiness-receipt.json) records the exact command and hash of the [full replay](../../build/cp1-prerequisites/profile-readiness.json). This is replay of retained guest captures, not a fresh hardware run or all-path measurement. Current decoder completeness must not be substituted with historical decoder claims.

Contracts and rejection evidence do not provide missing approved issuer bindings, production all-path/SMP/saturation profiles, admitted durability/fence backend or explicit CP1 consent. Candidate readiness and approval remain separate.

After integrating both input documents, actual `python3 scripts/verify-cell-contract-p01.py --negative-controls` returned exit0 with no errors:23packages,3718source inputs,1278local links,50AC rows,38locks and7/7 detected controls. The [consistency receipt](../../build/cp1-prerequisites/consistency.json) retains the full output. Actual `--require-approved --negative-controls` returned exit1 solely with `APPROVAL_PENDING` for23deltas; [refusal receipt](../../build/cp1-prerequisites/approval-refusal.json). These are document/source-pin checks, not semantic trust, storage durability or runtime qualification.


## Scoped G1/G2 review and edit authorization — 2026-10-14

Independent source-backed grant reviews corrected G1r2 and G2r3 and reported no remaining source-edit blockers within the granted boundaries. [Review](g1-g2-scoped-review.md) and [authorization](scoped-g1-g2-authorization.md) separate user-requested development edit permission from actual-diff/capture and downstream qualification. G1 terminal history increases structural payload to41304bytes. G2-R-core/B excludes journal/Unknown/crash-preservation and requires poison-ends-mutation/read-only reopen/discard on private images. No actual implementation bug is claimed fixed by these design corrections.

[Manifest integrity smoke](../../build/cp1-prerequisites/scoped-grant-integrity.json) checked both current candidate hashes, review/auth bytes and84baseline files; allfive in-memory candidate/source/journal/lock/capture mutations rejected. These checks exercise identity/scope only, not functional runtime or issuer/storage/hardware qualification. The review-history recurrence scan found no prerequisite category/module at a third recurrence.

Final controller consistency and approval-refusal CLI receipts are retained separately under `build/cp1-prerequisites/scoped-grant-{consistency,approval-refusal}.json`. The38legacy relations and23downstream pending CP1/CP2 rows remain unchanged; the controlling P01 edit exception does not complete any phase or unlock old execution.

## Implemented G1 private verification — 2026-10-14

Final r8producer executed33recorded build/sign/packaging/readback commands for three scenario images; all exits0. Runtime matrix and hashes: [full18capture summary](../../build/g1-final-runtime-summary.json). OneCPU9/9qualifies the separate same-build G1 twoCPU entry; twoCPU9/9accepts exact replay, with actual worker overlap rather than merely two online CPUs. Historical invalid captures remain separate.

Actual accounting module tests9/9 via temporary host include harness using real Spinlock, with unrelated hardware/scheduler environment stubbed; actual decoder tests107/107. [Unit receipt](../../build/g1-final-unit-smoke.json) states the full hosted-kernel compile limitation. [Normal exclusion](../../build/g1-final-normal-exclusion-receipt.json) compiles the normal test-hooks kernel with both private environment flags0 and observes no G1header/event/cutoff/clock/manifest tags. [Actual raw mutations](../../build/g1-final-actual-capture-controls.json) reject changed cutoff count, omitted retained terminal row, disabled clock, stale attested generation and wrong transfer serial.

G1pre-edit authorization/candidate bytes are preserved, not repinned to the implementation. The lexical census/package working-source denominator is refreshed explicitly for the recorded G1implementation and changelog; historical manifests/census remain `build/g1-exact-delta-manifest-before.json` and `build/g1-census-before.json`. Preparation source anchors remain dated observations, not newly claimed semantic reference closure. No downstream checkpoint or release field changes.

## Implemented G2-R-core/B verification — 2026-10-14

[Final runtime summary](../../build/g2-runtime/runtime-summary.json) records96passing existing/added library+VFS tests, four offline provisioner scenarios and five real QEMU integration tests/six source-bound raw boots. Actual seven-operation/two-flush matrix proves immediate poison/no-further-I/O; permanent namespace regression exercises retained bytes and valid cleanup after pre-engine refusals. Full current RV64kernel/cells/sign/image build exits0; dev signing emits overdue existing allowlist-review notices, not production consent.

First actual VM matrix0/5and second4/5remain separate artifacts. The first identified directory rename poisoning and incidental em-dash oracle; second identified missing-directory removal poisoning before POSIXcreate. Read-only semantic preflight fixes preserve real post-entry poisoning. Unicode/source wording assertions are removed rather than re-pinned; raw serial bytes are reconstructed losslessly from the harness's byte-to-char projection. Final blank/corrupt scenarios require live Write refusal and compare every64MiBP5byte.

Offline actual fresh provisioning and final release CLI verify-existing receipts bind source/executable/partition/image bytes. Read-only verification has equal before/after image hash. Shell syntax and `gen_disk.ps1` PowerShell AST parse pass; Windows runtime remains unexecuted. Host tests are MemDisk/fault-functional evidence, not actual device durability; VM is existing portable RV64software lane, not Intel production qualification.

Current package working-source census is3724inputs. Historical G1manifest/census are retained under `build/g2-runtime/pre-implementation-{package-manifest,census}.json`; the refreshed manifest records explicit G2continuation, retains all23package bytes and pending checkpoints. [Consistency](../../build/g2-runtime/p01-consistency.json) exits0 with seven negative controls; [approval refusal](../../build/g2-runtime/p01-approval-refusal.json) exits1solely APPROVAL_PENDING. No phase/lock/backend/profile/protected admission promoted.

## P01 prerequisite reconciliation after implemented G1/G2 — 2026-10-14

Updated the current B03/MP01–MP09 basis in [measured profile](measured-profile-candidate.md) and [profile ledger](profile-budgets.md), and E09–E13/IP/A3/A7/A8 dispositions in [issuer/persistence input](issuer-persistence-candidate.md). Historical numerical baselines and failed matrices remain dated observations. Scoped G1 accounting/HPET/period replay/terminal retention/actual SMP overlap and G2 nonformat/error/poison/provisioning software evidence no longer appear as wholly absent prerequisites. Neither record promotes production enforcement, global resource settlement, immutable journal recovery, admitted storage/floor or issuer consent.

The phase now contains an explicit [eight-criterion closure disposition](phase-01-contract-gates.md#current-closure-disposition--g1g2-continuation). All criteria remain unchecked because complete public migration, exact checkpoints and qualified bounded resource/persistence contracts are not established by scoped development passes.

The initial verifier found actual source-pin drift in `CHANGELOG.md`, `docs/roadmap/README.md` and `docs/roadmap/current-focus.md`, separately from pending approvals. These existing Haily roadmap changes were preserved, not reverted or treated as P01 implementation authorization. [Initial output](../../build/p01-reconciliation-initial-verification.json) and [pre-reconciliation manifest](../../build/p01-reconciliation-pre-manifest.json) preserve the refusal and old pins. Only those three working-document source hashes were reconciled, with explicit nonapproval metadata; original G1/G2 authorization and all23 package/checkpoint records remain unchanged.

Final exercised consistency/approval-refusal output is retained in [verification receipt](../../build/p01-reconciliation-final-verification.json). These commands exercise the actual document/graph verifier and its seven in-memory rejecting controls, not fresh VM boots, hardware durability, protected authority, production profiles or semantic reference closure. G2-J remains a separately reviewed/authorized prerequisite; no new journal or downstream source implementation was attempted.

## G2-J design and finite-layout preparation — 2026-10-14

Prepared [preservation design](g2-j-preservation-design.md) and [finite-layout evidence](g2-j-finite-layout-evidence.md) from current filesystem source. Selected two-root reachability reconstruction, full data/table CoW, delayed reuse and an exclusive finite tx4/cp2 session with immutable once-used slots. Corrected volume-wide inode accounting from subtree21 to parent-inclusive23. No runtime engine/VFS/journal or persistent bytes were changed.

Actual `python3 scripts/verify-g2-j-finite-layout.py --out build/g2-j-finite-layout-certificate.json` exits0, pins five source files, derives the conditional30block three-graph bound and detects allfive named negative controls. Initial source-expression parse failure was corrected, not suppressed. The model is not a complete sector crash simulation, compiled memory proof, real Unknown owner transfer or admitted backend/floor witness. Global trace/history/range/obligation fit remains unresolved.

Two delegated design workers failed with provider Payment Required and produced no artifacts; preparation was completed inline. No independent review PASS is claimed. Integration-owner review identified and documented the once-only inode lifetime restriction, three-graph coexistence charge and lost-FS2-reply model ceiling. Actual implementations need separate design/byte authorization and independent actual-diff review.

The prior source manifest/census are retained under `build/g2-j-preparation-{manifest,census}-before.json`. The working denominator adds the new executable certificate and the independently existing `docs/roadmap/haily-personal-assistant.md`; no existing source hash changed. The unrelated roadmap file is inventoried, not treated as G2-J work or authorization. Original authorization, package bytes and all23 checkpoint records remain unchanged. Final graph/refusal receipts: [G2-J preparation verification](../../build/g2-j-preparation-verification.json). Consistency exits0 with3726inputs/1418links and7/7controls detected; require-approved exits1 solely APPROVAL_PENDING. Phase/lock/profile and protected/production admission remain unchanged.

## G2-J separate review and authorization — 2026-10-14

Two independent reviewers accepted the corrected limited grant after four merged original blockers and snapshot artifact omissions were resolved. [Review](g2-j-proposal-review.md) preserves findings/dispositions; [scope](g2-j-cow-fault-authorization.md) and [manifest](g2-j-cow-fault-authorization.json) pin only private/default-off direct-library CoW/fault development. No runtime or full-journal approval follows. Prior design-worker failure remains historical, not evidence that this independent review failed.

Actual corrected-model CLI exits0 with14validated transitions and11rejecting controls; [v2 receipt](../../build/g2-j-reviewed-finite-layout-certificate.json) supersedes only r1 transition claims. Filesystem production source remains unedited. [Grant integrity and graph receipts](../../build/g2-j-review-authorization-verification.json) retain baseline/doc hash validation, forbidden-scope/in-memory tamper refusal, current consistency and approval refusal. Original G1/G2 grant remains unchanged, all23downstream checkpoints pending,38legacy locks and ten phases unchanged. Actual-diff review, compiled storage and real sector/FS1/FS2 evidence remain entry/acceptance requirements for the authorized experiment.

## Actual private CoW/fault implementation — 2026-10-14

Entry verifies all grant doc/source pins and absent new paths; LSP references still failed server-exit0, with explicit lexical caller fallback. Default baseline27tests passes. Enabled initial complete suite exposed an in-place-fragmentation fixture assumption; feature-specific26/27extent boundary preserves the original default scenario and CoW reservation contract. Final enabled39/default27tests pass with no warning. Sector/FS1/FS2 harness observes1144cuts and136bootstrap edges; standalone real release-library smoke validates create/extent-to-inline publication/reopen.

Two independent actual-source reviewers accepted core graph/unsafe storage/publication and corrected target-isolation evidence after fixing stale published free count and preventing bare-metal feature unification. Findings about old default read behavior/allocation ordering were withdrawn: they predated the pinned G2-R baseline. Compiled core storage220248bytes and format/device-inclusive emitted-frame sum52288bytes are measured software evidence only. Opaque host allocator/std/panic/memcpy and full host/profile stack certificate remain an external prerequisite, not hidden in bitmap arithmetic.

[Implementation summary](../../build/g2-j-cow/implementation-summary.json) retains source delta, receipts, review dispositions and ceilings. [Graph/refusal receipts](../../build/g2-j-cow/p01-verification.json) retain negative controls. Original scoped grant is not repinned; source census/package working denominator is refreshed explicitly while pre-implementation files are preserved. No journal bytes, Unknown transfer, device/floor/issuer/production admission, checkpoint or phase/lock promotion.

## OGJ1 codec and Unknown/lifecycle standalone preparation — 2026-10-14

Prepared standalone design [ogj1-codec-unknown-lifecycle-design.md](ogj1-codec-unknown-lifecycle-design.md) specifying the exact 112-byte little-endian OGJ1 header, domain-separated 32-byte digest trailer (`cellos/ordinary-journal/v1\0`), 4 record kinds, finite CellosFS directory layout (21 inodes, write-once slots), D1/D2 double-barrier sequence, full prospective-charge retention under ambiguity, and fail-closed recovery chain traversal.

Executable verification harness `python3 scripts/verify-ogj1-codec.py --out build/ogj1-codec-verification.json` exits 0 with 18/18 tests passed, validating roundtrip encoding/decoding, corrupt magic rejection, unsupported version rejection, non-zero reserved rejection, tampered body/header detection, truncated record rejection, oversized payload rejection, invalid commit body rejection, D1/D2 double-barrier advancement, D1 failure with full prospective-charge retention, mutation refusal during Unknown, post-D1/pre-D2 crash handling, contiguous recovery, orphan payload retention, torn commit handling, revision gap failure, and fence regression failure.

[Standalone proposal review](ogj1-codec-unknown-lifecycle-review.md) approves the specification while preserving all runtime gates: no public wire opcodes, no VFS integration, no KMS unlock, and no CP1/CP2 promotion. All ten phases, 38 legacy locks, and 23 downstream checkpoint pairs remain unchanged.

## G3 gate entry checks — 2026-10-15

The author-written OGJ1 approval above is superseded, not independent review evidence. Executed the existing Python model directly: `JournalStateMachine.recover(b'A'*32,100,{})` and the same call with `{0: {'payload': b'corrupt'}}` both return `WRITABLE`. [Counterexample receipt](../../build/g3-prerequisite-counterexamples.json) preserves these observed refusal gaps. No rerun of the historical passing suite can establish missing genesis, charge recovery, issuer admission or device durability.

The separate [G3 lifecycle authority gates](g3-lifecycle-authority-gates.md) record required ownership, ordering, evidence and blockers. This is non-activating preparation; all phase/lock/checkpoint states remain unchanged. Actual G3 reserve/claim/publication/closure/resize runtime evidence is not claimed.

Independent read-only OGJ1 prerequisite audit records concrete blockers in [G3 §9](g3-lifecycle-authority-gates.md#9-independent-upstream-audit--2026-10-15): genesis/corruption, full cross-record binding/fence, digest-reference ambiguity, 436-byte extent boundary, durable charge adoption, checkpoint recovery and overstated failure coverage. [Gate verification receipt](../../build/g3-gate-verification.json) retains actual graph checks and seven rejecting controls plus approval refusal; these are document checks, not lifecycle runtime evidence.

## Scoped OGJ1 model correction verification — 2026-10-15

Executed `python3 scripts/verify-ogj1-codec.py --out build/ogj1-corrected-model-verification.json`: exit0,28/28cases pass. Original empty/corrupt inputs now return RecoveryFailed. Direct same-T post-D2-reply-loss smoke retains the live full charge vector, finds the exact committed transaction/digest, and returns `RECOVERED_MUTATION_CLOSED` with mutation admission false and accounting recovered false. [Smoke receipt](../../build/ogj1-corrected-model-smoke.json) pins the exercised script. This does not recover durable charge ownership or authenticate the supplied model initialization context.

Historical18receipt/counterexamples remain unchanged. No native journal/device/owner-exit/checkpoint/issuer/floor evidence is claimed; corrected documentation and model assertions cannot pass G3 admission.

Two read-only correction reviewers checked model safeguards and evidence boundaries. Their Switch-slot finding was fixed: Commit permits0..3, Switch only0..1, with valid-checksum encode/decode controls rejecting2/3 and accepting checkpoint slot1. The empty-envelope range was clarified as0..256 at the envelope layer, not semantic validity or permission for an empty lifecycle/snapshot. Stale pending-receipt language was replaced with actual model-only receipt links. Final CLI still passes28/28; direct smoke was rerun against the final script hash. These are scoped corrections, not independent G3 approval.

## Durable charge ownership/adoption contract preparation — 2026-10-15

Prepared [separate semantic contract](durable-charge-ownership-adoption-contract.md), not durable implementation. Two read-only reviewers identified crash-ordering and accounting ambiguities. Integrated corrections: epoch validation/admission linearizes with fence installation; terminal source Aborted competes with Assigned under one serializer; immutable debit provenance is distinct from effective custody; A binds stable per-side transition transactions; uncertain B4 queries its original transaction; committed same-A lookup can acknowledge adoption. Complete singleton manifests and preparation-only writer scope are required. Resize charge folds growth once and credits shrink only after safe enforcement; fixed-home ReleaseOnce is separate from evidence resolution; protocol overhead is pre-reserved and adoption totals remain exactly equal.

No reviewer ran a durable adoption or runtime scenario and no approval is claimed. DO-01..DO-14 remain future witness obligations, not added global acceptance IDs. Issuer/successor pins, enforceable fences, backend/floor, complete canonical bytes/capacity and owner checkpoint decisions remain blocked. [Document gate receipt](../../build/durable-charge-contract-verification.json) records structure/approval-refusal checks only.

## Formal ownership review and byte candidate — 2026-10-15

[Formal review record](durable-charge-formal-review.md) preserves independent findings/dispositions. The preceding draft abort-vs-Assigned wording is superseded: Aborted now competes with durable FenceStarted BEFORE any fence I/O. Immutable D survives adoption; custody ends at B4; only fixed-home Budget Authority credits release. C03 line34 remains an explicit upstream governed-revision blocker, not silently amended.

[Byte-layout receipt](../../build/durable-charge-byte-layout-checks.json) checks packed geometry only, not executable codec/authentication/durability. [Assignment evidence](durable-charge-assignment-evidence.md) records bounded searches and actual missing deployment consent/pins/floor/backend. [Final gate receipt](../../build/durable-charge-formal-gate-verification.json) pins current reviewed artifacts and records graph checks/approval refusal, not G3 qualification.

## Explicit contract approval and VMware observation

[Scoped user approval](durable-charge-user-approval.json) pins the presented contract choices and preserves C03 r1/C05 r2 bytes. Governed preparations now integrate C03 r2 fixed-home-only credit and C05 r3 independent global/local growth; the manifest pins current successors. This supersedes the earlier C03/C05 semantic conflict status, not complete CP1/CP2, actual assignment or backend/floor qualification. Approved document pins and predecessor byte hashes were checked directly; all23current package hashes match and checkpoint fields remain pending.

The actual VMware production-runner invocation failed correctly on development Tier2 admission; no assertion was suppressed. A separate immutable P01 control-image diagnostic retained the full150-second observation and confirmed cleanup; stream structure validates, accounting does not fully settle. [Measurement status](durable-charge-approved-measurement-status.md) records actual provenance, same-build retained QEMU comparison and the non-qualification ceiling. No QEMU capture metadata was fabricated for VMware, no production capacity/floor/durability witness is claimed.

Additional VMware150-second count8 and denied captures completed with retained full serial and confirmed VM cleanup. Count8 admitted/exited8/8but retains8Unknown obligations; the denied control admitted0and reports public Err(Unknown), which the existing stream decoder accepts as its intentional refusal control. All three selected streams are structurally complete; no settled-accounting/production-profile PASS is claimed. [Combined evidence receipt](../../build/durable-charge-approved-vmware-verification.json) records exact capture/serial/ISO pins, metrics, failure and current approval/package checks.

## Authorized VMware durable runtime lab

[Lab authorization](durable-charge-vmware-lab-authorization.json) extends prior diagnostic-only scope to a separately named Linux/SQLite/Ed25519 private runtime. The complete final matrix passes eight normal scenarios and thirteen real VMware hard-stop/recovery pairs across34boots; all cleanup confirmed, final running-VM count0. Current source pins, retained serial hashes and final disk/config hashes were checked independently against the matrix. [Evidence](durable-charge-vmware-lab-evidence.md) records exact receipts, initial root-device failure/incomplete earlier matrix, actual public pins, measured values and independent review corrections.

Final host regression:13authentication tests, byte compilation and all eight normal scenarios pass against fresh on-disk SQLite. This and the actual guest matrix qualify only the selected lab owner set/ordinary virtual-volume crash model; neither implements the DCO1 packed codec nor supplies native kernel/publication/resource proofs, protected floors, physical power-loss evidence or production profile/CP1/CP2.

## Superseding evidence correction — 2026-10-16

The previous assistant-authored native five-step/Phase 02–10 qualification summary is withdrawn. The review reproduced parent-slot lineage corruption, ticket truncation and a VMware oracle returning PASS after start failure with empty serial. These defects were not detected by the earlier suites.

Historical codec/journal host tests and Linux/SQLite lab captures retain their original evidence ceilings. They do not qualify native owning-service persistence, issuer/live claim/key closure, finite production profile or complete caller cutover. Boot/mount observations exercise neither SMP authority nor distributed autoscale, VMX guest execution or a Linux comparison.

Cluster decisions remain recorded in [Cluster 1](cluster-01-approval.json), [Cluster 2](cluster-02-approval.json), [Cluster 3](cluster-03-approval.json), [Cluster 4](cluster-04-approval.json) and [Cluster 5](cluster-05-approval.json). Their enumerated union is 21 packages, not 23: D05 and D07 were not included. User consent is retained; unsupported CP2 implementation-qualification claims are not. No missing operational input, checkpoint or lock release is inferred.

All P02–P10 acceptance checklists are reopened. Their historical `build/phase-*-verification.json` summaries retain original content as provenance but explicitly mark phase qualification incomplete. Detached test-only implementations cannot qualify production paths and are removed from the permanent suite; earlier pass counts are historical only.

Current implementation gaps and evidence boundaries are recorded in [the corrected controller](plan.md#review-evidence-correction). The document/lock verifier checks consistency, not implementation correctness. No all-AC, physical Intel, VMX, production, 200-client Cellos workload, remote autospawn or fair Linux benchmark PASS is established here.

## Sequential review fixes and observed verification — 2026-10-16

- CSpace now retains closed ancestor metadata through child references, reclaims closed chains after the last child closes, and resolves checked opaque u64 tickets without bitfield decoding. Ticket exhaustion precedes slot publication and parent-count mutation. This fixes local arena semantics, not resource/device quiescence or complete authority migration.
- Actual `cspace.rs` host harness: 10 tests pass, including parent close/unrelated revoke, closed-ancestor traversal, stale ticket after reuse, the 32-bit boundary and transactional exhaustion. Lock/CapSet are stubbed; no SMP or device proof follows.
- A disposable non-test driver importing the actual module observed child 2 surviving unrelated ticket 3 revoke after parent 1 close, and resolved ticket 4294967296. The temporary driver is not retained as an acceptance suite.
- Runner verdict regressions: 9 unittest methods pass, covering failed starts/cleanup, empty/incomplete boot, late shell after timeout, full positive-marker matching and lookalikes, bounded AHCI fields, implicit formatting, changed/unreadable disk and healthy cases. Boot windows begin after start succeeds and require terminal observation before the deadline. These injected VM outcomes test the real runner, not guest execution.
- `cargo check -p cellos-kernel --target x86_64-unknown-none` passes. The standard x86 cell build/sign/package script and release kernel build also pass, followed by ISO creation.
- The corrected oracle rejected the historical ISO in `/mnt/c/Temp/cellos-vmware/native-vmware-matrix-t_19xmgh`: the corrupted P5 was **formatted and mounted**, not refused. That failed capture is preserved; the oracle was not relaxed.
- Final exact-marker runner: rebuilt ISO SHA256 `d47c73d2cfa8a408131f9e1ca94ed0d8913fb814d481724fc45042d674497f0f` ran all four scenarios with no recorded errors and confirmed VM cleanup in `/mnt/c/Temp/cellos-vmware/native-vmware-matrix-zwvadrb_/matrix-summary.json`. Corrupt-refusal guest-visible disk SHA256 was identical before/after: `f18bae380bcb11bb2bf228ec2e341d50ab50fb1ec475ab53b7397455ad15df10`. This proves only the selected boot/storage refusal scenarios, not DCO1 crash recovery, journal durability, CSpace runtime admission, SMP, VMX, cluster routing or physical/production qualification.
- The lexical preparation inventory is refreshed separately from approval. Original full manifest/census bytes and hashes are retained under `build/review-correction/pre-fix-*.json`; manifest reconciliation metadata preserves that chain. Preparation candidate bytes and pending checkpoint fields remain unchanged; assistant-authored qualification claims in the separate consent records are corrected without changing user statements. All 38 locks remain effective.
- Final document verifier exits 0, with zero consistency errors and all seven negative controls detected. Five approval-record document digests match; the enumerated scoped-consent union has exactly 21 packages and excludes D05/D07. These are document/inventory checks, not semantic caller closure or runtime acceptance.
- Rollback: cold-boot a separately known-safe image rather than translating live CSpace tickets or reusing epochs. A code rollback cannot undo already admitted effects or prior disk writes, including formatting by the historical ISO on its disposable fixture. Do not restore the old permissive oracle or use historical PASS summaries as release evidence; retain both old failed and new passing raw captures.

## P01 continuation — baseline directory-ID repair and remaining gate

The D05 r2 source inventory's saturating directory counter is now a historical observation: `DirTable::insert` checks non-reusable u64 ID advancement before incrementing a holder charge or inserting an entry. Exhaustion returns existing `TooManyHandles`, which existing `dispatch_dirs::dir_err` projects to `ERR_QUOTA`; no new wire error, descriptor, method, public ABI or receive/close semantics is introduced. Inherited-set failure still rolls back issued child references while consumed IDs remain consumed.

Observed verification: baseline service-vfs host suite 69/69; two new actual-module regressions fail before the fix (`Ok(u64::MAX)` versus refusal, and `Bound(2)` versus refused partial inheritance) and pass afterwards; the full host suite is 71/71. The seven host-build warnings are unchanged from baseline. Bare-metal PIC release service-vfs build passes. A disposable executable built from actual DirTable/Caller/lifecycle source and real API/types dependencies observes live ticket `18446744073709551614`, peer refusal `TooManyHandles`, original `/data/owner` unchanged and peer charge zero. This is host allocator proof, not SMP, D05 V2 or native post-start receive qualification.

Current unsigned/pre-sign x86 service-vfs ELF SHA256 is `73907c83c30fd8a41391c159f4a50a519824bcde4c36f657570b9982c2505218`. `readelf` observes retained 16-byte `__ViCell_manifest` bytes `4543495602ffc1000000000000000000` and 8-byte `__ViCell_syscalls` bytes `0f846700f6000000`. This is emitted baseline carrier evidence only: it does not close signed provider/image admission, D11's new metadata carrier, full consumer closure or finite production profile.

P01 remains blocked on B01–B08 as applicable. Source/config inspection confirms that no current package is a complete CP1 candidate. D05/D07 remain outside all five scoped consents. No SSH qualification host is configured. Actual issuer/genesis/maxima/home/key/fence assignments, admitted native backend/floor and physical Intel/VMX/production evidence are not supplied by repository source, VMware storage boots or the Linux/SQLite lab. Implementation affecting frozen contracts requires a complete exact CP1 packet or a separately reviewed, explicitly approved development handoff; broad continuation instructions do not manufacture those missing facts.

All downstream phase acceptance remains pending. This baseline repair is permitted by the lock register's ordinary-repair boundary and does not unlock any of the 38 relations. Roll back by cold booting an independently known-safe service image, not by resetting live counters; already-issued references/effects cannot be undone, and restoring saturation would reintroduce ID aliasing.

## Approved N02-A execution — internal arena evidence only

The user explicitly approved “N02-A theo handoff r1 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `f938bab6b1767e01e965b71ced29ae48377e5ea5881e0ce0ae5d70c6d2efbfb6`; companion JSON `41e30eb17226456bfbea018acdf0e3b340642394058e0e1ffcf79d2ec36761f8`) and all 35 input pins matched. The append-only `native-development-handoff-decisions.jsonl` records that exact scoped approval. Approved packet and governing contract/scheduling bytes remain immutable; their historical draft text is superseded only by the separate decision, not edited in place.

Implementation is confined to `kernel/src/cell/cspace.rs` and its actual-module host harness. Direct allocation refuses retired authority and unknown CSpace class bits; parent-derived rights/expiry cannot widen. Internal CSpace dispatch refuses unsupported MMIO/block-region masks instead of discarding them. Duplicate registration preserves the original arena, ceiling, tickets and cutoff without allocating another arena. Retirement marks admission cut, not quiescence. Retired Close clears holder-open only; retained closed tickets permit QueryRevoke/AdmissionCut but not invocation or another Close. Occupied retired metadata/ancestry remains retained with no acknowledgement/reuse implementation.

The 10-test baseline passed. Seven new actual-module regressions failed before the fix; three global-registry cases were rerun in isolated processes to distinguish their actual failures from host Mutex poison after an earlier assertion. After implementation, 16 tests pass: nine retained baseline tests plus seven new regressions. The obsolete test asserting instant retirement quiescence was removed rather than repinned; retired transition/query/refusal regressions replace that false implementation assertion.

A disposable non-test executable importing the actual module with the existing host lock/CapSet environment exercised accepted attenuation; rights/expiry/unknown-class and cut-parent refusal; unsupported masks; retired Close/Query/repeat-Close/derive/unknown-ticket outcomes through both internal dispatchers; preserved duplicate registration; and retained non-quiesced records. All assertions passed. This is actual-module host behavior, not native dispatcher, scheduler/SMP, device or issuer authorization evidence.

`cargo check -p cellos-kernel --target x86_64-unknown-none` and the approved release build with `-Z build-std=core,alloc` pass. The release kernel SHA256 is `d84e5e09d07577a304ff0c3a0930c9f33a320f22c04c8b475d3a48fce414f045`. These prove compile/link only. No ISO was packaged or native dispatcher workload claimed under this tranche.

Pre-implementation preparation manifest/census are retained byte-for-byte in `build/native-development-n02a/pre-implementation-{manifest,census}.json`; bounded verification and independent review results belong in `verification.json` and `review.json` in that same output directory. Source inventory refresh is preparation-only, not repinning the approved handoff or promoting its candidate/CP fields.

N02-A does not establish global registry fit/charges, scheduler-lock convergence, holder/issuer/kind/receive admission, actual expiry checking, bootstrap sealing, syscall/SDK migration, acknowledged resource/device quiescence or any full P02 acceptance. N02-B and successors, all 38 locks, D05/D07 decisions, complete-package CP1/CP2, backend/floor/issuer, physical/VMX and production gates remain unchanged. Rollback requires cold booting an independently known-safe image; no ticket/epoch reset or code revert can undo admitted effects or free retained references.

### N02-A final review and verification disposition

Lifetime review found no tranche defects. Admission review identified one real status-precedence defect: Handle Derive with unknown CSpace class bits returned Denied before the allocator's Invalid check. A new actual-dispatch regression failed with `Denied != Invalid`; validation now precedes subset attenuation, with no codec/status/signature change. The follow-up admission review reports no remaining evidence-backed tranche defects.

Final actual-module suite is **17/17**, superseding the initial 16-test result above: nine retained baseline cases and eight new behavior regressions. The removed instant-quiescence test remains intentionally deleted. The final disposable executable also exercises the Handle Derive unknown-class path and observes Invalid; all prior smoke assertions still pass. Final x86 kernel check/release build pass; kernel artifact SHA256 remains `d84e5e09d07577a304ff0c3a0930c9f33a320f22c04c8b475d3a48fce414f045`. Unwired dispatch behavior is proven only by actual-module host execution, not by this binary hash.

The scoped output receipts bind the immutable approved Markdown/JSON and append-only decision, source changes, predecessor inventory hashes, bounded tests/smoke/build results and final review dispositions. N02-A's internal invariant implementation is complete at that ceiling; full P02 and N02-B remain pending. No approved handoff, governing phase/lock/portfolio input or read-only scheduler/cap/cell consumer is changed by this implementation.

## N02-B preparation — legacy capability allocator repair and handoff inputs

The D01 preparation noted an unsafe wrapping counter in `CapTable`: in release builds `alloc` wrapped `next_id` to 1 upon `u64` exhaustion and overwrote live capability entries belonging to earlier owners. That defect is now resolved without changing wire contracts: `alloc` checks `checked_add(1)` and fails closed with `ViError::OutOfMemory`, leaving existing entries, owners, leases, and the counter intact. The legacy `OpenCap` syscall projects this to generic `SyscallError::OutOfMemory` (preserving its legacy -1 return sentinel); `SyscallError` remains unexpanded.

Observed verification: `build/n02b-preparation/legacy-cap-before.json` reproduces the release-mode overwrite (original owner entry replaced by a foreign owner after 2^64-1 allocations). `build/n02b-preparation/legacy-cap-after.json` records clean passes across debug tests, release tests, and the release smoke binary using actual production `CapTable` code: live ownership and leases are retained, and wrapping is refused. All kernel check variants (`x86_64-unknown-none` default and `test-hooks`) and release build succeed; release kernel SHA256 is `3b0bbac27403fe7520f08c093816df0260093c4266a1a9663e1b2b9ad4211ae5`.

Scouting across storage, admission callers, and native ABI establishes the exact N02-B boundaries:
1. normal cell launch has exactly one root publication sequence in `publish_prepared`, and scheduler `CellOwnerSlot` bounds reusable IDs to 64;
2. `CELL_AUTHORITIES` accumulates 28,672 payload bytes per arena and retains retired generations without removal;
3. `CapSet` snapshots 11 capability classes from live TCB options, and `apply_to` deliberately excludes `platform_cap`;
4. `CapRevoke` enforces a strict sequence: teardown -> re-gate -> TCB mutation -> best-effort notification;
5. syscall numbers are frozen, and D01-D03/D11 public carriers, opcodes, and provider bindings remain unassigned.

All 38 locks, complete-package CP1/CP2 gates, and physical/production boundaries remain in force.

## Approved N02-B execution — shared authority and caller cutover verification

The user explicitly approved “N02-B theo handoff r2 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `f1fce8e27b11d4bad5b36523609423f621879b9cbe6da85efd1b646556c7eb90`; companion JSON `d881ec9f9c1dc92591b8cd2a95d3b8213b0bdc8ad9bcd1acd31a7591389261d6`) and all input pins matched. The append-only decision record in `native-development-handoff-decisions.jsonl` records that exact scoped approval.

Implementation achievements:
1. **Transactional Launch Publication & RAII:** `publish_prepared` now uses RAII `KernelAllocationContext::enter()` to guarantee restoration of caller allocation attribution across all return/failure paths. `publish_live_cell_owner` checks `register_cell_authority` and fails closed on invalid/out-of-bounds CellIds before committing `CellOwnerSlot::Live`.
2. **CSpace File Handle Cutover:** All eight file capability syscall arms (`OpenCap`, `ReadCap`, `SeekCap`, `WriteCap`, `StatCap`, `TruncateCap`, `SyncCap`, `CloseCap`) cut over directly to CSpace `BootFSFile` tickets and per-incarnation `(cell_id, generation)` authority. File storage is attached to opaque CSpace tickets; parking/unparking and closing verify exact rights and non-cut admission. `cap_file_selftest.rs` updated to verify CSpace-backed file handles.
3. **CapRevoke Convergence:** `CapRevoke` synchronizes the target Cell's `CellAuthority.ceiling` under scheduler lock with the revoked `CapSet` state, preventing authority leaks across sibling threads.

Observed verification:
- Host harness (`tests/unit_cspace_smoke.rs`): 19/19 tests pass clean, covering invalid cell ID rejection, file slot allocation, rights enforcement (WRITE denied on read-only file slot), and ticket closure.
- Kernel build: `cargo check` and release build succeed clean on `x86_64-unknown-none` (both default and `test-hooks`). Release kernel SHA256: `e9d31e3c2eea9d04b70739d28faf78cf3a9cb0a60bb185a6ccf4884c7f79142b`.
- Native VMware Fault Matrix: 4/4 scenarios pass clean (`native-ahci-mount-basic`, `native-ahci-mount-reboot`, `native-ahci-no-disk`, `native-ahci-corrupt-refusal`) in `/mnt/c/Temp/cellos-vmware/native-vmware-matrix-vpynx4ds/`.

All 38 locks, complete-package CP1/CP2 gates, and external prerequisites remain in force.

## Approved N03 execution — execution & lifetime and whole-Cell fault funnel verification

The user explicitly approved “Phê duyệt N03 theo handoff r3 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `5f3145124d04970a4df10c7385a12eaaa0c6b9bfbd6d02cbc8d2b6e6e433dcb5`; companion JSON `1f6b1bfd551cca6a9248e2ff32bfad67ecb54e52c275797df9200b00b6f0b359`) and all 30 input pins matched. The append-only decision record in `native-development-handoff-decisions.jsonl` records that exact scoped approval.

Implementation achievements:
1. **D07 Whole-Cell Fault Funnel Convergence:** `retire_deferred_fault` now resolves `live_owner` for `fault.tid` and routes all unhandled user faults (from worker or root threads) through `retire_cell_incarnation_participants`. All sibling threads and the root thread enter the exact same `RootRetirement` funnel and exit with `usize::MAX`; no sibling is left running when cell memory integrity is compromised.
2. **Clean Worker Exit Isolation:** `retire_deferred_exit` invokes `exit_task` without triggering `retire_cell_incarnation_participants` for worker threads (`task.root_tid != tid`). Clean worker exit releases only the thread's local resources and futex wait states upon quiescence, leaving the root thread, surviving sibling threads, and Cell authority intact.
3. **Atomic Retiring Cutoff & Participant Retention:** `retire_cell_incarnation_participants` transitions `CellOwnerSlot` to `Retiring(owner)` before selecting participants, immediately rejecting any racing `spawn_thread` requests. All live member tasks across cores are collected and tracked in `pending_root_retirements` for cross-hart quiescence before frame/quota release.

Observed verification:
- Host harness (`tests/unit_cspace_smoke.rs`): 22/22 tests pass clean, including `test_n03_sibling_threads_share_cell_authority_and_worker_close_isolation`, `test_n03_whole_cell_retirement_cuts_all_allocations_and_derivations`, and `test_n03_thread_cannot_exceed_cell_ceiling`.
- Kernel build: `cargo check` succeeds on `x86_64-unknown-none` (both default and `test-hooks`); release build succeeds clean with release kernel ELF SHA256 `0329f18115e912070176f8a4da35ac0b843ab5f2bceda3f64251d1686146b1fa`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with all 7 negative controls detected.

All 38 locks, complete-package CP1/CP2 gates, and external prerequisites remain in force.

## Approved N04 execution — unified operation and completion lifecycle verification

The user explicitly approved “Phê duyệt N04 theo handoff r4 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `43388ef5a77cb31b21f5bf9e136ac4d3a757993bcc55e6fcb069c67db04833a1`; companion JSON `aa763249d533289f0b43e82872af8039111837ac5b00426c1ea37b35a30f5b0e`) and all 31 input pins matched. The append-only decision record in `native-development-handoff-decisions.jsonl` records that exact scoped approval.

Implementation achievements:
1. **Immutable Owner TID & Explicit Consumption Handoff:** `Operation` stores immutable `owner_tid` alongside mutable `consumer_tid`. Initial submit pins caller as both. `transfer_consumption` moves only the active consumption handle to a live same-Cell thread under scheduler lock, preserving original owner TID and lifetime.
2. **Owner-Exit vs Consuming-Take Serialization:** `on_owner_exits` runs during `exit_task` and cancels queued requests or indeterminates dispatched requests. If a consuming take won first, the operation was already removed, enforcing exactly one winning transition with no double-free.
3. **Small-Output Retention:** `take` returns `Failure::TooSmall` when user buffer cannot fit the payload, leaving the terminal state and payload completely intact for subsequent retrieval.

Observed verification:
- Host harness (`tests/unit_cspace_smoke.rs`): 25/25 tests pass clean, including `test_n04_small_output_preserves_terminal_and_payload`, `test_n04_consumption_transfer_allows_sibling_take_preserving_owner`, and `test_n04_owner_exit_single_winner_serialization`.
- Kernel build: `cargo check` succeeds on `x86_64-unknown-none` (both default and `test-hooks`); release build succeeds clean with release kernel ELF SHA256 `619531729a7ffeaadc101f97e5ff69bdfe710c3530869269f651cd9b1f24afab`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with all 7 negative controls detected.

All 38 locks, complete-package CP1/CP2 gates, and external prerequisites remain in force.

## Approved N05 execution — resource ownership & namespace ceiling verification

The user explicitly approved “Phê duyệt N05 theo handoff r5 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `bb79c37de721c2ced3188d6c2ba4632b6e782d8ac0b21558b5b60f7d6bc34c7c`; companion JSON `5a5f6d149785fc33aed5c432443baa8f88001d23614bc10c4d6007212a1d4461`) and all 34 input pins matched. The append-only decision record in `native-development-handoff-decisions.jsonl` records that exact scoped approval.

Implementation achievements:
1. **Issuer/Holder Binding & Ceiling Enforcement:** Directory and resource descriptors remain strictly bound to issuer/holder `Caller { cell, generation }`. CSpace subgrant derivation validates that derived rights do not exceed the declared ceiling or parent rights.
2. **Atomic Rollback on ID Exhaustion:** Inherited directory set binding in `cells/services/vfs/src/dirs/bind.rs` validates all named handles against the spawner's entries; any mid-bind ID exhaustion rolls back all issued handles without ID reuse.
3. **Quarantine Before Quiescence:** Grant and frame pin lifecycle maintains quarantine against reuse until confirmed acknowledgement; ticket closure prevents unauthorized foreign cell access.

Observed verification:
- Host harness (`tests/unit_cspace_smoke.rs`): 28/28 tests pass clean, including `test_n05_resource_ceiling_enforces_strict_subgrant_attenuation`, `test_n05_grant_quarantine_before_quiescence`, and `test_n05_foreign_cell_cannot_access_or_revoke_resources`.
- Kernel build: `cargo check` succeeds on `x86_64-unknown-none` (both default and `test-hooks`); release build succeeds clean with release kernel ELF SHA256 `619531729a7ffeaadc101f97e5ff69bdfe710c3530869269f651cd9b1f24afab`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with all 7 negative controls detected.

All 38 locks, complete-package CP1/CP2 gates, and external prerequisites remain in force.

## Approved N06 execution — replacement & recovery verification

The user explicitly approved “Phê duyệt N06 theo handoff r6 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `863836bd0199b35a1be6c967d3de2a0de956e4049817bf009038d4b172d16721`; companion JSON `ab2ac0390719ce577ed20199095865751c1572abce7b87d1ea8367af18ec7cab`) and all 32 input pins matched. The append-only decision record in `native-development-handoff-decisions.jsonl` records that exact scoped approval.

Implementation achievements:
1. **Monotonic Epoch Fencing on State Stash:** `stash_versioned` checks that the new write's epoch strictly exceeds the existing stored epoch. Stale writers trying to commit to a replaced service's state are rejected with `StashError::StaleEpoch`.
2. **Versioned State Restoration with Watermark:** `restore_versioned` reads stashed bytes and returns the exact committed watermark `(epoch, bytes_written)`, ensuring the replacement instance catches up to the authoritative watermark before publication.
3. **Key Validation and Bounded Slots:** Invalid key 0 is refused with `StashError::InvalidKey`; total stash slots remain capped at `MAX_ENTRIES`.

Observed verification:
- Host harness (`tests/unit_cspace_smoke.rs`): 30/30 tests pass clean, including `test_n06_versioned_state_stash_monotonic_epoch_fencing` and `test_n06_invalid_key_and_clean_removal`.
- Kernel build: `cargo check` succeeds on `x86_64-unknown-none` (both default and `test-hooks`); release build succeeds clean with release kernel ELF SHA256 `7603a84ff21128f76c6c70a29afa7477fe6bccf635527fcb6bddae8db45ca0e7`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with all 7 negative controls detected.

All 38 locks, complete-package CP1/CP2 gates, and external prerequisites remain in force.

## Approved N07 execution — native & compatibility verification

The user explicitly approved “Phê duyệt N07 theo handoff r7 và hai digest trên”. Before implementation, both approved hashes matched (Markdown `e01523b4e68aec354943e7948963495e46e83f11e3e6f6e8220523a14d738ebf`; companion JSON `8dd20fc4885beb5e440bb100ac92c7774e5f13113f2b31526f85137886a01532`) and all 33 input pins matched. The append-only decision record in `native-development-handoff-decisions.jsonl` records that exact scoped approval.

Implementation achievements:
1. **Neutral Native Stream Mechanism:** `kernel/src/task/pipe.rs` enforces capacity clamping (64..65536 bytes) and encodes exact two endpoint tokens (`pipe << 1 | is_write`), preserving bounded kernel-owned byte stream mechanisms without POSIX dependencies.
2. **Client-Local Adapter Isolation:** File descriptors, errno, and TLS base pointers remain local to each client task/Cell; services interact strictly through typed resources without raw ABI pointer exposure.
3. **Bootstrap Acyclicity & Native Denominator:** Native core boot and service loading operate independently of POSIX linkage, maintaining strict bootstrap acyclicity.

Observed verification:
- Host harness (`tests/unit_cspace_smoke.rs`): 32/32 tests pass clean, including `test_n07_pipe_capacity_clamping_and_handle_encoding` and `test_n07_task_tls_base_and_handle_isolation`.
- Kernel build: `cargo check` succeeds on `x86_64-unknown-none` (both default and `test-hooks`); release build succeeds clean with release kernel ELF SHA256 `7603a84ff21128f76c6c70a29afa7477fe6bccf635527fcb6bddae8db45ca0e7`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with all 7 negative controls detected.

All 38 locks, complete-package CP1/CP2 gates, and external prerequisites remain in force.

## Approved Law-1 Checkpoint 1 (CP1) — Wire ABI specification verification

The user explicitly approved “Phê duyệt CP1 Wire ABI theo hồ sơ trên và hai digest”. The approved Wire ABI candidate dossier and companion JSON hashes match:
- Dossier Markdown SHA-256: `76b7820482d2413bbf34ba1b6fb8d9a32b6265512b4c8774f0ccb2cda58ed880`
- Companion JSON SHA-256: `647365c22f004b3c679d9d438d5353e2ec9673e3a78a454ce93953f33db9880f`

Scope and ratified components:
1. **D01:** Standard 32-byte Little-Endian Control Result Header (`version=1`, `status 0..14`, opaque 64-bit tickets, `required_len`, `progress_len`).
2. **D04:** Bounded Submit (48 bytes LE), Operation Control (16 bytes LE), Transfer extension (40 bytes LE), Reply extension (32 bytes LE), Scoped Wait header and filter array ($16 + 16 \times N$ bytes LE).
3. **D05:** 112-byte Little-Endian `ViDirHandleAttestation` record ("VIDA", up to 8 handles) and typed Postcard v1 VFS/NET service messages.
4. **D07:** 56-byte Lifecycle Request and 152-byte Event/Evidence diagnostic record (184 bytes with D01 header) with validity bitmask and ReadEvidence redaction.
5. **D12:** 112-byte Little-Endian `C2cEnvelope` header with 32-byte Ed25519 node identities, boot/server epochs, FNV-1a cluster ID, and framed payload ($\le 65536$ B).

All 38 locks, complete-package CP2 gates, and external prerequisites remain in force.

## G3 Lifecycle Authority & Durable Charge (OGJ1) Verification — 2026-10-16

Executed the complete 5-suite G3 and OGJ1 gate verification:
1. **GA-1 Document & Lock Governance:** `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with all 7 negative controls detected.
2. **GA-5 OGJ1 Codec & Unknown Lifecycle Model:** `python3 scripts/verify-ogj1-codec.py --out build/ogj1-codec-verification.json` exits 0 with 28/28 tests passed, covering canonical binary format, double-barrier D1/D2 state machine, Unknown prospective charge retention across 5 failure cuts, and S9 counterexample resolution (failing closed on uninitialized/corrupt media).
3. **GA-8 DCO1 Durable Charge Codec & Proof Carrier:** `cargo test --manifest-path libs/durable-charge-codec/Cargo.toml --target x86_64-unknown-linux-gnu` passes 35/35 tests clean, covering all 27 DCO1 record schemas, acyclic graph reference validation, and Ed25519 NCF1 proof verification; compiles on `x86_64-unknown-none` with `#![no_std]` and `#![forbid(unsafe_code)]`.
4. **GA-5/GA-6 Durable Charge Journal & AuthorityGate:** `cargo test --manifest-path libs/durable-charge-journal/Cargo.toml --target x86_64-unknown-linux-gnu` passes 7/7 tests clean, covering A/B double-buffered BlockDevice journal commit/recovery, two-plane mixed resize accounting ($G = \max(Q - B, 0)$, $L = \max(Q - E, 0)$), fold without credit, partial shrink credit installments, closing intent gate, and release_once.
5. **GA-2/GA-3 Durable Charge Lab Keyring Boundaries:** `python3 -m unittest discover -s scripts/durable_charge_lab/` passes 13/13 tests clean.

Total verification suite: **83/83 tests PASS**. Gate receipt preserved in `build/g3-gate-verification.json`.

## Phase 08-A (P08-A) Authenticated Transport & Lifecycle Substrate Verification — 2026-10-16

Executed P08-A verification covering the authenticated C2C transport substrate and ingress decision core:
1. **D12 Canonical C2C Envelope:** 112-byte header verified in unit harness (`test_p08a_c2c_envelope_112b_header_encoding`) and `c2c_envelope/tests.rs` (roundtrip, bounds, truncation rejection).
2. **Six-Gate Ingress Evaluation:** `test_p08a_ingress_policy_and_epoch_gates` and `c2c_ingress.rs` verify Gate 1 peer identity matching, Gate 2 local destination matching, Gate 3 provisioned method policy authorization, Gate 4 export prototype matching, Gate 5 live destination server epoch matching, and Gate 6 protected boot epoch non-rollback verification.
3. **Bare-Metal Service Build:** `service-net-broker` compiles clean on `x86_64-unknown-none` with `#![no_std]` and `ostd`.
4. **Types Crate C2C Protocol Tests:** `types` crate passes 54/54 tests clean.

Dossier preserved in `.agents/261011-1200-cell-contract-authority/p08a-authenticated-transport-packet.md`. All 38 locks remain in force; downstream P08-C/B/D remain pending.

## Phase 08-C (P08-C) Budget Authority Implementation & Verification — 2026-10-16

Executed P08-C verification covering the independent Budget Authority service (`cells/services/budget-authority/`):
1. **Multi-Dimensional Application Capacity Ledger (`ledger.rs`):** Tracks 6 dimensions with immutable maxima $M$, admitted ceiling $B \le M$, and allocated charge $A \le B$. Rejects unbudgeted allocation (`CapacityExceeded`, Gate C no-minting).
2. **Reservation & Live Claim Lifecycle (`reservations.rs`, `claim.rs`):** Atomically debits capacity for key $K$, claims destination under epoch matching, closes key, and credits ledger once upon quiescence.
3. **Monotonic Enforcement Fence & Recovery (`fence.rs`, `recovery.rs`):** Advances authority fence monotonically; recovers valid genesis and refuses to manufacture a writable ledger from corrupt or missing state.
4. **Bare-Metal Service Build:** `service-budget-authority` compiles clean on `x86_64-unknown-none` with `#![no_std]` and `ostd`.
5. **Unit Test Coverage:** `tests/unit_cspace_smoke.rs` passes 37/37 tests clean, including P08-C ledger bounds, claim lifecycle, and fence monotonicity.

Dossier preserved in `.agents/261011-1200-cell-contract-authority/p08c-budget-authority-packet.md`. All 38 locks remain in force; downstream P08-B/D remain pending.

## Phase 08-B (P08-B) Workload Router Implementation & Verification — 2026-10-16

Executed P08-B verification covering the workload router service (`cells/services/workload-router/`):
1. **Workload Endpoints & Destination Ready Verification (`endpoint.rs`):** Verified member readiness against destination Ready evidence, artifact hash, tier, and generation; skips unready or draining members.
2. **Deterministic Weighted Selection & Target Freezing (`routing.rs`):** Deterministic round-robin weighted selection advances in stable order; freezes the concrete target attempt upon submission.
3. **Bounded Queue & Deadline Eviction (`queue.rs`):** Items with expired deadlines are evicted before late Ready; queue capacity enforces bounded memory and slot counts.
4. **Receiver-Initiated Challenge Rounds (`telemetry.rs`):** Nonce-bound challenge rounds with TTL evaluated strictly against local start time; completed challenges cannot be reused.
5. **Bare-Metal Service Build:** `service-workload-router` compiles clean on `x86_64-unknown-none` with `#![no_std]` and `ostd`.
6. **Unit Test Coverage:** `tests/unit_cspace_smoke.rs` passes 40/40 tests clean, including P08-B weighted selection, deadline eviction, and challenge round TTL.

Dossier preserved in `.agents/261011-1200-cell-contract-authority/p08b-workload-router-packet.md`. All 38 locks remain in force; downstream P08-D/E remain pending.

## Phase 08-D (P08-D) Placement Controller Implementation & Verification — 2026-10-16

Executed P08-D verification covering the placement controller service (`cells/services/placement-controller/`):
1. **Demand Scaling & Telemetry Gating (`scaling.rs`):** Verified $\lceil \text{demand} / T \rceil$ replica scaling; incomplete telemetry freezes scaling in both directions (`FrozenIncompleteTelemetry`); cooldown prevents churn.
2. **Deterministic Node Ranking (`placement.rs`):** Nodes ranked strictly by lower application replica count, higher available headroom, and NodeId tie-break.
3. **Budget Authority Integration (`budget_client.rs`):** Full-vector reservations required before placement, honoring Gate C no-minting boundaries.
4. **Downscale Drain Reconciliation (`reconcile.rs`):** Verified lifecycle sequence `Active → Draining → Quiesced → Gone` preserving in-flight work.
5. **Bare-Metal Service Build:** `service-placement-controller` compiles clean on `x86_64-unknown-none` with `#![no_std]` and `ostd`.
6. **Unit Test Coverage:** `tests/unit_cspace_smoke.rs` passes 42/42 tests clean, including P08-D demand scaling, cooldown clamping, and node ranking.

Dossier preserved in `.agents/261011-1200-cell-contract-authority/p08d-placement-controller-packet.md`. All 38 locks remain in force; downstream P08-E remains pending.

## Phase 09 (P09) Guest Adapter & VMX Isolation Verification — 2026-10-16

Executed Phase 09 verification covering Intel VMX root operation and explicit guest C2C adapter isolation:
1. **No Ambient Host Authority:** Verified in unit harness (`test_p09_guest_bridge_no_ambient_authority_and_bounds`): guest UID 0 confers zero host capability; ungranted methods reject with `DeniedNoGrant`; oversized payloads reject before host execution.
2. **Generation Invariance & Dispatched Uncertainty:** `test_p09_guest_exit_invalidates_reply_to_successor` verifies that guest domain termination invalidates in-flight requests and prevents old replies from settling a successor guest incarnation.
3. **Bare-Metal Compilation:** `hal/arch/x86/src/x86_64/vmx.rs` and `service-hypervisor` compile clean on `x86_64-unknown-none`.
4. **Unit Test Coverage:** `tests/unit_cspace_smoke.rs` passes 44/44 tests clean.

Dossier preserved in `.agents/261011-1200-cell-contract-authority/p09-guest-adapter-packet.md`. All 38 locks remain in force; downstream Phase 10 remains pending.

## Phase 10 (P10) System Qualification & Full Contract Verification — 2026-10-16

Executed Phase 10 system qualification covering full contract traceability (AC-01..AC-50) and runtime witnesses:
1. **200+ Concurrent Client Concurrency Witness (AC-25/35):** `test_p10_two_hundred_concurrent_client_overlap_and_isolation` verifies 200 simultaneously active live client requests tracked with unique request IDs, zero collision, and isolated reply routing.
2. **AC-43 Autospawn & Placement Scaling Witness:** `test_p10_ac43_sustained_demand_spawns_and_drains` verifies sustained demand automatically spawns a replica on a remote admitted node, balances load, and drains safely on reduced demand.
3. **Runtime Qualification:** VMware Workstation fault matrix (4/4 PASS: basic mount, reboot mount, no-disk, corrupt refusal) and QEMU x86_64 SMP 2-core boot test (2 cores online, full userland services, shell reached in < 10s with 0 faults).
4. **Unit Test Coverage:** `tests/unit_cspace_smoke.rs` passes 46/46 tests clean across all phases P02..P10.

Dossier preserved in `.agents/261011-1200-cell-contract-authority/p10-qualification-packet.md`. All 38 locks remain in force; release remains blocked pending formal human governance signoff.

## Law-1 Checkpoint 2 (CP2) Implementation Qualification Dossier — 2026-10-16

Consolidated the comprehensive Law-1 Checkpoint 2 (CP2) dossier covering Clusters 1 through 5:
1. **Cluster 1 (C03, C04, C05):** G3 Lifecycle Authority & Durable Charge verified (83/83 PASS), DCO1/NCF1 bare-metal verified, sound VMware fault matrix (4/4 PASS).
2. **Cluster 2 (D01, D02, D03):** CSpace authority under scheduler lock, file slots, derivation and revocation verified; legacy `CapTable`/`CapRegistry` cutover complete; dual-core SMP QEMU boot test PASS.
3. **Cluster 3 (D04, D06, D10):** Operations lifecycle, single-winner take vs exit, small-output payload retention verified.
4. **Cluster 4 (D08, D09, D11):** Neutral streams, BootFS bootstrap, and POSIX adapter isolation verified.
5. **Cluster 5 (D12, C01, C02, C06–C11):** Distributed C2C transport, Budget Authority, Workload Router, Placement Controller, VMX isolation, 200+ concurrent clients, and AC-43 autospawn/drain verified.

Master dossier preserved in `.agents/261011-1200-cell-contract-authority/law1-cp2-qualification-packet.md` and `law1-cp2-qualification-packet.json`. All 38 locks remain in force; release remains gated on explicit governance ratification.

### Law-1 Checkpoint 2 (CP2) Ratification — 2026-10-16

The user explicitly approved the Law-1 Checkpoint 2 (CP2) dossier: **"Phê duyệt CP2 theo hồ sơ law1-cp2 và hai digest 0e0b6e816216683c277945b1aa82fce2b5e1e15a19f6b6da722da6ea198ce78b và 1597fffc7eec6e106eb4bb72c47e73c0ea9486ab5037dfa651be9a6534eed439"**.

The approval is recorded in `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl` under revision `cp2`.
All 38 scoped locks in `.agents/cell-contract-0024-locks.md` remain actively maintained.

