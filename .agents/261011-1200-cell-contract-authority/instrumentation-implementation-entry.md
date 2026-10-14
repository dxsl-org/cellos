# P01 — Source-bound instrumentation implementation entry

Planning deliverable only. No source/ABI change or instrumentation runtime test. This consolidates [owner review](instrumentation-owner-review.md), [trace schema](private-trace-spec.md) and [producer-close protocol](producer-close-protocol.md) into a bounded implementation handoff; it does not complete P01 or authorize migration/profile activation.

## Source facts and guard lifetime

| Handoff | Observed source | Required change in the later private implementation |
|---|---|---|
| Trusted bootstrap init | `kernel/src/main.rs:1421-1429`; `loader/governed_spawn.rs::spawn_trusted_init:214-245` | Carry an explicit private BootstrapInit source marker from the real boot callsite, not every same-named test fixture. Bind actual init owner/digest before push_ready. Do not obtain identity from the returned TID after init can already run on another CPU |
| Syscall launch ingress | `task/syscall.rs::governed_spawn_request`, SpawnFromElf/SpawnFromPath/SpawnPinned dispatch | Mint copy-only logical attempt context before fallible validation; retain actual caller tuple and route. A trace observation guard is short-lived, not held through hashing, file reads or allocation |
| Request | `loader/spawn_request.rs::SpawnRequest:5-12` | Add cfg-only optional copy metadata; preserve caller ceiling, inherit_from, argv and replacement semantics. Boot/replacement paths default to unselected unless explicitly selected; no authority field becomes an observer permission |
| Governed preparation | `loader/governed_spawn.rs::spawn_gated:36-211` | Signature/admission/ceiling/priority/preparation failures occur before TaskLaunchState. Preserve attempt context through them and across request→state move. Do not use an observation guard as a long-lived preparation reservation |
| Final state | `task/launch.rs::TaskLaunchState:27-46`, complete, publish_prepared | Private metadata accompanies existing complete state. Publication event is one short observation at committed owner/digest boundary before ready push. Existing path/digest may be staged; no new String/hash inside this observation |
| Caller outcome | `spawn_gated:211`, trusted-init:244` map publication result directly | Success is finalized by publication only; ingress must not emit a duplicate successful LaunchResult. Ingress failure result wrapper emits one rejection/rollback after local guards have dropped, while preserving actual returned error. Missing publication observation invalidates capture, not returned success |
| Init ELF→path fallback | `cells/tools/init/src/service_table.rs:56-78` | Keep behavior unchanged: argv republished on each attempt, grant freed, OOM does not fall back. Two syscall attempts get distinct contexts; failed argv/VFS preparation is NOT_RUN if no launch reaches kernel |
| Deferred teardown | `scheduler.rs` retirement extraction, `task.rs:1318-1424` | Copy immutable subject/incarnation/obligation token before task drop; use fresh short observer guard at actual release result. Never keep an active trace guard across a queued retry or while waiting for remote quiescence |

**Logical attempt context is not the active observation guard.** Attempt lifetime may span fallible loader work and rollback; active count covers only trace/counter access. This avoids allocator recursion and close waiting on an arbitrary filesystem operation while preserving incomplete-attempt evidence. Pending logical attempts/obligations are still part of the collector's quiescence predicate.

A scoped failure wrapper must cover all relevant `?`/return paths, including pre-request argument/grant/path validation. Its observer result cannot replace the public error or run allocator work inside a source lock. Ordinary Rust Drop during normal returns is not a guarantee on fault/panic paths: abandoned context/guard produces incomplete evidence. Do not force-clear it.

`governed_spawn_request:1727-1732` currently snapshots generation+CapSet, not the full Cell/root tuple. Private parent CellId/rootTID must be copied in that same guarded task observation (and at earlier ingress when needed), not reconstructed after request move. Existing authority revalidation at publication remains unchanged. The observation guard does not survive scope exit or borrow a live Task after SCHEDULER is released.

## Arm/source-selection correction

The kernel cannot see which userspace `CellSpec` callsite produced a syscall, and SpawnRequest currently carries no configured-entry token. Adding such a public token/inspection syscall is out of scope. Therefore refine the earlier arm-at-configured-ingress wording:

1. Isolated build manifest binds init/config/driver/probe digests and exact expected paths/routes. Private BootstrapInit marker is set only at the real main boot launch, absent on atomic-publication fixtures. This marker has no admission effect.
2. Prepare observer storage before init; bind the actual BootstrapInit CellOwner/digest and arm **before that init's ready push**. Boot fixtures before this marker are excluded. CaptureArm precedes driver attempt; parent init is a bound external parent, not one of the nine workload subjects.
3. Kernel syscall ingress creates logical attempt context even if target is not yet classifiable. Bounded path validation selects driver candidate by attested bootstrap caller and expected authorized path; digest is confirmed at normal governed admission/publication. Known unrelated paths are excluded from scoped record rows, not from global resource drift.
4. An unclassifiable request from a selected parent cannot be guessed as unrelated. It invalidates capture with preserved ingress/error evidence; driver not published means NOT_RUN. Do not call an unknown request a successful zero-child control. Minting attempt context before validation is not a claim an unreadable caller path is kernel-attested.
5. Scoped children are selected by actual driver parent identity and expected Pinned path/digest. Never by task name or consecutive TIDs. Arm is one-shot; no init feature/new syscall/policy widening.

This correction preserves the compact event bound: selected driver has at most two attempts, each child one; unrelated known bootstrap attempts have no scoped trace rows. Unknown pre-driver input ends valid-measurement eligibility, not runtime execution. Before pre-sample, reset the designated resource peak window under its existing owner synchronization; startup peak and child-workload peak remain distinct. No allocation of trace storage after pre.

## Auxiliary storage ownership

| Storage | Single-writer / sharing rule | Observation boundary |
|---|---|---|
| Immutable manifest/capture scope | Build/boot prepares before arm; bind bootstrap tuple under publication owner | Release publication before any consumer sees scope; no live String/path parsing in allocator hooks |
| Subject identity entries | Assign once under SCHEDULER at publication; entry never reused during capture | Publish readiness Release; later cleanup receives copied index+full identity and checks exact generation. Acquire before reading immutable entry |
| Trace payload slot | One successful slot reservation owns one slot | No subsequent writes after ready Release; no unsafe shared mutation justified by the admission gate |
| Obligation identity entry | Immutable identity/token assigned before handoff; mutable state transition has a named source owner or atomic CAS | Cross-owner completion cannot overwrite non-atomic state. One bounded CAS conflict invalidates observation; runtime resource transition is unchanged. Slot reuse only after observer terminal and a fresh nonwrapping token |
| Component counters | Scalar atomic values or fields updated under their existing source owner; not a lockless mutable struct | Checked bounded updates; partial/incoherent diagnostic update invalidates capture. Epoch participates in every relevant write or metric is absent/changed, never fabricated coherent |
| Actual heap/frame maxima | Existing allocator lock serializes that metric's updates | No trace/index lock, recursive usage() call, source-lock inversion or sum-of-requested-bytes relabeling |
| Snapshot source metadata | Collector-owned fixed output copied from source-coherent observations | Copies are not one multi-lock instant. Driver/global and child metrics remain separately classified |
| Final component entry | Frozen only after exact component terminal outcome; immutable thereafter | Late update marks separate sticky invalid flag; cannot change already exported success into a silently revised counter |

Admission STOP+active protects producer lifetime, **not auxiliary data races**. A scalar atomic snapshot is coherent for that scalar, not automatically for a compound component row. Dynamic cross-owner compound rows without a proven owner/version protocol are absent/changed; they cannot qualify live/settled. Final child counters can be copied after exact child quiescence and per-entry finalization; driver may still be alive and is not subtracted as reclaimed.

For actual implementation, inspect source-lock ownership for every field and align the producer version/update protocol with the declared snapshot sidecar. No blanket seqlock over all owners, unbounded CAS loop, new nested scheduler→heap/pin lock or interrupt masking is approved. If shared state cannot meet these constraints, reject that implementation design; do not weaken the metric's meaning to get a passing number.

## Explicit blast radius and implementation order

One development-only compile gate is necessary so trace storage is absent from ordinary/test-hooks images not measuring P01. Proposed private build input `CELLOS_P01_TRACE=1` is recognized by kernel/build.rs only with test-hooks and target x86_64, emitting checked internal cfg `cellos_p01_trace`; absent/0means omitted, other values or incompatible target/features reject the measurement build. This is not an init feature or public runtime setting. `rerun-if-env-changed` prevents stale builds. It is a future build-script edit, not a flag currently implemented.

Expected touched files during later work: kernel/build.rs, kernel/src/main.rs and proposed kernel/src/p01_measurement.rs; loader/spawn_request.rs, governed_spawn.rs, task/launch.rs, task/syscall.rs; actual scheduler/task/reaper/stack/allocator/deferred/pin owner sites identified in owner review; cells/tests/bench/src/main.rs, bench-probe.rs and proposed scenarios/parked_footprint.rs; an isolated build/capture wrapper. New diagnostic fields/storage compile out when private gate is absent, not no-op forwarding shims. Before changing exported Rust symbols, obtain actual LSP references and migrate every constructor/caller; no field shim or ABI claim. No edits to libs/api wire records, production policy/signing, shell authority, cluster/controller/router behavior or unfinished old plans.

Implement as one P01 preparation slice, not phase11:
1. Add compile gate and fixed private storage/encoder/decoder/guard; verify disabled build omits payload and preserves profiles. Add source-bound coverage table to actual diff.
2. Thread private source/attempt metadata through ingress/request/state, bind bootstrap init before runnable publication, record successful and rejected outcomes without duplicate result or altered return. Keep existing signing/admission/atomic checkpoints.
3. Bind subject/obligation identities and actual component release/retention counters at reviewed owners; include guard frames and pre-loss queue-full outcomes. Do not fix unrelated runtime accounting in this measurement change.
4. Add the finite parked driver/probe behavior and init-owned private config, with explicit release/exit/settling markers and three coherent settled samples. Require exact children; no retries or kill-based success.
5. Run source/unsafe/lock review on the real diff before resource interpretation. Exercise normal count-zero/1/4/8, denied launch, observer contention/full/unready/late retained outcomes and disabled build; preserve all failures and no-new-regression evidence.

## Go/no-go and rollback

Planning entry is defined; **implementation approval and actual code review are not claimed**. Source-bound risks are now named: bootstrap observer marker, logical/active guard separation, conditional target classification, private compile gate, atomic auxiliary ownership and absent cross-owner coherence. Existing async runtime remains baseline only.

A later development measurement slice may not activate new Cell contract/public ABI or release locks. Exact Law-1 checkpoints remain independent. If required ABI change is discovered, stop that delta and submit exact proposal rather than slipping it into diagnostics. If observability cannot cover retention safely, report incomplete resource evidence.

Rollback deletes only measurement-created code/config/artifacts and restores touched owners' original behavior; no production data migration. Keep source review, raw invalid captures and superseding decisions. Capacity/model checks prove only their stated schema/interleavings, not this unimplemented source coverage. All10phases/38locks/50new-contractACs remain pending.

Observed [planning decision model](implementation-entry-model-evidence.json):16private-build cases (one enabled, eight omitted, seven rejected), five exact-parent/unknown-target selection cases and two result-owner cases. These exercise proposed decisions only; the environment flag/source marker are not implemented and no real compiler/kernel result is inferred.

## Superseding implementation evidence

Private source implementation and bounded runtime captures now exist; planning-only statements above remain historical. Latest [source-coherent runtime evidence](plan.md#source-coherent-counters-and-denied-control--superseding-runtime-evidence) supersedes the first implementation run: fifteen structurally complete captures, three independently accepted denied controls preserving public Err(Unknown), source-owned counters and24/24 parser tests. Normal resource qualification remains false: physical retained-range union and historical allocation-owner evidence are unavailable, with explicit prerequisites recorded in the handoff. Fresh disabled compilation omits private export tags. No ABI activation, lock release, production budget, two-CPU qualification or commit follows.

