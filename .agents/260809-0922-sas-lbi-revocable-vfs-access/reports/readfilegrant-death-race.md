# ReadFileGrant caller-death race

## Verdict

`ReadFileGrant` is not a safe Phase 02 migration primitive yet. The synchronous
IPC convention does not establish a kernel-enforced lifetime for the resolved
grant pointer. Phase 02 must not add callers until Phase 03 supplies a scoped
pin/lease and a completion release point, or an equivalently safe copy path.

## Evidence

- `cells/services/vfs/src/dispatch.rs:292-314` authorizes the caller, resolves
  the grant to a raw pointer, reads the file, and copies later. The safety
  comment relies on the caller remaining blocked, not on a kernel-held lease.
- `kernel/src/task/syscall.rs:4128-4165` implements `GrantSlice` as a lookup that
  returns `base` and `size`; it neither pins the region nor returns a revocable
  operation token.
- `kernel/src/task/syscall.rs:245-320` removes grants owned by a dying task and
  frees their frames unless the existing pin registry says they are held.
- `kernel/src/task/scheduler.rs:486-517` is the terminal scheduler funnel for
  clean exit, force exit, faults, watchdogs, heartbeat death, and hot-swap
  retirement. Caller blocking therefore does not prevent an independent death
  path from reaping its grant while VFS is preempted.
- `kernel/src/task/syscall.rs:347-365` releases quarantined frames through
  `pin::acknowledge(tid)` after IOMMU cleanup. That acknowledgement is keyed by
  task and is not a per-VFS-operation completion primitive.

## Minimal safety contract

1. VFS must obtain a kernel-validated hold before it receives a usable address.
2. Owner death must quarantine held frames instead of freeing or reallocating
   them.
3. VFS must release the exact hold only after the final copy can no longer
   access the frame; cancellation and error paths must release or quarantine
   fail-closed.
4. A stale completion must not release a newer operation's hold; the identity
   therefore needs an operation-scoped generation/token, not only a TID.
5. The design must define lock order across grant tables, the pin/lease table,
   frame allocation, and scheduler/death cleanup before implementation.

## Planning consequence

- Do not migrate shell, Lua, WASM, or other clients to `ReadFileGrant` in Phase
  02.
- Make Phase 03 a prerequisite of Phase 02.
- Treat any new syscall semantics, wire field, manifest authority, or broadened
  `NotifyOnExit` authorization as a separate approval checkpoint under the
  plan's Law 1 boundary.
