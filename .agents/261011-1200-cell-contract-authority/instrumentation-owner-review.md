# P01 — Instrumentation insertion-point review

Status: planning-only source map; no instrumentation implemented or new runtime qualification. Read with [resource contract](resource-attribution-contract.md). Normal source/ABI/admission unchanged. Two independent reviewers covered publication identity and resource/reclamation; findings were adjudicated and incorporated below.

## Parent source map

| Boundary | Source owner | Capture requirement / constraint |
|---|---|---|
| Prepared task publication | `kernel/src/task/launch.rs::publish_prepared` | Under SCHEDULER, every fallible preflight precedes task insertion; task/owner/routes/measurement commit precede push_ready. Fixed publication record must commit before push_ready without allocating, formatting, hashing or adding subsystem locks. Capture caller identity from live validated state, not a later TID lookup |
| Artifact evidence | `publish_prepared:254-256`, `kernel/src/measurement_log.rs::commit_staged` | Existing staged path/digest may supply identity metadata, but measurement log is not a complete incarnation trace: u32TID, pathString/Vec growth and best-effort dropped entries; aggregate continues when entry omitted. Do not modify attestation semantics or call it lossless resource evidence |
| Root terminal state | `kernel/src/task/scheduler.rs::begin_root_retirement` | Live→Retiring is a lifetime transition, not frames-reclaimed. Retain CellOwner tuple in trace |
| Worker zombie extraction | `scheduler.rs::take_reapable_zombies`; `kernel/src/task.rs:1318-1332` | Do not declare release on extraction. Task drop occurs outside SCHEDULER, then waiter publication under SCHEDULER. Capture immutable identity before drop; a post-drop TID lookup cannot recover it reliably |
| Completion release | `kernel/src/task.rs:1334-1348` | Queue slot release happens outside SCHEDULER after pending release extraction. Record actual release outcome; extraction alone is not release or Cell-settled |
| Grant/DMA cleanup | `kernel/src/task.rs:1350-1363` | DMA retry and pending grant reap are separate from zombie stack drop. Preserve each obligation's exact former owner; no inferred release from task absence |
| Root generation cleanup | `kernel/src/task.rs:1372-1389` | Grants/VFS leases, zombies, waiter wake, capability/resource registry releases occur in order. Resource trace must not move cleanup under SCHEDULER or merge all boundaries into terminal |
| Owner slot/Cell quota admission release | `task.rs:1390-1424`, `scheduler.rs::finish_root_retirement` | Exact Retiring owner must match before slot clears; quota deregister only on success. Emit separate slot-release/quota-release records with saved owner. Failure means retained, never settled |

| Allocation observation | Source owner | Capture requirement / constraint |
|---|---|---|
| Kernel heap committed object usage | `kernel/src/memory/heap.rs::usage` | Existing used/free read holds heap lock. Call only from safe observer stage; never call recursively inside GlobalAlloc or while adding scheduler→heap order. This is allocator usage, not exact per-generation object ownership |
| Charge then actual heap allocation | `heap.rs::QuotaAlloc::alloc:118-144` | Quota charge precedes heap result; null allocation refunds. Charge/reservation high-water and successful allocation peak are different metrics; no success event for null |
| Heap deallocation | `heap.rs::QuotaAlloc::dealloc:147-151` | Refund uses current execution Cell, not stored allocation-incarnation owner. Do not use current hart/Cell to infer historical allocation identity; exact attribution requires reviewed allocation-owner metadata or narrower object-owner instrumentation |
| Current cleanup backlog | `scheduler.rs::publication_snapshot_counters` | Counts only pending grant/VFS-holder/completion releases plus last sweep tick, under scheduler. Not a complete pending-root/zombie/DMA/deferred obligation vector or proof of per-workload settlement |


Existing retirement code deliberately drops resources outside SCHEDULER because stack unmapping follows KERNEL_ROOT-related ordering. Observation may copy fixed metadata while an owner lock is held, but cannot add a new lock dependency from that critical section. Consumer drains records at safe points; never walks live tasks after freeing them to reconstruct identities.

## Scope and prerequisites

- Confirm this ELF publication path covers every selected measurement launch before instrumenting; root/thread/hotswap/boot-only paths must be explicitly included or excluded. No universal coverage inferred from one hook.
- Configured async baseline remains scenario evidence; no explicit spawn/incarnation mapping retroactively added to old serial.
- Preserve artifact digest staging before critical publication. Captured path representation must be bounded and unambiguous (or use an artifact identity table prevalidated outside critical sections); do not hash ELF or allocate Strings in trace insertion.
- Existing measurement log's256entry limit is independent of proposed private256event trace. Boot selftests generate many publications; future trace needs an explicit scoped arm/baseline/disarm protocol or accounting of every boot event. Do not silently filter workload identities by name.
- No public inspection ABI or permission widening; actual trace/coherence layout remains a reviewed implementation prerequisite, not a completed feature.

## Adjudication

Two source-backed reviews identified eleven findings; deduplicated into five Major design gaps and one Minor accounting correction. No Critical claim or full-plan red-team certification. All accepted as planning mitigations; no runtime fix, source approval or gate release.

| Group | Evidence | Resolution |
|---|---|---|
| Major — subject/route/attempt identity | `scheduler.rs::spawn_thread` shares root identity; syscall Path/Pinned converge; Elf uses caller path hint; governed preflight can fail before publication | Add subject TID/root-role, route/source kind and private ingress attempt ID. Preserve digest as actual byte identity; path hint is not filesystem attestation. Reviewer premise that parked children are worker threads is rejected: selected workload is root-only. The wider task-identity gap is accepted |
| Major — boot/run identity | TID and generation restart on boot; no measurement boot source defined | Host assigns unique capture header before VM launch, binds stream/artifact/command; kernel supplies local facts only. Reject any claim host boot label is kernel-attested freshness |
| Major — generation lost during cleanup/retention | Worker dropped before TID-only grant/DMA queues; deferred items lack incarnation; queue-full retention can bypass existing quarantine/abandoned counts | Capture identity before removal and propagate through private obligation tracking; map every pending root/zombie/VFS-context/holder/completion/grant/DMA/deferred outcome. Observe pre-loss Full/abandoned decisions. Unknown blocks settled; no ordering/authority change |
| Major — heap ownership and actual peak | `heap.rs` current-Cell charge/refund lacks historical allocation owner; LockedHeap aligned used counter changes under its internal lock | First actual heap metric stays global. Do not promise per-generation actual heap without owner metadata. Update actual high-water within existing heap critical section, no added trace lock; requested bytes and physical frame maxima are separate metrics |
| Major — capture scope/capacity | Best-effort measurement log allocates/drops; private256event proposal has no event fan-out/retry proof |256is candidate, not validated bound. Define arm-before-pre/disarm-after-outcome and prove finite record formula before execution; bounded maxima counters need not emit full row per allocation. Overflow invalidates observation without altering kernel behavior |
| Minor — leaked stack guard attribution | `scheduler.rs:891-902`, `tcb.rs:891-899` usable pages differ from normal `Stack::drop` pages+guard_pages | Include allocated/guard frames in proposed observation, not existing usable-only leak counter. No correction to unrelated runtime code in this planning pass |

## Implementation-entry checklist

- Select exact root-only measurement paths and copy-only event fields; carry private attempt/route data before fallible load.
- Pin capture/header/arm protocol, event-capacity formula and all cleanup obligation owners. Preserve identity after task/Cell slot disappears.
- Keep actual heap observation global unless an allocation-owner mechanism is reviewed; demonstrate counter update linearization without new lock edges.
- Explicitly include retention-before-provenance-loss and guard frames. Verify invalid captures cannot suppress errors, block publication/reclamation, replay launches or free unknown resources.
- Review real proposed implementation diff/lock graph and then exercise isolated workload; this source map does not approve unshown trace layout/code. All10phases/38locks/checkpoints remain unchanged.
