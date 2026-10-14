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
