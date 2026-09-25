---
title: "Follow-on proposal: C thread and process adapters"
status: completed
priority: P2
depends_on: ["portability-program/06", "portability-program/07"]
---

# C thread and process adapter proposal

## Problem

Phase 07 supplied a real class-A Tier-2 C reference port, but the requested class-B and class-C
ports cannot be represented by the published native ABI:

- `sys_spawn(entry, arg)` creates an in-cell task and inherits its cell/domain, but there is no
  reviewed C `pthread_*` ABI, thread lifecycle wrapper, or C TLS runtime.
- `sys_spawn_from_path` already moves a caller task's staged argv into the kernel launch request,
  but the old storage shared a predictable generic state-stash namespace. An arbitrary caller
  could address another task's derived key; rejected spawn preflight also left caller argv staged.

The carrier must be hardened before a C adapter publishes it. This is kernel correctness work, not
a new ambient path or process authority.

## Existing constraints

| Existing facility | Reusable fact | Consequence |
|---|---|---|
| `Spawn` (5) | Creates an in-cell task at `(entry, arg)`; kernel prepares a user stack and inherits the parent cell/domain. | A C start trampoline can create a thread without a new process. |
| `SetTlsBase` (9) | Self-only task TLS register install; kernel does not dereference or allocate the block. | The C runtime must own template, per-thread allocation, alignment, and destructor policy. |
| `FutexWait`/`FutexWake` (17/18) | Domain-keyed wait/wake over caller-owned aligned words. | Mutex and condition-variable parking can remain userspace. |
| `Pipe*` (19, 22–25) | Endpoint ownership and blocking stream are established. | Child stdio redirection can use real pipes only after a child I/O contract exists. |
| `SpawnFromPath` + `set_spawn_argv` | The kernel takes a caller-private argv slot into `SpawnRequest` and publishes it to the allocated child before ready. | The carrier is the required atomic association once its storage is private, keyed by full TID, and cleared on every launch attempt. |
| launch profiles | Caller-target edges are reviewed and capability ceilings exact. | No C API may turn a string into ambient spawn authority. |

## Proposed phases

### P1 — C thread runtime contract

**Status:** completed 2026-09-25 at the RV64 QEMU ceiling. The public
`cellos_pthread.h` and C implementation cover `pthread_create`, one-shot
`pthread_join`, non-recursive mutexes, and condition variables. The Tier-2
`c-pthread` witness completed two coordinated workers and 32 immediate
create/join reuse cycles (`C-PTHREAD-QEMU: PASS`). It does not claim C
`__thread`, TLS destructors, cancellation, detachment, or a full POSIX
threading personality.

**Scope:** a narrow C implementation library for `pthread_create`,
`pthread_join`, `pthread_mutex_*`, and condition-variable operations.

1. Specify ABI-visible C types, ownership, cancellation policy (initially unsupported), error
   codes, and which common pthread calls deliberately return `ENOSYS`.
2. Implement a start trampoline over `sys_spawn`; join state is private shared cell memory and
   all wait/notify transitions use futexes with lost-wakeup tests.
3. Add a C TLS runtime only after the ELF/TLS-template ownership decision is explicit. Do not
   advertise `__thread` or thread-local destructors before the template and per-task block
   placement are proven on RV64 and AArch64.
4. Add a two-thread Tier-2 C witness: mutex counter, condition hand-off, join, timeout, and
   thread exit. It must prove an isolated peer cannot wake its futex key.

**Exit criterion:** C source builds with the porting toolchain, uses `pthread_create`/`join` and a
mutex/condition pair, and passes RV64 QEMU in a Tier-2 cell. A separate TLS witness is required
before claiming `__thread` support.

### P2 — C child-process adapter contract

**Status:** completed 2026-09-25 at the RV64 QEMU ceiling. `libs/port-platform/include/cellos_spawn.h`
and `libs/port-platform/cellos_spawn.c` publish `cellos_spawn` / `cellos_child_wait` /
`cellos_spawn_argv_raw` / `cellos_spawn_argv_split` / `cellos_spawn_receive_grants`. The Tier-2
`c-spawn` witness launches `/bin/c-spawn-child` over the reviewed ELF edge, delivers a two-item
command line (one item containing a space), grants a pipe write endpoint, reads the child's ordered
192-byte report to exact EOF, collects the child's status through `Wait` (published 42), denies an
unreviewed target, refuses an over-long command line client-side, and proves a denied launch left no
staged command line (`scripts/qemu-c-spawn.sh`: `C-SPAWN-QEMU: PASS`, harts 1 and 2). It does not
claim `fork`, `exec`, `posix_spawn`, sessions, job control, signals, or path lookup.

**Two platform facts this phase established:**

1. The kernel drives no block hardware on QEMU (G2 loader redesign), so the raw `SpawnFromPath`
   route cannot resolve a disk-installed cell: the cell store belongs to the VFS service. The
   adapter therefore composes the reviewed **ELF** edge with a caller-fetched grant, exactly as
   `ostd::sys_spawn_from_path` does, and keeps the path form for boards where the kernel block
   device exists. Both forms resolve the same reviewed row and neither can widen authority.
2. An exited cell root leaves the task table quickly, so `Wait` publishes a child's status only to a
   waiter registered before the child is collected. `cellos_child_wait` therefore reports
   `CELLOS_SPAWN_EREAPED` (terminal, status no longer published) rather than treating a failed wait
   as an error, and a launcher that needs the status waits promptly or has the child report it over a
   granted endpoint. The witness proves both channels.

**Carrier change:** the staged command line moved from a keyed kernel map to two task-local fields
(`Task::staged_argv`, `Task::inherited_argv`). The keyed map allowed a *different* task's
"failed-launch" cleanup to address a child's pending command line; task-local state makes that
unreachable by construction. `StateStash`/`StateRestore` on the reserved argv key and the
`StagedSpawnArgvCleanup` contract are unchanged in observable behavior.

**Scope:** a narrow `cellos_spawn.h`; it is not `fork`, `exec`, or a POSIX process personality.

1. Reuse the kernel's atomic per-task argv carrier: a private full-TID map moves bytes from the
   calling task into exactly one `SpawnRequest`, then to the allocated child before it is runnable.
   Generic `StateStash` operations cannot address this map. A failed external-launch attempt clears
   staged argv before returning.
2. Define a narrow C wrapper with a fixed target, a NUL-safe UTF-8 argv vector encoded as
   `\0argv1\0` plus NUL-terminated items, and a total payload limit of 512 bytes. The wrapper stages
   and launches synchronously from one task; it is not a new kernel syscall.
3. Preserve launch-profile authorization: each C parent/child edge is an explicit reviewed profile
   row with a bounded child capability ceiling. Arbitrary filesystem paths are rejected.
4. Define pipe inheritance as explicit `PipeShare` grants performed before child start; do not
   smuggle raw handles through argv. Child code receives only endpoint tokens it was granted.
5. Define wait/exit behavior from `Wait` or a capability-gated notification mechanism. No signal
   emulation, `waitpid` process groups, or shell-compatible job-control scope.
6. Add a C utility witness that launches one fixed child, streams ordered data over a pipe, passes
   argv, observes the child status, and proves an unreviewed target is denied.

**Exit criterion:** the C witness passes RV64 QEMU in independent Tier-2 domains, and a negative
launch-edge test proves arbitrary targets/argv cannot expand authority.

### P3 — Reference-port closure

**Status:** completed 2026-09-25. The closure record lives in
[phase-07 § Reference-port closure](./phase-07-reference-ports-and-cost.md); phase 07 is now
`completed`. Raw QEMU logs are published as `docs/evidence/c-pthread-qemu.{log,txt}` and
`docs/evidence/c-spawn-harts{1,2}-qemu.{log,txt}`, each with its Tier-2 admission marker:
`C-PTHREAD-QEMU: PASS` on harts 1 (25,912 B ELF) and `C-SPAWN-QEMU: PASS` on harts 1 and 2
(launcher 43,944 B, child 27,904 B). Phase-authored surface: 12 new files, 1,307 lines, plus one
reviewed launch row and the task-local command-line carrier. Measured artifact window: 3.29 h
(197 min), 11:42 → 14:59 on 2026-09-25 — a wall-clock window over the promotion's own files, not
billable hours, and it includes the staged-argv defect investigation. Contract additions:
`cellos_spawn.h` (+ `cellos_syscall.h` shared by both kits); the POSIX shim contract is unchanged
because neither kit is a shim symbol. Remaining class-D blocker: a third-party port that needs a
`fork`/`exec` process tree (no `fork`, no `execve`, no dynamic linker); the supported alternative
is one reviewed child launch. No third-party class-C port was attempted, so the class-C claim
rests on an in-tree workload — matching the class-B precedent, and stated rather than implied.
Historical Tetris-C hours remain unrecorded and are not estimated.

## Security invariants

1. C threads never cross cell/domain boundaries; `Spawn` is in-cell only.
2. TLS memory remains private to the owning task unless a separate grant is deliberately used.
3. C child creation has no ambient path authority: exact profile edge and child capability ceiling
   remain kernel-enforced.
4. Pipe inheritance is explicit endpoint duplication, never handle-text parsing.
5. Unsupported POSIX semantics fail loudly and remain in the generated shim contract.

## Promotion gate

The portfolio owner selected the atomic kernel carrier over the rejected strict-userspace-staging
alternative. Implementation hardens the existing launch transaction rather than adding a second
spawn ABI:

- `kernel/src/cell/state_stash.rs` now keeps argv in a bounded private full-TID map, separate from
  generic state keys.
- `kernel/src/task/syscall.rs` stages, reads, and clears this private carrier only through the
  literal argv protocol key; external spawn attempts clear it even when allowlist or preflight
  rejection happens before `governed_spawn_request`.
- `kernel/src/task/scheduler.rs` discards unread argv when the recipient dies.

The C wrapper still needs malformed and concurrent negative witnesses before `cellos_spawn.h` is
published. A userspace lock alone remains unacceptable because callers outside the adapter could
bypass it.

## Risk assessment

- **Undone by:** deleting the proposed adapter crates, profile rows, and witnesses; it need not
  modify existing pipe/futex/TLS behavior.
- **Cannot be undone:** a published `pthread_*` or C-spawn ABI once third-party ports depend on
  layout/error/lifecycle behavior. Version headers and refuse unsupported calls from day one.
