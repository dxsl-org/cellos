# P01 — Private trace producer/close protocol

Design only for the [private x86 trace](private-trace-spec.md). No code or public ABI change. This protocol intentionally permits an invalid measurement under contention instead of unbounded producer retries. Actual memory safety, atomics and hook coverage still require implementation review/runtime verification.

## Decision

Use one native64bit atomic admission word: bit63STOP, bits0..62active observer count. Initial valueSTOP; arm once with release-store0after preallocated trace/scope/header setup and actual BootstrapInit binding, before its ready push, per [source-bound entry](instrumentation-implementation-entry.md). Earlier configured-driver-ingress wording is superseded; no public configured-entry token is added. Capture lifecycle Closing still accepts necessary cleanup observations. A separate final **freeze** sets STOP and forbids any further reservation; this is not the earlier Closing lifecycle event.

Active observer count is bounded1024for diagnostics, not runtime thread/CPU admission. Reaching that bound invalidates capture; do not increment or spin. The256record index is a separate native atomic; never wrap/reuse. All producer observation paths (record writes, component/obligation counters, peak updates) use admission, not just record-producing paths. Otherwise close cannot freeze auxiliary payloads.

The diagnostic tradeoff is explicit: one unsuccessful CAS skips observation and sets sticky invalid status, while the underlying operation continues with its original result. Measurement success is not guaranteed under interrupt/SMP contention. Do not add retries, disable interrupts, take new locks or suspend CPUs to make it pass.

## Producer steps and memory ordering

1. Check immutable selected scope. Unselected events return without trace work. Boot/normal background events are not retrospectively attributed to selected subjects.
2. Load gate Acquire. If STOP is present or active≥1024, do not enter. A selected post-freeze event invalidates capture; an unselected/global update outside the explicit measurement window is simply outside scope.
3. Try one **strong compare_exchange** from observed gate to gate+1, successAcqRel/failureAcquire. Failure sets invalid-contention and returns without trace/counter access. A strong CAS avoids introducing spurious failure as a normal invalidation source. No retry loop.
4. On success, the producer owns an active observation guard. It may update only the relevant fixed observation state while following that source's existing synchronization. Admission does not serialize producers; it does not make a BTreeMap/ledger/UnsafeCell safe. Shared subject/obligation registration must occur under the existing lifecycle owner lock, with index/immutable identity copied to later cleanup. Cross-owner component counters must be atomic or exclusively owner-locked; mutable auxiliary fields cannot be written concurrently without a reviewed protocol.
5. If a record is needed, load index and attempt one strong CAS to reserve index+1 only when index<256. Reservation contention/full sets invalid and skips that record. Successful reservation owns exactly that slot. Construct the complete160byte payload there; ready flag release-store1only after all payload bytes are initialized. No partial record is ready. No console/allocator/formatting/extra lock in producer.
6. After every admitted observation path, including reservation failure, release-decrement active count exactly once. Decrement preserves STOP set by a concurrent closer. A copy-only guard is an implementation aid, not permission to assume fault unwinding always runs. Counter overflow or unmatched decrement invalidates; never wrap or repair state by pretending a writer finished.

One guard per observation transaction; do not recursively admit another guard for its helper functions. Interrupt-nested observations can have separate guards and slots, but cannot mutate the same non-atomic state reentrantly. If an admitted writer faults/stalls and never decrements, closure times out as incomplete. No force-clearing count, no fake ready flag.

## Final freeze and export

Only one safe-point collector may freeze. Before a successful close it must have exact selected lifecycle/obligation terminal evidence; stable samples alone are insufficient. A timeout may freeze an incomplete capture, preserving unknown state.

- Fetch_or(STOP,AcqRel). Its atomic modification-order position is the admission cutoff. An observer admitted earlier keeps its guard until payload/counter work completes. A stale pre-freeze gate load cannot successfully CAS after this cutoff.
- At safe points outside subsystem locks, load gate Acquire until active=0or observation bounds expire. No collector waiting while holding scheduler, allocator, pin or address-space locks. No producer waits for collector.
- If active remains nonzero, export only ready records as partial evidence and label incomplete; never inspect unready slots or mutable auxiliary entries.
- With STOP and active0observed, no admitted observer can still write. Acquire-load each ready flag before reading its immutable slot. Expected reservation count and ready count must agree; a hole/invalid flag rejects full-capture acceptance.
- Freeze source counters/sidecars under their already reviewed owner protocol before copying. Guard drain alone does not turn values into a coherent multi-owner instant, and never certifies source writers omitted from admission.
- Export immutable records/counter snapshot and sticky validity status outside locks. Full buffer or timeout must still have an independent final invalid status; missing final status is invalid at host.

Release-decrements form an atomic release sequence on the same gate, with the closer's final Acquire observing completion. Record-ready Acquire is an additional payload publication edge. Gate/index observation order is neither scheduling time nor causal order across sources. No SeqCst requirement is claimed; this reasoning must be checked against actual implementation and supported target atomics.

## Late events and validity window

After STOP, no observer may modify captured payload. A late selected event sets the separate sticky invalid flag only; actual runtime cleanup continues. The exported result describes the bounded capture interval, not perpetual absence of future events. Host retains raw serial through final safe-point validity sampling and VM termination; any late invalid notification or missing termination bound invalidates acceptance. A producer cannot print a notification while holding an allocator/source lock: the safe-point collector emits it. Do not declare success from an earlier final line and stop consuming serial.

Unselected allocations made by exporter after freeze are not part of measured peak/residual. Exporter storage must already be accounted; it cannot hide pre-freeze observation overhead. An outstanding selected obligation cannot be classified as unselected merely because its original task disappeared.

## Interleaving obligations

| Race | Required outcome |
|---|---|
| Producer loads open gate; closer freezes before producer CAS | CAS fails, no payload access, capture invalid for skipped selected event |
| Producer admits; closer freezes while producer is writing | STOP retains active count; closer cannot accept complete capture until producer ready-publishes and releases guard |
| Two producers load same active count | At most one identical CAS succeeds; loser returns bounded invalid, does not touch counter/slot |
| Two admitted producers load same record index | At most one owns slot; loser invalidates then releases guard |
| Fault between slot reservation and ready publication | Slot never read as complete; active guard/ready hole makes capture incomplete |
| Counter-only producer overlaps close | Same gate as record writer, so no unfenced auxiliary mutation |
| Late selected cleanup after freeze | Runtime unchanged; invalid status, no overwrite/reclassification as reclaimed |

Atomic admission/index strongly serialize their own scalar transitions only. The model below is not proof that these events are correctly inserted in kernel paths, that language-level non-atomic access is safe or that weak-memory targets obey an unshown implementation.

## Verification and rollback

Throwaway interleaving model must enumerate producer-load/CAS/write/ready/release versus closer-stop/drain, assert unique slots, no post-cutoff successful admission and no valid close with active/unready work; cover reservation failure and counter-only observers. Report model counts and bounds, not kernel PASS.

[Observed model evidence](producer-close-model-evidence.json) enumerates20774interleavings across four two-producer/one-closer scenarios: normal11472, one-slot4848, counter-only second producer1407, first-writer fault after reservation3047. One-slot and fault scenarios never reach a complete final state; normal/counter-only scenarios permit complete or invalid outcomes according to ordering. Invariants held for guard count, unique slots, ready-after-write, preserved STOP, stale admission failure and incomplete faulted writer. Final-state reconciliation includes late invalid flags. This is finite strong-atomic sequential modeling, not a Rust/weak-memory proof, runtime success rate or hardware timing bound.

Implementation-entry checklist: review x86 target lock-free support, exact AtomicU64/usize representation, guard balancing, no reset/rearm, sidecar ownership/reentrancy, sticky-invalid export and all hook paths. Actual denied-launch/overflow/late-cleanup/fault captures and normal scenarios must then be exercised. No runtime sample or production budget follows from this model.

Rollback removes only isolated development instrumentation; normal runtime state/results never depended on it. Preserve invalid captures and superseding docs. All10phases/38locks/checkpoints remain pending.
