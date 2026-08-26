## Kernel `CAP_TABLE` already has a death-funnel, but not a generation key
**Verdict:** Kernel file capabilities are owner-checked and revoked on `Exit`/`ForceExit`, so they already fail closed on cell death; the remaining lifecycle weakness is that the table keys ownership by `CellId` only, not `(CellId,generation)`.
- [PROVEN] `OpenCap` resolves the caller's `cell_id`, allocates a `CAP_TABLE` entry, and `ReadCap`/`SeekCap`/`WriteCap`/`CloseCap` all verify that same owner before use or revoke.  
- [PROVEN] `ReadCap` parks the file object out of the table, does I/O outside the lock, then unparks it; concurrent access during the parked window is refused.  
- [PROVEN] `Exit` and `ForceExit` both call `CAP_TABLE.revoke_all_for(cell_id)` after `exit_task`, so kernel caps do not survive the death funnel.  
- [INFERRED] Because `CapEntry.owner` is `CellId` only, any future `CellId` reuse would rely entirely on that death-funnel staying complete; unlike VFS state, the table itself does not encode generation.
**Source:** [kernel/src/task/syscall.rs:2836](/home/dmin/cellos/kernel/src/task/syscall.rs:2836), [kernel/src/task/syscall.rs:2902](/home/dmin/cellos/kernel/src/task/syscall.rs:2902), [kernel/src/task/syscall.rs:3129](/home/dmin/cellos/kernel/src/task/syscall.rs:3129), [kernel/src/task/syscall.rs:2034](/home/dmin/cellos/kernel/src/task/syscall.rs:2034), [kernel/src/cell/cap_registry.rs:35](/home/dmin/cellos/kernel/src/cell/cap_registry.rs:35), [kernel/src/cell/cap_registry.rs:177](/home/dmin/cellos/kernel/src/cell/cap_registry.rs:177), [kernel/src/cell/cap_registry.rs:204](/home/dmin/cellos/kernel/src/cell/cap_registry.rs:204)

## VFS durable registries are already generation-scoped
**Verdict:** The VFS-side state that can outlive one syscall already uses the right principal shape: attested `Caller { cell, generation }`, not sender tid and not name/path hints.
- [PROVEN] `Caller` exists only from kernel attestation, treats same `CellId` plus different `generation` as a different principal, and refuses durable ownership when generation is absent.  
- [PROVEN] `HandleTable` and `PendingTable` both store `Caller`, compare it on every lookup/remove, and document generation as the barrier against successor-cell inheritance.  
- [PROVEN] `ReadAsync` records durable state only when `caller.may_own_state()` is true; `Poll`/`ReadGrant` re-authorize against the stored path before data moves.  
- [PROVEN] Spec 17 makes this normative: services holding open handles or pending reads must compare `generation` as well as `CellId`.
**Source:** [cells/services/vfs/src/caller.rs:15](/home/dmin/cellos/cells/services/vfs/src/caller.rs:15), [cells/services/vfs/src/handle_table.rs:19](/home/dmin/cellos/cells/services/vfs/src/handle_table.rs:19), [cells/services/vfs/src/pending.rs:26](/home/dmin/cellos/cells/services/vfs/src/pending.rs:26), [cells/services/vfs/src/dispatch.rs:161](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:161), [docs/specs/17-ipc-wire-contract.md:421](/home/dmin/cellos/docs/specs/17-ipc-wire-contract.md:421)

## Current revocation boundary is path-state and grant-state, not `DataPtr`
**Verdict:** Revocable VFS access can be made coherent for handles/pending reads/grants; `GetFile`/`DataPtr` is the hard stop because it hands out permanent SAS authority that no lifecycle hook can reclaim.
- [PROVEN] `GetFile` authorizes before resolution because the reply is a raw pointer that "cannot be taken back once handed out".  
- [PROVEN] Fast-IPC is forced to authorize identically, and Spec 17 says `GetFile` is not a valid Tier-2 rewrite target and must be removed or replaced before Layer B.  
- [PROVEN] Spec 18 says `DataPtr`-style raw pointers are unrepresentable across the Tier-2 boundary.  
- [INFERRED] Any revocable design therefore has to converge on handle/grant/CQ-mediated lifetimes; trying to make revocation work while retaining raw `DataPtr` is structurally false.
**Source:** [cells/services/vfs/src/dispatch.rs:55](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:55), [cells/services/vfs/src/main.rs:97](/home/dmin/cellos/cells/services/vfs/src/main.rs:97), [kernel/src/fast_ipc.rs:126](/home/dmin/cellos/kernel/src/fast_ipc.rs:126), [docs/specs/17-ipc-wire-contract.md:445](/home/dmin/cellos/docs/specs/17-ipc-wire-contract.md:445), [docs/specs/18-cell-trust-tiers.md:155](/home/dmin/cellos/docs/specs/18-cell-trust-tiers.md:155)

## Grant pinning/quarantine is the only proven frame-reuse barrier today
**Verdict:** For memory that can still be touched after the initiating syscall path, the existing fail-closed mechanism is explicit pinning plus quarantine-on-death plus driver acknowledgement before frame reuse.
- [PROVEN] `GrantFree` and `GrantUnregister` refuse teardown while the region overlaps a pin.  
- [PROVEN] `reap_grants_for_task` marks dead-task pins quarantined first, then withholds or frees grant frames; death is never blocked on the pin.  
- [PROVEN] `release_acked_frames` is the sole path that returns quarantined frames to the allocator after IOMMU cleanup.  
- [PROVEN] `alloc_grant_pages` zeroes reused frames before handoff, while `free_grant_pages` restores kernel identity mapping and deallocates only after the quarantine decision.
**Source:** [kernel/src/task/syscall.rs:208](/home/dmin/cellos/kernel/src/task/syscall.rs:208), [kernel/src/task/syscall.rs:232](/home/dmin/cellos/kernel/src/task/syscall.rs:232), [kernel/src/task/syscall.rs:347](/home/dmin/cellos/kernel/src/task/syscall.rs:347), [kernel/src/task/syscall.rs:4167](/home/dmin/cellos/kernel/src/task/syscall.rs:4167), [kernel/src/task/syscall.rs:4223](/home/dmin/cellos/kernel/src/task/syscall.rs:4223), [kernel/src/memory/pin.rs:4](/home/dmin/cellos/kernel/src/memory/pin.rs:4), [kernel/src/task/syscall.rs:96](/home/dmin/cellos/kernel/src/task/syscall.rs:96)

## `ReadGrant` and `ReadFileGrant` are safe today only under a synchronous contract
**Verdict:** The current grant-copy VFS arms are not lifecycle-safe by themselves; they are safe only because the service assumes the caller stays blocked until reply, which is exactly the contract Phase 07 flags as non-portable to cancellation/async.
- [PROVEN] `ReadGrant` copies into a shared grant and replies only after filling it, but it does not pin the caller's grant in the kernel pin registry.  
- [PROVEN] `ReadFileGrant`'s `unsafe` comment relies on "the caller's `ipc_call` blocks until we reply" as the safety argument.  
- [PROVEN] Phase 07 calls out this exact pattern as an async hazard: once futures become cancellable, that reasoning no longer protects the frame.  
- [INFERRED] For revocable VFS access, these paths must stay strictly synchronous/non-cancellable until they move onto the same pin/quarantine discipline as DMA and future async grant users.
**Source:** [cells/services/vfs/src/dispatch.rs:208](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:208), [cells/services/vfs/src/dispatch.rs:292](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:292), [docs/specs/17-ipc-wire-contract.md:421](/home/dmin/cellos/docs/specs/17-ipc-wire-contract.md:421), [.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:59](/home/dmin/cellos/.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:59)

## Completion queues show the right non-revocable-memory pattern
**Verdict:** The completion queue is the model to copy for revocable VFS lifecycle edges: kernel-owned sink, pre-reserved slot, deferred wake, no caller-controlled memory lifetime.
- [PROVEN] The queue is kernel-owned heap memory in the TCB, never a grant, specifically so a cell cannot free/unregister it under an in-flight operation.  
- [PROVEN] Slot reservation happens before submission, append takes only the queue leaf lock, and waking is deferred until `yield_cpu` after `SCHEDULER` is free.  
- [PROVEN] `exit_task` strips dead tasks' TIMER reservations into a deferred release list, and `yield_cpu` drains that list outside `SCHEDULER`.  
- [INFERRED] A revocable VFS design should treat "completion/close/revoke acknowledgement" like CQ slots, not like borrower-owned raw pointers.
**Source:** [kernel/src/task/tcb.rs:388](/home/dmin/cellos/kernel/src/task/tcb.rs:388), [kernel/src/task/completion.rs:1](/home/dmin/cellos/kernel/src/task/completion.rs:1), [kernel/src/task/completion_wait.rs:168](/home/dmin/cellos/kernel/src/task/completion_wait.rs:168), [kernel/src/task/scheduler.rs:495](/home/dmin/cellos/kernel/src/task/scheduler.rs:495), [kernel/src/task.rs:654](/home/dmin/cellos/kernel/src/task.rs:654)

## Proposed lifecycle invariants for `open/read/close/revoke/death/cancel` [EXPANDED]
**Verdict:** The minimal coherent state machine is `Unbound -> Open -> InFlight -> Closed/Reaped`, with fail-closed ownership at every edge and pin/quarantine only when memory may outlive the synchronous reply.
- [PROVEN] `Open` may create durable state only for an attested caller with nonzero generation; VFS already enforces this for pending reads/handles, and Spec 17 requires it for any service-held state.  
- [PROVEN] `Open -> InFlightRead` must re-check owner and current path policy, not trust open-time policy forever; VFS already does that in `Poll` and `ReadGrant`.  
- [PROVEN] `Close/Revoke` must be owner-only and indistinguishable from unknown-handle failure to non-owners; both VFS tables already implement this, and kernel `CloseCap` verifies owner before revoke.  
- [PROVEN] `CellDeath -> Reaped` must run the terminal funnel: `exit_task`, revoke kernel caps, clear service hooks, reap grants, quarantine any pinned frames, then release only after driver ack.  
- [INFERRED] `Cancellation` must be defined as either "operation never obtained revocable memory outside the reply window" or "memory is pinned/quarantined until acknowledged"; there is no safe middle state.  
- [INFERRED] The Async Pinning Registry should remain a substrate, not a policy engine: it should protect only operations whose target memory can outlive the synchronous reply. Do not expand it to raw `DataPtr`, because raw `DataPtr` has no revocation point.
**Source:** [cells/services/vfs/src/caller.rs:42](/home/dmin/cellos/cells/services/vfs/src/caller.rs:42), [cells/services/vfs/src/dispatch.rs:178](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:178), [cells/services/vfs/src/handle_table.rs:77](/home/dmin/cellos/cells/services/vfs/src/handle_table.rs:77), [kernel/src/task/syscall.rs:2034](/home/dmin/cellos/kernel/src/task/syscall.rs:2034), [kernel/src/task/syscall.rs:232](/home/dmin/cellos/kernel/src/task/syscall.rs:232), [.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:95](/home/dmin/cellos/.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:95)

## Negative tests the lifecycle must pass
**Verdict:** The decisive tests are not throughput tests; they are inheritance, stale-owner, and frame-reuse tests that prove every terminal edge fails closed.
- [PROVEN] Existing VFS unit tests already cover "same `CellId`, different generation" refusing predecessor handles/pending reads; keep that shape for every new durable registry.  
- [INFERRED] Add one death-funnel test per durable object type: open/read in progress -> `Exit`/`ForceExit`/fault -> successor cell under same path or same `CellId` must observe "unknown/denied", never inherited state.  
- [INFERRED] Add one quarantine test per pinned data path: owner dies before acknowledgement -> frames stay withheld; after acknowledgement -> exactly then they may be reallocated.  
- [INFERRED] Add one cancellation/refusal test per revocable read path: close/revoke/cancel before completion must produce either no data or an explicit error, never a silent short success and never a post-reuse write.
**Source:** [cells/services/vfs/src/handle_table.rs:170](/home/dmin/cellos/cells/services/vfs/src/handle_table.rs:170), [cells/services/vfs/src/pending.rs:165](/home/dmin/cellos/cells/services/vfs/src/pending.rs:165), [kernel/src/memory/pin.rs:23](/home/dmin/cellos/kernel/src/memory/pin.rs:23), [kernel/src/task/syscall.rs:355](/home/dmin/cellos/kernel/src/task/syscall.rs:355)
