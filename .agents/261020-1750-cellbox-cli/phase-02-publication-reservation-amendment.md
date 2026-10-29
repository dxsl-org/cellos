# Phase02 CP1 amendment — allocation-free publication reservations

Status: **proposed; awaiting separate exact approval**. This extends only the file closure of the already approved [Phase02 annex](phase-02-source-admission.md), SHA-256 `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`. It changes no assigned syscall, selector, record, error, authority, fixture route or rollout restriction.

## Observed missing prerequisite

The approved annex requires fallible preparation followed by allocation-free final child publication. Source inspection found two remaining existing allocation sites outside its78 paths:

- `kernel/src/measurement_log.rs:51-98::commit_staged` advances the aggregate then attempts `entries.try_reserve(1)`. OOM drops the detailed entry after publication has begun. It has no retained reservation for prepared children.
- `kernel/src/task/hart_local/ready.rs:124-130::push_on_hart` uses `BTreeMap::entry(...).or_default().push_back(id)`, allocating a map node or queue growth at final ready insertion. The queue field/type lives in `kernel/src/task/hart_local.rs:120`.

Delaying the ready push or ignoring log OOM would not satisfy the accepted publication invariant. No edits to these three files have yet been authorized or applied. Other reachable approved work continues independently.

## Exact added source closure and initial bytes

| Added path | SHA-256 before amendment edits |
|---|---|
| `kernel/src/measurement_log.rs` | `1978d3e975535876ff0a64df1061e9940f2a44e77aa2afbf335d4a582828f71a` |
| `kernel/src/task/hart_local.rs` | `594dbdb022318189985ac1c2775fcb83bf3fbf2a8ef5c5b72b7bdbf2ef5f856b` |
| `kernel/src/task/hart_local/ready.rs` | `7b00764a4f89dfd9fd799c883d420e738f4dc73e356d6c2535782e6c433d3dbf` |

Other already approved loader/scheduler/session integration files consume the reservation guards. No new protected directory grant, alternate ready queue, alternate CellOwner, measurement bypass or logging suppression is authorized.

## Required implementation behavior

1. Add a canonical `measurement_log::PublicationReservation::reserve() -> Result<Self, ViError>` guard. Reserve bounded charged detailed-log capacity before visible publication, accounting for all outstanding guards so another launch cannot consume promised capacity. At the existing log maximum, refuse preparation explicitly; do not silently omit its required detailed record. The aggregate and log entry change together in `commit(self, tid, path: String, digest: [u8;32])`, with no allocation. The path is already owned/staged before commit. Drop/abort releases the reservation without aggregate/log mutation. Ordinary existing launch-log behavior remains unchanged unless it must honor capacity already reserved for pending children; it cannot steal a reservation.
2. Add `hart_local::ready::PublicationReservation::reserve(priority:u8) -> Result<Self, ViError>` pinned to the scheduler-selected destination hart. Reserve its canonical queue entry/capacity before publication. The final `commit(self, tid)` cannot allocate, fail or publish a placeholder task. Other pushes, steals, priority changes and wakeups may not consume the outstanding promise or move its reserved slot to another hart. Drop releases the promise without a runnable task. Keep SCHEDULER→ready leaf lock order and existing priorities/RT placement/preemption semantics.
3. Use a bounded fixed priority-bucket representation, or preallocated reserved capacity with equivalent guarantees, inside the existing canonical per-hart ready owner. No second prepared/live scheduler truth. Retain existing public internal queue-helper behavior and tests; preserve FIFO order within priority and work-stealing rules. All fallible memory work precedes final owner/context/receipt visibility; quota/accounting and abort/death release are real, not assertions or infallible hidden growth.
4. Kernel integrating owner holds these guards in its existing prepared-publication resource container. Commit order remains context/grants/issuer receipts/CellOwner/routes/measurement then ready, under the approved canonical lock/authority model. No synchronous cross-service IPC in the final publication transaction.
5. Parent verifies reservation exhaustion, interleaved ordinary pushes/publications, abort/expiry/death rollback, no premature ready visibility and allocation-free commit using actual owner methods/source, plus the approved private signed-x86 target scenarios. No host-only result closes CBX-03..05.

A reservation guard is an internal capacity claim, not a new wire capability. It cannot grant a launch edge, change a child ceiling or substitute for a valid approved receipt.

### Review-mandated invariants and compatibility

- Under LOG, enforce `entries.len()+publication_promises<=MAX_ENTRIES` and `entries.capacity()>=entries.len()+publication_promises`. Reserve space for all outstanding promises plus the new one. Ordinary commit cannot steal promised detailed-log capacity; preserve its aggregate-only fallback for ordinary entries when needed. Reserved commit consumes exactly one promise and appends/advances the aggregate together. Preserve the existing fixed-ring CellMeasure audit emission.
- Under each hart ready leaf lock and priority, enforce `capacity>=queued+publication_promises+selection_rollback_promises`. Selection removes a runnable task but retains a rollback promise until successful selection consumes it or rollback reinserts at the original FIFO index. Stealing does not hold SCHEDULER, so that lock alone cannot protect the vacancy. Every ordinary enqueue, direct steal destination and restore_selection path must honor both kinds of promises.
- Prove existing infallible wake/requeue headroom; a full bucket cannot drop a wake, assert/panic, consume a promise or hide infallible growth. Establish destination headroom before removing a steal victim; on refusal leave it queued. Preserve the actual batch rule `(stealable/2).max(1)`, non-RT eligibility and destination-tail order.
- Pin enqueue **and preemption notification** to the reservation's selected destination even if prepare/commit run on different harts or an RT secondary becomes online. Expose the pinned destination to the integrating scheduler or send notification through the same commit path. No hotplug/offline mechanism is added.
- Charge pending claims to the creator's exact live principal before success, separately from bounded canonical shared-buffer attribution. Abort/expiry/parent-or-issuer death refunds a pending claim once. Successful commit transfers its charge to the declared retained/live owner, never refunds through a consumed guard's Drop. Historical log/path charges remain attributed while retained after child exit; kernel-cell-zero attribution alone is not creator-quota evidence.
- Preserve the existing ready-map representation/consumer contract within this closure: `kernel/src/loader/manifest_section_tests.rs` directly snapshots `BTreeMap<u8,VecDeque<usize>>` and is **not admitted for edits**. A replacement that requires editing that consumer or a compatibility-only shim is not authorized; request another narrow amendment instead. Existing admitted atomic-publication snapshots must remain consistent. Internal reservation counters/guards do not add a second runnable-task list.

## Admission, rollback and unchanged gates

If approved, source closure becomes the original78 exact paths plus these3 files (81 total); all prior candidate/snapshot hashes remain historical immutable approval subjects, not rewritten to hide expansion. No additional selector/field is allocated. The current assistant remains single integrating owner, with one kernel publication serialization owner. Build/sign/image/VM execution stays in `/tmp/cellbox-phase02-zRWExI6S` under the existing private admission; canonical quarantine/guards and all other locks stay unchanged.

Rollback reverts only these reservation changes together with their slice-owned callers and stops/reaps only the owned private VM. Preserve unrelated dirty changes/evidence. Code reversal cannot undo already delivered guest IPC or writes inside disposable guest disks; no user/canonical disk or remote endpoint is authorized for writes. This amendment is not CP2, Phase02 acceptance or rollout approval.
