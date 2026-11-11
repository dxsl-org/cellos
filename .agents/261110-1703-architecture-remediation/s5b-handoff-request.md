# S5b handoff request — one IPC operation model (phase 11)

**Date:** 2026-11-11 · **Plan:** [plan.md](plan.md) · **Prior slice:** S5 Phase 10 closed 2026-11-11 (owner). S1 is still open, only for the post-CI untrack and the signing-blocked release item.

## 1. Scope (measured on `ac2b01a4f`)

Phase 11 as planned: exact-operation IPC with `WaitCompletion` becomes the one request/reply model. It is the largest behaviour change in the plan and the first slice since Phase 08 to change `libs/api`, so Law-1 CP1/CP2 applies.

| Surface | Today |
|---|---|
| Blocking `sys_send` in cells | 212 sites in 83 files; `Sending` has no deadline |
| Service lookup | 153 tid-only `lookup_service` sites, 42 bound |
| `fast_ipc` | 55 kernel references; self-described scaffolding that still gates generic paths |
| Parked `TaskState` variants | 11 (`Sleeping`, `Sending`, `Recv`, `PipeRead/Write`, `FutexWait`, `Waiting`, `Polling`, `WaitEvent`, `WaitCompletion`, `WaitIpc`, `WaitIrq`) |
| Kernel→VFS `Rename` | loops on VFS with no expiry (`task/syscall/fs.rs:519`) |
| SDK outcomes | `RemoteEndpoint::call` returns `NotSupported`; local outcomes collapse to `ViError::IO` |

The plan's eight steps run in this order:

1. Census + CP1.
2. Typed outcome `CellMethod` SDK surface.
3. Bounded send default, `IpcReply` replies, and the `Rename` fix.
4. Registration flags; delete `fast_ipc`.
5. Fold the waits into completion sources and collapse the parked states.
6. Remove dead arms and fields; mark the opcodes reserved.
7. Bound lookup only.
8. Spec 17 + CP2.

## 2. Staging: proposed

Step 1 is read-only plus a document: classify all 212 send sites as reply, event or request, all lookup sites, and every `TaskState` reader in `libs/api` and `cells/`, then write `cp1-ipc-model.md`. CP1 states the exact `libs/api` deltas, the opcodes reserved, the migration census and the negative cases.

**Nothing in `libs/api`, `libs/ostd` or `cells/` changes until you confirm CP1.** Steps 2–8 follow under that confirmation, one commit per step. Each migration step is split per directory (`libs/ostd` clients, then services, then drivers, then apps).

## 3. File set (file-bounded exception, after CP1)

- **Modify:**
  - `libs/api/src/abi/syscall.rs`, `libs/api/src/services/ipc.rs`, `libs/api/abi/syscalls.toml` (reserved opcodes)
  - `libs/ostd/src/{ipc,service,cluster_endpoint,syscall}.rs`, `libs/ostd/src/clients/**`
  - kernel IPC: `task/async_ipc.rs`, `task/completion*.rs`, `task/tcb.rs`, `sched/scheduler.rs`, `task/syscall/{ipc,fs,decode,misc}.rs`, `task.rs`, `cell/service_registry.rs`
  - every `sys_send` and tid-lookup caller under `cells/services`, `cells/drivers` and `cells/apps`
  - `docs/specs/17-ipc-wire-contract.md`
  - generated headers and golden dump; Law-1 digest scripts, re-pinned under CP2
- **Create:** `cp1-ipc-model.md`, `cp2-ipc-model.md`, and the two new x86 fixtures (non-receiving client; VFS killed mid-`Rename`)
- **Delete:** `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs`, `cells/services/vfs/src/fast_handler.rs`, the dead `Task` fields, the `spawn_from_file` stub, the dummy waker and `TaskState::Polling`
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`

## 4. Behaviour changes to expect

- **SDK `send` is bounded by default:** a deadline, or `Busy`. Event-only posts are an explicit allowlisted path.
- **Service replies use `IpcReply`;** a client that never receives no longer parks a service.
- **Kernel `Rename` returns an error within its deadline** if VFS dies.
- **Retired opcodes stay reserved and are never reused.** The outcome enum becomes public compatibility surface (irreversible).

## 5. Conflicts

- **The UI session** (compositor, viui, `ostd::display`) committed its work. Steps 2, 3 and 7 touch `libs/ostd` and every cell, compositor included. Each such commit is rebased on whatever that session has committed, and none of its uncommitted files is staged.
- **The iGPU DMA-export slice** keeps waiting.

## 6. Checkpoint

Phase 11 is a checkpoint phase. It closes with:
- the two new fixtures, which fail on the tree before this phase;
- the existing C2C evidence lanes unchanged;
- the full local lane set on 3 arches, `x86-c2c` and the two-node lane;
- digest scripts re-pinned under CP2;
- an independent review.

## 7. Questions to the owner

1. Grant S5b staged as in §2: step 1 (census + CP1) now, steps 2–8 only after your CP1 confirmation.
2. Kernel `Rename` (requirement c): keep it in the kernel as an exact operation with expiry (smaller change), or move it to userspace (removes the kernel→VFS call entirely)?
