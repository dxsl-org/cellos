# Phase 03 Recommended Semantic Bridge

## Recommendation

Approve a narrow semantic extension of existing syscalls, with no new syscall
number, VFS request/response variant, manifest bit, or `libs/api` edit.

### 1. Per-request VFS grant-copy lease

When the kernel-registered VFS task calls existing `GrantSlice` while processing
an attested request, the kernel may resolve only a grant owned by that current
sender and shared to VFS. Before returning the address, it records an exact
lease keyed by `(vfs_tid, sender_tid, grant_id, recv_generation)` and pins the
covered frames.

The kernel releases exactly that lease after the matching `Send` terminates,
including send failure because the caller died. Owner death tombstones the
grant and quarantines its pinned frames; VFS death releases every lease held by
that VFS task because no service instruction can touch the addresses afterward.
A stale send/generation cannot release a newer lease. Table exhaustion, missing
attested request context, wrong owner/grantee, duplicate unresolved lease, and
release mismatch fail closed.

This reuses the existing `GrantSlice`, `Send`, grant reaper, and pin/quarantine
shape, but it changes their semantics and therefore needs the explicit semantic
approval requested here. It must not use owner-wide `pin::acknowledge(tid)`.

### 2. Current-caller-cell-only VFS death watch

Permit the kernel-registered VFS task to call existing `NotifyOnExit` without
`SpawnCap` only for the owning cell task of the task in its kernel-maintained
`current_caller` slot. The kernel derives that owner from the current sender's
`Task.cell_id`; request bytes do not choose it. Threads share their cell's
identity and generation, while durable VFS state is owned by that same
`Caller { cell, generation }`. VFS gains no ability to watch or control an
arbitrary TID.

VFS records `cell_owner_tid -> Caller { cell, generation }` only after an
attested request creates durable state. All threads of that cell converge on
the same owner watch. VFS subscribes after dropping VFS locks and before
sending the open response. Existing `NotifyOnExit` already converts the
subscribe-after-death race into a queued synthetic death. A no-attestation
receive is treated as a death event only when its returned TID exists in the
local watched map; otherwise it is denied/ignored fail-closed. Owner death
purges all matching file/dir handles and pending reads for exactly the recorded
generation. A worker-thread exit does not purge cell-owned state. VFS restart
begins with empty tables, so no subscription recovery is required for
pre-restart service state.

This is a service-specific authorization semantic change, not broad SpawnCap.
It needs the explicit semantic approval requested here. Phase 03 must also
prove that every Cell-terminal path terminates or notifies the owning cell task;
if a terminal path can leave that owner alive while declaring the Cell dead,
this bridge fails its matrix and implementation stops.

## Lock and terminal order

1. Receive establishes `current_caller` and a monotonically increasing receive
   generation under scheduler state.
2. `GrantSlice` validates grant tables, records the exact leaf pin/lease, then
   returns the address; it holds no scheduler, frame allocator, or VFS lock.
3. VFS copies while the lease protects frame reuse.
4. Matching `Send` ends the lease after the last possible VFS access. If owner
   death already quarantined frames, exact lease release returns only those
   frames attributable to that lease.
5. Terminal cleanup handles VFS-held leases before task memory can disappear;
   owner cleanup quarantines before grant-table removal and frame reuse.
6. VFS calls `NotifyOnExit` only after releasing its state lock; death-event
   cleanup reacquires only VFS state and makes no kernel call while holding it.

Grant/pin order remains grant table -> pin/lease leaf -> all released -> frame
allocator -> kernel root. Scheduler is never held across frame release.

## Approval boundary

This checkpoint authorizes only the two semantic extensions above and their
tests. It does not authorize a syscall number, wire format, manifest,
`libs/api`, or `libs/types` change; broad VFS `SpawnCap`; Tier 2; async DMA;
`RecvScatter`; generic reactor; or SMP work. If implementation cannot satisfy
the design without one of those changes, stop and request a new checkpoint.
