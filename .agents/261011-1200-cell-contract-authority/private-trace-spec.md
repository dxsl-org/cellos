# P01 — Private x86 measurement trace specification

Status: design only; not implemented, benchmarked or ABI-approved. This is a disposable test-hooks measurement format for the selected x86_64 workload, not a public syscall, persistent application schema or remote attestation protocol. [Resource contract](resource-attribution-contract.md) and [owner review](instrumentation-owner-review.md) define meaning and insertion constraints.

## Scope and capture identity

One isolated boot captures one driver plus at most8root-only parked children, signed artifacts selected by exact measurement config. No threads, grants/DMA/shared regions, replacements, restarts or capacity-to-OOM sweep. Boot selftests/other services are outside attribution; their global counter drift remains visible and is not subtracted silently.

Host creates a unique capture ID before VM launch, stores it with complete command/image/config digests and raw stream. Record sequence identifies only the joined local capture. A reused ID, multiple streams or ambiguous boot boundary invalidates capture; the ID is not kernel-attested freshness. No node identity is fabricated for a standalone VM: local scope is capture+local-kernel; cluster node binding remains absent.

Arm before the real BootstrapInit's ready push, with explicit private source marker and actual owner/digest binding, per [source-bound implementation entry](instrumentation-implementation-entry.md). This supersedes configured-ingress arm wording: the kernel cannot observe a userspace CellSpec callsite without a new interface. Driver selection uses the bound init parent and expected path/normal verified digest, not task name. Logical attempts are created before fallible validation; unclassifiable selected-parent input invalidates capture, never becomes an inferred unrelated success. Existing ELF→raw-path fallback keeps separate attempt IDs. Only verified publication establishes driver identity; children follow actual parent identity. Known unrelated publications are excluded from scoped rows, not global drift. Boot fixtures have no BootstrapInit marker. Observation never changes runtime admission/results.

State machine: Idle→Armed→Closing→Closed; any invalid flag remains sticky. No implicit rearm/reset in the same boot. Closing starts after final snapshot and requested driver exit; retain late scoped cleanup observations until driver terminal/cleanup outcome or observation bound. This is not a production lifecycle state machine. Closing never blocks a producer or frees unknown resources. A timeout emits incomplete outcome when possible and keeps unknown accounting; trace loss is independently recorded in host evidence.

## Record layout — private version1, little endian

Every record is160bytes; fields are explicit fixed-width serialization, not a transmuted Rust struct. Reserved bytes must be zero. Zero means absent only for fields whose event does not carry that fact; absence in a required identity is invalid. IDs do not wrap; exhaustion invalidates capture. Raw kernel pointers/physical addresses are not printed as object IDs.

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 8 | record sequence, starting1 |
| 8 | 2 | event kind |
| 10 | 2 | record validity/presence flags |
| 12 | 2 | logical CPU of observation; not proof of workload execution |
| 14 | 1 | launch route: absent / Elf / Path / Pinned |
| 15 | 1 | source kind: absent / caller ELF bytes / kernel path resolution |
| 16 | 8 | attempt ID, if applicable |
| 24 | 8 | subject TID |
| 32 | 8 | root TID |
| 40 | 8 | CellId |
| 48 | 8 | incarnation generation |
| 56 | 8 | parent subject TID |
| 64 | 8 | parent CellId |
| 72 | 8 | parent generation |
| 80 | 8 | private object/component/snapshot key |
| 88 | 2 | component or metric group |
| 90 | 2 | result category |
| 92 | 4 | detail/presence bitmap; event-specific meaning |
| 96 | 32 | four unsigned64values, interpreted by event schema |
| 128 | 32 | actual ELF digest for published artifact; zero on other events |

Event kinds: CaptureArm=1, PreBoundary=2, LaunchIntent=3, LaunchResult=4, TaskTerminal=5, OwnerSlotResult=6, QuotaReleaseResult=7, ComponentStart=8, ComponentFinal=9, SnapshotGroup=10, Closing=11, CaptureFinal=12. Result categories: success=1, denied=2, unpublished rollback=3, pending=4, retained=5, unknown=6, incomplete=7. Event-specific reason codes remain existing error/source classifications, not invented replacements for public errors. Full private serializer/parser diff must enumerate concrete code mappings before implementation; unknown event/result invalidates decoding rather than silently passing.

LaunchResult success doubles as publication record; emitted after committed task/owner/routes metadata and before push_ready. Intent carries parent/attempt/source but no invented child identity. Failure may have no child identity and never counts as an admitted child. Root/worker role is a presence/detail bit; the selected scope accepts roots only. Artifact digest is computed/staged outside publication critical section; Elf path remains a hint. Expected paths/config labels stay in host manifest, joined by byte digest and exact route, not variable-length strings in records.

Snapshots carry group index0..3 and four metrics each. The record key identifies stage sample; presence bitmap distinguishes absent metric from zero. No metric is derived from a missing identity/counter. Global metrics have no fabricated Cell owner. ComponentFinal retains subject incarnation even after task removal; object key is a nonrepeating private token assigned before provenance loss, not a late pointer lookup.

Presence flags bits0..8are subject/owner/parent/attempt/digest/worker-role/global-metric/snapshot-sidecar/component-counter presence respectively; bits9..15must be zero. Owner means root+CellId+generation together. A successful publication requires subject/owner/attempt/digest; root-only scope rejects worker-role. SnapshotGroup requires global/scoped metric classification and sidecar presence. Its detail low four bits indicate present values, all other detail bits zero. Launch failure component identifies error namespace (1syscall,2ViError,3observer); value0retains raw source error representation, never a remapped public result. Attempt intent has no child subject/owner/digest flags; parent identity is required unless explicit boot-root ingress. These constraints are private decoding requirements, not new syscall encodings.

## Metadata propagation and producer safety

Carry private scope/attempt/route/source information from kernel syscall ingress through governed request/prepared publication state, with BootstrapInit marker supplied internally by its real boot source. No userspace configured-entry token crosses the ABI and no metadata authorizes an operation. Preserve actual parent tuple while its owner is locked/validated. Save subject/root/incarnation in copy-only teardown metadata before dropping task or queuing TID-only cleanup. Exact producer sites are in the owner review; source paths remain unchanged until implementation review.

Preallocated append storage:256records,160bytes each (40960bytes), plus256one-byte readiness flags. Writers reserve unique slots with a bounded nonwrapping atomic index. Write payload once, then release-publish readiness; consumer acquire-checks before reading. Index reservation order is **not physical completion time or cross-lock causal order**. An unready slot cannot be exported as zero-filled valid data. No wrap, overwrite, allocator call, formatting, trace lock, wait or retry-until-success in allocator/scheduler critical sections. Exhausted reservation sets sticky invalid state and returns; runtime operation continues unchanged.

This first design targets x86_64 lock-free native scalar atomics only. It does not claim RV32 AtomicU64 support or portable lock-freedom. Actual Rust interior-mutation/atomic memory-ordering and interrupt-reentrancy implementation need diff review. Observation flags/counters use atomics compatible with this target; no hidden mutex fallback in producer paths. Host watchdog/collector bound does not block kernel writers.

Drain/export happens outside subsystem locks. During Closing, collector waits only at safe points for already-reserved slots and exact scoped obligations, with the existing500guest-tick/150host-second bound. Unready reserved slots at end yield incomplete capture; no reuse or forged final record. Final invalid status is also exported separately from the record array, so buffer-full cannot erase evidence of overflow. Missing status/final header invalidates host acceptance.

Closing/Closed transition must exclude new reservations and prove all pre-close writers finished before exporting a valid final header; checking ready-count alone is insufficient if a producer can reserve afterward. Review an atomic admission/in-flight gate in the actual implementation, with no producer waiting under subsystem locks. A late scoped obligation/counter update after its final record invalidates capture; final counter entries remain frozen for export. No future-activity/quiescence proof is inferred from stable samples or index order.

[Bounded producer/close protocol](producer-close-protocol.md) now specifies one strong CAS admission attempt, one strong CAS slot attempt, release-ready publication and release guard decrement, with atomic STOP cutoff and safe-point Acquire drain. All counter-only observers also participate. Contention skips observation and marks capture invalid without changing runtime operation; no producer spin or new subsystem lock. This resolves the design shape, not actual sidecar memory safety/coverage or weak-memory correctness.

## Components and bounded obligations

Eight disjoint logical component classes per subject: (1) stacks/segments; (2) operations/completions; (3) grants/pins/VFS leases; (4) VFS receive contexts; (5) DMA/IOMMU cleanup; (6) deferred address-space release/quarantine; (7) capability/device-resource registrations; (8) other attributed bookkeeping. Class8is not a catch-all success: unknown/unattributable bytes remain unknown. Logical classes can reference the same physical frame; retain physical-union reconciliation separately and never sum overlapping stores as frames.

Emit one ComponentStart and one ComponentFinal per subject/class, including explicit checked-zero final state for absent classes. Each owner updates pending/released/retained/unknown counts, byte quantities and retry count on transitions without emitting a row for every attempt. ComponentFinal is emitted only once, when its scoped subject cleanup has a terminal accounting outcome or observation expires. It may be retained/unknown/incomplete and must not imply release. Intermediate retry counters are diagnostics; saturating/exhausted counters mark invalid rather than claiming exactness. A failed cleanup later succeeding changes internal state before finalization, not multiple final rows.

A fixed128entry private obligation identity pool,64bytes per entry (8192bytes), tracks active scoped obligations only. Entry fields: CellId/generation/subjectTID/private object token/byte quantity (five64bit values), component/state/flags (eightbytes), creation sequence and terminal sequence (two64bit values). Root mapping is the captured subject table; token uniqueness survives slot reuse. This is not a pointer-to-all-heap-allocations map: actual heap remains global. Existing queue metadata may instead carry equivalent copy-only facts, but must not duplicate charges. Pool overflow/provenance loss sets retained-unknown/invalid; it does not constrain runtime admission or discard an obligation as zero.128is an observation capacity, not proof every arbitrary workload fits; any selected run exceeding it is invalid and reported.

Fixed auxiliary payloads are also specified, not implicit heap-backed maps: subject table9×64bytes (subject/root/CellId/generation/parentTID/parentCell/parentGeneration/publicationAttempt, eight64bit words); component counters9×8×64bytes (pending/released/retained/unknown counts, live/retained bytes, retry count and owner epoch, eight64bit words); snapshot source metadata6×16×24bytes (epoch-before, epoch-after, status32, source-owner16, reserved16). Snapshot status is valid=1/absent=2/changed=3/unknown=4; epoch equality is meaningful only if every relevant writer participates in that owner's versioning protocol. Reading two equal values alone cannot prove no ABA/missed mutation. Unknown source owner or absent epoch prevents coherence claims.

Records, readiness, obligation pool, subject table, component counters and snapshot metadata total56896serialized payload bytes. Header/control atomics/alignment/export buffers are additional actual compiled overhead, recorded in artifact/baseline. Preallocate before pre-sample; no allocation inside producer hooks. ComponentFinal's four values are pending/released/retained/unknown counts; byte/retry/epoch facts come from its matching fixed counter entry and coherent final source metadata. No silent missing sidecar, duplicate charge or phantom zero.

## Event-capacity proof for the selected scope

N≤8children and R=N+1subjects. Driver has at most two existing loader attempts; each child uses one Pinned attempt: A≤N+2. No replacement or application retry. Rejected attempt ends the requested-count measurement but capture may still collect cleanup for already-published subjects.

An unpublished rollback is one LaunchResult, not an admitted subject or extra TaskTerminal. Its cleanup remains tracked by attempt/actual parent in the obligation pool, with retained/unknown outcome if association or release cannot be proved. Rollback does not create additional component row pairs; it contributes to the already scoped owner's counters where one exists, otherwise capture remains incomplete. Driver launch failure before any driver publication is NOT_RUN, not a count-zero control success.

| Record class | Maximum formula | N=8 |
|---|---|---:|
| Capture controls | Arm, pre boundary, closing, final | 4 |
| Snapshot groups | Six samples × four groups | 24 |
| Launch intent/result | 2A=2(N+2) | 20 |
| Subject terminal | R | 9 |
| Owner-slot and quota result | 2R | 18 |
| Component start/final | 2×8×R | 144 |
| Total | 4+24+2(N+2)+3R+16R=51+21N | 219 |

The256record buffer leaves37records, but this margin is not permission for extra event kinds, retries or threads. Count-zero control uses same schema/capture overhead (maximum51records). Snapshot samples are pre/live/cleanup and three coherent child-settled observations; final stable sample is the reported child-settled value, not a seventh sample. Driver is still present across these resource snapshots; its later cleanup is traced separately and never subtracted as child resource residual.

Proof is conditional on this **compact event schema**, not every raw allocation/retry receiving a row. More samples/task fan-out or duplicate final records invalidate the capacity claim. Any future schema/scope change must recompute it. Exact-once producer state and all unclassified outcomes must be reviewed/validated in actual code; this arithmetic alone proves neither event coverage nor runtime correctness.

## Snapshot and high-water rules

Four groups contain sixteen metrics in fixed order: total/used/free physical frames/page size; actual kernel heap used/free/scoped execution-context charge/scoped allocated stack bytes; scoped segment bytes/admitted live subjects/active operations/reserved completion slots; scoped outstanding cleanup obligations/retained physical-frame union/unknown-retained obligation count/capture invalid flags. Metrics with unimplemented owner/coherence evidence are absent, not zero, and prevent the applicable settled/resource claim. These are diagnostic metrics, not a public version of ProcessInfoV2 or approved production units.

Each subsystem's values are taken under its existing owner lock or an explicitly reviewed versioned copy. Never nest subsystem locks for a combined snapshot. A sample exports component values with source epoch/stage validity; values read sequentially from unrelated locks cannot claim a simultaneous aggregate. Presence/detail fields and capture sidecar carry source epochs where the compact record cannot fit them. Sidecar is fixed and baseline-accounted; coherence metadata cannot be silently omitted to preserve row capacity.

Child-settled samples begin only after exact child terminals and no attributed pending/unknown/quarantine obligations; background drift or concurrent scoped changes invalidate reconciliation. Three stable samples are an additional check, not the proof of quiescence. Physical-union reconciliation must identify overlaps or report absence/unknown; no duplicate frame count. Parent/observer/control costs remain visible.

Actual heap/frame maxima are updated at allocation-owner linearization while that existing allocator lock is held, using only bounded scalar writes/atomics. Requested Layout bytes are not allocator-aligned used bytes. No global simultaneous heap+frame peak is claimed: separate maxima and their possible upper bound are labeled. Scope activity between samples is not hidden by a stage flag. Event sequence alone cannot make a torn multi-owner snapshot coherent.

## Verification and implementation gate

Required throwaway design-model checks: explicit160byte offsets, all N=0..8capacity totals≤256, compact-record encoding/decoding boundaries, incomplete-slot rejection, no duplicate attempt finalization, and missing/retained metrics preventing settled. Model checks are not kernel execution or evidence the workload exists.

Before implementation: review the specified private mappings/counter/epoch layout against every touched source/error owner, producer memory safety/lock ordering and successful/failed publication/teardown coverage. No public approval is implied by this private format. Before resource acceptance: actual artifact includes trace overhead; exercise selected counts/control, intentional denied launch, missing final/overflow and retained cleanup without authority bypass. Existing async baseline remains separate.

Rollback removes isolated measurement instrumentation/workload/config/image; normal semantics and trace-free profiles unchanged. Keep raw evidence/invalid captures and dated superseding revisions. No source changes in this specification pass. P01/ACs/38locks/both checkpoints remain pending.

Design model exercised and saved in [model evidence](private-trace-model-evidence.json):160byte offsets, capacity N=0..8including maximum219,56896byte fixed serialized subtotal, eight invalid-record/presence/duplicate-final controls and five absent/incoherent/pending/retained/unknown settled controls. This is a limited throwaway model, not a complete parser, concurrency proof or implemented kernel test.

## Superseding private ownership export

Implemented private export includes `P01-HEAP-ENTRY` (152 bytes /19 little-endian u64s: original pointer, requested size, alignment, origin kind/attempt, ownership attempt and original tid/root/cell/generation; semantic kind at word10, current semantic tid/root/cell/generation at11..14 and transfer sender at15..18), `P01-HEAP-BINDING` (48 bytes: attempt, tid/root/cell/generation, closed) and `P01-HEAP-SOURCE` (48 bytes: before/after epoch, invalid, entry/binding counts, binding capacity). Runtime semantic kinds1/2/3/4/5 identify Operations/reply/mailbox/queued-wire/sender-wire; bit0x10 identifies an explicitly outside-selected task. Transfer sender is present only for a committed queued wire. Origin and semantic ownership remain separate; exact receiver transfer does not invent original allocator provenance. Existing80-byte exports remain historical and are not accepted by the new parser. Enlarged fixed ledger is accounted by compiled static_bytes();320-byte hex exporter buffer still fits.

`P01-RETAINED-RANGE` is now72 bytes: token, tid/root/cell/generation, base/end, current reason and immutable origin reason. Enumeration is version-bracketed; parser validates exact reasons, serials, owners, overlaps and physical union. Old exports lacking mandatory raw heap sidecars are not evidence for this revision.

Seven source-including host ledger tests exercise buffer transfer/realloc/release, stale tokens, overlap, layout mismatch and capacity/counter overflow. Host observer/hart adapters do not prove kernel admission-gate or SMP behavior. Full semantic coverage and simultaneous-allocation bound remain unavailable; parser rejects unsupported fit/coverage=true claims rather than treating a table ceiling as proof. Class8 remains unknown.

Runtime IPC joins cover caller Operations backing and reply wire, receiver mailbox backing and queued wire at actual commit. Failed queue publication does not transfer; realloc preserves joins; free uses stored provenance. Scheduler/source-owner→heap lock order is unchanged and allocator hooks never acquire scheduler. Superseding source-owned creation witnesses now cover quota nodes, log Vec/path handoff, death subscriptions/pending queues, lazy shared completion Arc, receive-copy scratch, direct hotswap copies and kernel synthetic wire publication. These named partitions are not complete pre-arm/background inventories; class8 and fit/coverage remain unknown/false.

The unchanged152byte heap export additionally uses kinds6=shared quota node,7=shared log backing,8=shared retained log path,9=shared death machinery,10=shared completion Arc,11=receiver copy scratch,12=kernel synthetic wire,13=receiver legacy/synthetic payload,14=replacement-owned migration copy and15=watcher pending-death backing. Shared kinds6/7/8/9/10/12 have zero semantic Owner/from and no adoption; bit0x10 is prohibited on shared kinds. Kind12 uses bounded byte-wire Layout. Task-associated kinds preserve full source-locked incarnation and may carry bit0x10 only for an explicitly non-selected owner. A migration copy is a new allocation, not a fictitious transfer of the original source pointer; failed/rolled-back appends release only the newly created objects. Synthetic producers have no invented Task incarnation or sender-transfer witness.

Per-hart creation guards label actual successful allocator Layouts, including opaque BTreeMap nodes and Arc headers, without depending on their representation. Nested guards restore prior state; ordinary observation admission prevents pre-arm/post-STOP hooks from mutating frozen evidence or setting LATE. Realloc preserves the original allocation provenance and semantic witness; actual free uses stored pointer/Layout. Known shared semantic witnesses prevent owner-filtered snapshots from falling back to Preparation/Execution attribution, but the exported aggregate partitions remain allocation-origin counts. Explicit pre-arm pointer/Layout adoption preserves Unknown origin; an unenumerated object is not an absent resource.

`scripts/x86/p01_elf_bounds.py` produces digest-bound successful-preparation partition arithmetic from retained build manifests and ELF64 headers. It checks interval union, unaligned boundaries, overlap permissions and arithmetic overflow; it is not signature/runtime/reclamation proof or an overall ledger-fit certificate.


## Additive sample1 heap inventory

Fresh artifacts make one bounded, preallocated 1024-slot copy of the live heap ledger at sample1 under the existing heap lock and an already-admitted observation guard. No allocation, scheduler call or new lock is introduced. Internal entries additionally retain a monotonically unique ledger-insertion incarnation; it is not a fabricated allocation birth for pre-arm objects. Reused pointers and reallocations receive new incarnations. Baseline semantic witnesses are copied, not retrospectively overwritten by final ownership.

`P01-HEAP-BASELINE-SOURCE` is 96 bytes /12 little-endian u64s: version1, sample1, heap ledger epoch, invalid, entry count, requested Layout bytes, allocator epoch, actual allocator used bytes, actual allocator free bytes, allocator source-owner2, capacity1024, and actual compiled baseline static-storage bytes. `P01-HEAP-BASELINE-ENTRY` is160 bytes: ledger incarnation followed by the unchanged19Q heap-entry schema. Final `P01-HEAP-ENTRY` remains152 bytes. Additive `P01-HEAP-INCARNATION` is16 bytes: final pointer and ledger incarnation. `P01-HEAP-ACCOUNTING` is64 bytes: before/after enumeration tuples of heap ledger epoch, allocator epoch, actual used bytes and actual free bytes. These tuples must agree and match the final heap-source epoch. Sample1 heap metrics use the exact baseline allocator copy, not a separate unlocked temporal guess.

The parser rejects missing, duplicate, malformed or truncated additive groups, overlapping allocations, unsupported source/epochs and changed pointer/Layout/origin for preserved incarnations. It exports exact created/released/preserved requested-byte maps, net deltas by semantic kind/full owner and the actual allocator-used delta. `unmatched_allocator_used_byte_delta` is explicitly allocator-used delta minus inventoried requested-byte delta; it is not automatically a leak, allocator metadata, extent padding, complete outside-selected attribution or proof that used bytes equal requested bytes. Complete semantic/allocator extent coverage remains absent; fit/coverage stay false and unequal raw residuals remain qualification blockers. Historical captures without the entire additive group return a null baseline and remain unqualified; raw captures are immutable.

The complete baseline static sidecar and per-entry incarnation overhead are included by `size_of_val(STORAGE)` in existing `P01-STORAGE resource_static` output; baseline-source word11 separately exposes its actual compiled contribution. Serialized payload size56896 remains the existing core trace payload subtotal, not a claim that additive heap storage has zero cost.

## Additive bitmap physical inventory

`P01-PHYSICAL-ENTRY` is96bytes/12u64: incarnation, frame, allocation epoch, free epoch, allocation attempt, kind, shared root, creation/failure VA, table state, cumulative path touches, failed path touches, failure load base or census epoch. `P01-PHYSICAL-SOURCE` is80bytes/10u64: version1, before/after source epochs, status, source-owner9, exported history count, capacity8192, supported-source mask7, remaining-gap mask7, final live history count. Source version epochs and bitmap allocator epochs are distinct and must not be substituted.

Kind0 has unknown semantic owner; kind1 has table creation provenance; kinds2/3/4 identify final-flags/mapped/writable preparation failure; kind5 has path evidence without creation evidence. Census kinds6/7 have allocation epoch0/attempt0 and word11 equal to the exact pre-arm allocator cutoff epoch; kind7 adds table-path evidence without a known birth. Freed histories retain their incarnation and provenance. Table path touches are cumulative, not current reference counts. Complete semantic/root/reference topology remains unproved; the original selected retained-union metric is unchanged.

Before opening admission, the exact used-bit census is taken under `FRAME_ALLOCATOR` and that lock remains held through the cutoff. For sample epochE, include known births≤E or census cutoffs≤E, excluding free epochs≤E. No pre-census reconstruction is claimed. Incomplete/faulted history cannot be used to reconcile allocator populations. Capacity8192 includes baseline used bits plus later incarnations and is only an observation ceiling; normal exhaustion is possible and fail-closed.

## Boot heap census and actual charged ownership

Private-enabled heap census now starts at the first successful allocation, before ordinary trace admission opens. Its lifecycle is Boot→Armed→Frozen under the existing heap lock; Boot→Armed does not stop tracking, and the collector freezes census entries together with allocator used/free accounting after ordinary STOP/drain. No heap/frame lock nesting or global simultaneous multi-source point is claimed. Post-freeze allocation/source callbacks leave the frozen census unchanged. Sticky capacity/counter exhaustion fails closed;1024remains an observation ceiling, not a fit proof.

Each allocation retains requested Layout, ledger incarnation, actual allocator-used increment, birth heap epoch and pre-arm flag. Charge is observed before/after allocation at the same allocator lock; it is not inferred from requested bytes or quota refund context. Free/realloc match the stored witness; missing entries are invalid rather than invented pre-arm births. Semantic source guards are active before arm. Shared kinds32..43 identify scheduler, signing, filesystem, driver, service, boot metadata, system configuration, filesystem scratch, futex, grant, domain and test-fixture backing respectively; kind47 identifies pipe backing. Exclusive64..68 identify task Box, segment array, AddressSpace allocation, task fields and task I/O backing. Historical nonselected bit16 remains separate (exclusive80..84); shared kinds never carry it. Task publication joins exact Box/name/cwd/argv/allowed-driver/segment-array/staged-argv/waiter/death-queue backing after final identity fields under scheduler ownership. Filesystem handle creation uses a shared fallback only without an enclosing semantic context; existing task-I/O ownership is preserved. No witness is assigned merely because charging Cell is0.

Existing19Q final and20Q baseline entry schemas remain unchanged. Additive `P01-HEAP-CHARGED-ENTRY` and `P01-HEAP-BASELINE-CHARGED-ENTRY` are40bytes/5u64: pointer, incarnation, actual charged bytes, birth heap epoch, pre-arm flag. Matching `P01-HEAP-CHARGED-SOURCE` and `P01-HEAP-BASELINE-CHARGED-SOURCE` are176bytes/22u64: version, sample, phase, ledger epoch, allocator epoch, used, free, count, requested, charged, unmatched, unowned, shared, exclusive, total pre-arm allocations, live pre-arm allocations, observed live highwater, overflow, capacity, compiled static bytes, invalid, source-owner2. Partitions sum to charged bytes; unmatched accounting, incomplete groups and inconsistent incarnations/epochs fail closed. Historical absent charged groups remain absent.

The exporter hex buffer is352bytes and actual compiled census storage is included in resource static overhead. Actual charged reconciliation is allocation census, not complete semantic ownership or arbitrary-path capacity proof. Source hooks and host/parser regressions are implemented; runtime proof is recorded separately after fresh capture.

## Additive live and settled heap censuses

The private ledger keeps five fixed immutable stage inventories for samples2..6. Admission to a stage observation precedes copying actual entries and allocator accounting under the existing heap lock; no allocation or new lock is introduced. Duplicate/out-of-range stages, wrong Boot/Frozen phase, incomplete accounting, counter/capacity exhaustion and invalid source state fail closed. Sample1 remains the pre-arm baseline; denied control emits only its actual sample3 census. Export occurs after STOP/drain/freeze, never by rereading a later allocator state for an earlier sample.

Additive lines are `P01-HEAP-STAGE-SOURCE <decimal sample> <22Q hex>`, `P01-HEAP-STAGE-ENTRY <sample> <19Q hex>` and `P01-HEAP-STAGE-CHARGED-ENTRY <sample> <5Q hex>`. Layouts are the existing source/raw/charged layouts above; source words1/2 contain sample2..6 and Armed phase1. Source allocator epoch/used/free match metric4/5 sidecars at that same cutoff. Every actually observed post-pre sample requires its complete group; duplicate pointers/incarnations, malformed totals, partitions, source identity or joins are rejected.

Across complete censuses, ledger/allocator epochs are nondecreasing. An unchanged ledger epoch requires an identical inventory; preserved incarnations cannot change Layout, original origin, charge, birth or pre-arm state. A newly live incarnation must postdate the previous ledger cutoff, and its allocator birth must postdate the previous allocator cutoff with pre-arm0. Semantic/adoption/transfer changes require an advanced ledger epoch and remain distinct from rebirth. Historical complete absence of all new stage tags preserves old qualification and exports `heap_stages=null`; partial evidence cannot mint coverage.

The Log syscall publishes the actual temporary user-copy pointer, capacity-derived Layout and full caller Owner as TASK_IO before invoking the stage callback. It does not infer ownership from allocator execution origin or charge Cell0. This closes the observed16charged-byte baseline gap while retaining original allocation provenance.

The preceding heap-only all-count matrix's compiled census storage is1378528bytes; resource static storage2367312bytes, core static59125bytes and exporter buffer352bytes are recorded separately. This private overhead remains visible and is not subtracted to infer a production image footprint. Snapshots prove their cutoffs only: they do not reconstruct every intervening lifetime, reset boot highwater at arm, prove all-path1024fit or supply physical/shared-reference coverage. The final [budget measurement report](profile-budgets.md#observed-all-count-budget-measurement-basis) keeps these refusal gates active.

## Current CPU graph and exact leaf-run wire

The private `p01_topology` producer walks the roots of all currently published scheduler tasks plus the kernel root, not only selected Cells. Root rows carry full task lineage and native Domain identity/generation when applicable. Deduplicated nodes preserve physical table identity, level, current incoming descriptors and the actual physical-incarnation join. External boot tables have no invented managed incarnation. Current references are not the retained ledger's cumulative attach/detach counters.

`P01-TOPOLOGY-SOURCE <sample> <16Q hex>` words are version, sample, physical allocator epoch, used frames, online CPUs, root/node/encoded-edge counts, root/node/edge capacities, status, exclusion reasons, scope, required nodes and required encoded edges. Capacities remain64/1024/16384. Status1 publishes a complete counted-and-verified cutoff; status4 publishes no partial rows. Refusal bits are unsupported1, missing root2, capacity4, malformed8 and missing history16. An aborted count cannot certify storage fit or workload capacity.

`P01-TOPOLOGY-ROOT` is8Q: root PA, task TID, root lineage, Cell ID, Cell generation, Domain ID, Domain generation, root class. `P01-TOPOLOGY-NODE` is5Q: table PA, level, physical incarnation, managed/external classification, current incoming descriptor count. Version2 `P01-TOPOLOGY-EDGE` is7Q: parent PA, first descriptor index, normalized descriptor, first target PA, per-descriptor leaf bytes, classification, span. Classification1 is entirely allocator-owned,2 external/mixed and3 managed-free leaf. Free direct-map leaves do not imply table lifetime or semantic ownership.

Only contiguous4KiB leaves within one PT with identical permissions/PAT/NX and classification form runs. Each expanded index and PA remains exact; table edges and2MiB/1GiB leaves have span1. The count pass includes the entire graph, including runs beyond storage capacity; fill validates every present/absent descriptor against the counted representation before publication. Version2 clears hardware accessed/dirty bits: these bits can change while the CPU accesses snapshot backing and are explicitly not observed atomically. Historical version1 captures retain their6Q descriptor wire.

Capture is permitted only after scheduler readiness on one online CPU. Root discovery precedes FRAME_ALLOCATOR then KERNEL_ROOT locking; local IRQ exclusion covers both passes. SMP and unsupported source states refuse rather than claiming cross-CPU quiescence. CPU DAG reachability, shared task roots/tables and physical leaf aliases are cutoff evidence only. NPT/EPT/IOMMU roots, unpublished/retired roots and global physical semantic ownership remain excluded. A passing graph preflight proves snapshot storage at that cutoff, never future/all-path capacity or budget fit.

Actual version2 verification: the [current CPU graph matrix](profile-budgets.md#capacity-refusal-and-observed-current-cpu-graph) contains78 complete graph cutoffs across15 fresh boots. Resource static overhead is8143448bytes; it includes fixed six-cutoff graph storage and remains distinct from the still-unchanged59125byte core and352byte exporter buffer. All-path/workload and global physical-owner gates remain false even when the graph fits.

## Allocation-lifetime non-CPU root census

`p01_noncpu` witnesses the actual NPT/EPT root constructor/free path, Sv39/VT-d SLPT constructors and VT-d root/context structure allocation. A fixed128-slot live ledger assigns a monotonic birth token, separately from the physical frame and heap allocation incarnations. Construction/destruction activity covers the allocation-to-publication and free-to-retirement gaps. Nested roots retire only after actual table-frame release; IOMMU types have no freeing destructor, so dropping their Rust handles does not remove their still-allocated root memory.

`P01-NONCPU-SOURCE <sample> <16Q hex>` is version1, sample, lifetime epoch, physical allocator epoch, used frames, online CPUs, live-root count, reference count, root capacity128, reference capacity256, status, exclusions, in-flight activities, total births, scope7 and actual HHDM base. A complete cutoff has status1, zero exclusions/activities and `epoch = 2*births - live_roots`. Refusal status4 emits no root/reference rows. Exclusions are unsupported1, capacity2, lifetime4, registry reference8, physical history16 and in-flight32. These are diagnostic ceilings, not admission or workload-fit bounds.

`P01-NONCPU-ROOT` is8Q: birth token, physical root PA, family, actual physical incarnation, backing kind, actual heap pointer, actual heap incarnation, current software-reference count. Families are NPT1, EPT2, Sv39 SLPT3, VT-d SLPT4 and VT-d root/context structure5. Backing is frame1 or heap2. Heap-backed rows require the actual4KiB/alignment4096 allocation at the corresponding heap cutoff and its exact incarnation; physical frame identity alone cannot certify reuse of a heap allocation.

`P01-NONCPU-REFERENCE` is5Q: root token, reference role, raw holder key, raw object key and parent root PA. Roles are VM registry1, software VT-d root slot2, per-bus context3 and per-holder domain4. Raw keys are current references, not reconstructed Cell generations, exclusive creation owners or authority grants. Parent/family/key/count, duplicate, source epoch, physical/heap incarnation and non-freeing IOMMU survival checks fail closed. Historical complete absence remains unknown.

The CPU topology lock excludes local IRQs across metrics and this census. Lifetime copying releases its lock before registry visitors; VM_REGISTRY is visited separately, VT-d uses DOMAINS then CONTEXTS, and both visitors release before FRAME_ALLOCATOR/history joins. Only boot-ready, one-online-CPU x86 cutoffs are certified. Descriptor trees, actual hardware publication/invalidation, creation-owner authority, other root/allocation families and all-path capacity remain excluded.

The fresh [root-lifetime matrix](profile-budgets.md#observed-non-cpu-root-lifetimes) has78 complete cutoffs; current resource static overhead8263048bytes includes this ledger and six immutable root/reference snapshots. No overhead is subtracted to infer production cost.

## Format-aware non-CPU software descriptor trees

`P01-XTREE-SOURCE <sample> <16Q hex>` records version1, sample, physical epoch, used frames, root count, node count, edge count, root/node/edge ceilings128/1024/16384, status, exclusions, supported-family scope15, required nodes, required edges and root lifetime epoch. Status1 requires exact two-pass count/fill, unchanged physical identities and a complete allocation-lifetime root census at the same cutoff. Status4 emits no node/edge rows. Refusal bits are unsupported1, capacity2, malformed4, physical history8 and root census16; ceilings are not future admission bounds.

`P01-XTREE-NODE` is5Q: PA, family, level, actual physical incarnation and incoming descriptor count. `P01-XTREE-EDGE` is7Q: parent PA, index, normalized raw descriptor, target PA, per-descriptor leaf bytes, run span and family. NPT/EPT use four levels; Sv39/VT-d SLPT use three. Only adjacent4KiB leaves with identical permissions, contiguous targets and the format-specific raw step coalesce within one parent. Fill checks every expanded descriptor, including the middle of a run and absent/new entries. Format-specific A/D normalization does not erase EPT memory type, Sv39 permissions or NPT huge-page PAT semantics.

The outer CPU-topology IRQ exclusion spans metrics and both root/tree snapshots. The tree reader holds the immutable root snapshot and FRAME_ALLOCATOR, validates each table's active physical incarnation before volatile HHDM reads, and never acquires the heap lock under FRAME_ALLOCATOR. Decoder joins all heap-backed root and child tables to their actual pointer,4096-byte/alignment4096 Layout and heap incarnation at that cutoff. Root birth tokens, physical incarnations and heap incarnations remain separate identities.

Software alias intervals combine non-CPU leaf extents with the CPU leaf graph where that graph is complete, counting distinct root mapping paths rather than tasks sharing the same SAS root. They are software descriptors, not hardware publication, DMA authority or physical allocation owners. VT-d128-bit root/context structures remain explicitly unsupported: a live family5 root refuses the tree certificate rather than disappearing. Other allocation families, hardware acknowledgement and all-path capacity remain excluded.

The fresh [software-tree matrix](profile-budgets.md#observed-non-cpu-software-trees-and-aliases) certifies78cutoffs. Resource static overhead is14014608bytes; no diagnostic overhead is subtracted to infer production cost.

