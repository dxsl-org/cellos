# Phase 03 Decision Package: Scoped VFS Frame Lifetime

## Verdict Needed

Choose the minimal scoped lifetime for VFS grant-copy before Phase 02 resumes. Do not mark approval in the plan until the user explicitly approves the selected option and its checkpoint boundary.

## Evidence

- `ReadFileGrant` resolves the caller grant pointer with `sys_grant_slice_with_len` and writes with `copy_nonoverlapping`; VFS owns no visible pin/lease around the copy (`cells/services/vfs/src/dispatch.rs:298`, `cells/services/vfs/src/dispatch.rs:309`).
- The current VFS safety comment assumes `ipc_call` blocking prevents grant free, but it does not address caller death or terminal reaping during the copy window (`cells/services/vfs/src/dispatch.rs:305`).
- Kernel grant reaping already handles owner/grantee death and pin quarantine, but that lifecycle is entered by task death/reaper paths, not by VFS copy-out (`kernel/src/task/syscall.rs:245`, `kernel/src/task.rs:680`).
- `GrantFree` and `GrantUnregister` already refuse pinned regions, showing a reusable enforcement shape, but current pin acknowledgement is not an exact per-copy release. Any service-side pin adaptation needs operation-scoped generation/token semantics before it is safe (`kernel/src/task/syscall.rs:4167`, `kernel/src/task/syscall.rs:4223`).
- VFS durable state uses generation containment and lazy purge; it prevents successor inheritance but does not immediately clean resources at death (`cells/services/vfs/src/caller.rs:17`, `cells/services/vfs/src/dirs/lifecycle.rs:61`).
- `NotifyOnExit` exists as syscall 204 and is explicitly `SpawnCap`-gated (`libs/api/src/abi/syscall.rs:216`, `kernel/src/task/syscall.rs:2281`). A VFS-private "watch held resources" rule is rejected because the kernel cannot see VFS ownership tables.

## Options

### Option A: Adapt Existing Pin Registry

Design: adapt the existing pin registry shape for VFS copy, but add operation-scoped generation/token semantics and exact release for one `ReadFileGrant` copy. Owner-wide/all-holds acknowledgement is not acceptable for this path.

Data flow: caller shares grant -> VFS requests scoped pin token for `(caller, grant, len, operation_generation)` -> VFS copies -> VFS releases exactly that token -> reply.

Checkpoint boundary: separate semantic approval is always required for userspace VFS to acquire this authority unless the design is wholly an existing kernel-mediated path with no new authority semantics. A new Law 1 checkpoint is additionally required for any `libs/api/`, syscall number, wire, or manifest edit.

Risk: Medium x Critical. The existing pin code is DMA-oriented and owner-wide ack is too coarse; adapting it for service copy can create lock-order, stale-token, or authority confusion. Mitigation: prove lock order, owner/grantee checks, per-operation token release, death path, release-on-VFS-death, and refusal semantics before Phase 02.

Rollback: remove the internal pin-token helper and keep Phase 02 blocked. Irreversible part: none if no ABI surface is added.

### Option B: New Scoped Lease/Token/Ack

Design: create an explicit `GrantCopyLease` or equivalent token for VFS copy-out. This may subsume Option A if adapting the existing pin table is cleaner than adding a parallel table. The kernel tombstones the grant on owner death, lease close, timeout, or VFS provider death; frame reuse waits for exact lease ack.

Data flow: caller shares grant -> VFS obtains lease token -> VFS copies -> VFS acks/ends lease -> kernel allows free/reuse.

Checkpoint boundary: requires a new checkpoint if it introduces any public request/response, syscall, manifest bit, or `libs/api`/`libs/types` delta. If implemented wholly inside kernel/VFS with existing syscall 204-style authority, it still needs separate lifecycle-bridge approval.

Risk: Medium x High. More explicit and auditable than Option A, but likely larger than needed and easier to leak tokens. Mitigation: one-shot tokens, forced terminal cleanup helper, timeout/fault tests, no async DMA/reactor scope.

Rollback: remove token table and keep existing grant behavior. Irreversible part: any ratified ABI docs or public discriminants require reserved-slot compatibility handling.

### Option C: Kernel-Mediated Copy

Design: stop exposing service-held grant pointers to VFS for copy-out. VFS returns/streams owned bytes to the kernel or asks the kernel to copy from VFS-owned source into the caller grant under scheduler-held lifetime checks.

Data flow: caller requests read -> VFS resolves file into owned bytes or chunk -> kernel validates destination grant owner/live state -> kernel copies -> reply.

Checkpoint boundary: feasible within current confirmations only if it reuses existing syscall/wire and remains internal. Any new copy syscall, `VfsRequest`, wire format, manifest bit, or syscall number needs a new explicit checkpoint.

Risk: Low x Critical if feasible; High x Medium feasibility risk because current VFS dispatch is userspace/service-side and the existing wire returns `GrantDone`, not owned full-file bytes. Mitigation: prototype design only in Phase 03; stop if it needs reactor, async DMA, `RecvScatter`, SMP, or public ABI expansion.

Rollback: keep VFS service-side copy blocked. Irreversible part: same as Option B for public ABI/doc ratification.

## Terminal Cleanup Helper

Minimum contract: one helper or mechanically proven equivalent must run before resource reuse for Exit, ForceExit, fault, watchdog, hot-swap, caller death during VFS copy, VFS death/restart, and cancellation. It must cover kernel caps, grant tables, pin/quarantine/ack state, VFS dir/file handles, pending reads, fast state, and death subscriptions.

Hard rules: no VFS locks held across kernel syscalls; no terminal path may rely only on successor-generation denial; fail-closed successor denial is containment, not cleanup.

## VFS Death Subscription / NotifyOnExit Authorization

Rejected shape: a VFS-private held-resource watch is not enforceable by the kernel because those ownership tables are private to VFS.

Allowed alternatives:

1. Supervisor bridge: an existing SpawnCap-holding supervisor watches cells and delivers death events to VFS. Accept only if every owner/provider, restart, queue overflow, and VFS restart case is proven.
2. Kernel-visible registry/service-specific death delivery: VFS registers kernel-visible ownership/interest records through a separately approved authority path, and the kernel delivers death events from those records. This must not become broad SpawnCap.

Stop if neither alternative is provable without broad VFS SpawnCap or unapproved syscall/wire/manifest changes. In that case, Phase 02 and Phase 04 remain blocked.

## Minimal Checkpoint Proposal

Ask for exactly these approvals, and no broader authority:

1. Scoped grant-copy lifetime choice:
   - Option A/B require a separate semantic checkpoint for operation-scoped token/lease authority.
   - Option C may avoid that semantic checkpoint only if it is wholly existing kernel-mediated behavior; otherwise it needs the same checkpoint.
   - Any `libs/api`, `libs/types`, syscall number, wire, or manifest edit requires a new Law 1/public-interface checkpoint.
2. Death delivery choice:
   - Supervisor bridge with proof matrix, or
   - separately approved kernel-visible registry/service-specific death delivery.
   - Explicitly reject broad VFS SpawnCap and VFS-private ownership watch authorization.

Stop criteria: no chosen lifetime, any Tier 2/per-domain page-table work, async DMA, reactor, SMP, syscall number/wire/manifest edit, or `libs/api`/`libs/types` change outside the existing confirmations.
