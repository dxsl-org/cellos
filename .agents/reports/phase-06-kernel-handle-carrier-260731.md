# Phase 06 — kernel half: directory-handle carrier, task record, attestation

Date: 2026-07-31 · Branch: `feat/wx-post-reloc-and-f1-signing` (not rebased, not amended)
ADR: `docs/specs/09c-vfs-directory-capabilities-adr.md`

## What landed

The kernel now carries an opaque directory-handle set from a spawner to the cell
it spawns, records it against the child's task, and states on its own authority
which spawner supplied it. It never interprets a handle, holds no handle table,
and makes no claim about whether the spawner actually held any of them.

`cells/services/vfs/` untouched. No handle-based `VfsRequest` variants added.

### Files

| File | Δ | Role |
|---|---|---|
| `libs/api/src/abi/dir_handles.rs` | +194 new | `ViDirHandle`, `ViSpawnDirHandles` carrier, validated `DirHandleSet`, `InheritedDirHandles` |
| `libs/api/src/abi/dir_attestation.rs` | +129 new | `ViDirHandleAttestation` + LE encode/decode |
| `libs/api/src/abi/dir_handles_tests.rs` | +175 new | 17 host tests |
| `libs/api/src/abi/syscall.rs` | +44 | `SpawnSetDirs = 240`, `QueryDirHandles = 241` appended at END |
| `libs/api/src/abi/syscall_tests.rs` | +24 | discriminant stability + anti-collision test |
| `libs/api/src/abi.rs` | +3 | module registration |
| `kernel/src/task/tcb.rs` | +29 | `Task::inherited_dirs`, `Task::staged_dirs` |
| `kernel/src/task/dir_inherit.rs` | +110 new | stage / clear / install-on-child / attestation lookup |
| `kernel/src/task.rs` | +8 | install inside the task-creation critical section |
| `kernel/src/task/syscall.rs` | +168 | two handlers, query gate, clear-after-spawn on 4 spawn paths + hot-swap |

### Shape

- **Carrier** — `SpawnSetDirs(a0 = *const ViSpawnDirHandles)` stages a versioned
  `#[repr(C)]` carrier on the caller's own task. The next spawn consumes it and
  clears it; every spawn handler clears unconditionally afterwards, so a spawn
  that failed before task creation cannot leave its set for an unrelated later
  child. `a0 = 0` clears. SpawnCap-gated, matching every spawn entry point.
- **Record** — `Task::inherited_dirs` holds `{spawner_cell_id,
  spawner_generation, DirHandleSet}` inline, bounded at
  `MAX_SPAWN_DIR_HANDLES = 8`. No allocation, and no caller-supplied count sizes
  anything. Written once, inside the scheduler-lock critical section that creates
  the task, before any hart can pick it up.
- **Attestation** — `QueryDirHandles(a0 = cell_id, a1 = buf, a2 = len)` writes a
  112-byte record into the caller's buffer. Restricted to the registered
  `service::VFS` provider and to a cell asking about itself.
- **Hot-swap** — the handler clears the orchestrator's staged set before the
  swap, so a replacement instance can never inherit (ADR point 2).

Both new opcodes have `allowlist_bit() → None`, joining
`RegisterService`/`CapRevoke`: giving them bits would deny them to every cell
whose `__ViCell_syscalls` section was generated before those bits existed, and
the authority check at dispatch is the real gate regardless.

## Verification

| Command | Result |
|---|---|
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | clean |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | clean |
| `cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | clean |
| `cargo clippy -p vicell-kernel --target riscv64gc-... -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `cargo test -p api` | 35 + 2 passed, 0 failed (17 new) |
| `bash scripts/check-baseline.sh` | exit 0 |
| `bash scripts/build-boot-ramdisk-ci.sh` + release build + `scripts/qemu-boot-test.sh` | **`PASS: shell prompt reached`** |
| `cargo test --test boot -- --test-threads=1` | **53 passed, 1 failed** (`bench_all_pass`) |
| `cargo test --test hotswap-smoke -- --test-threads=1` | **11 passed, 0 failed, 0 SKIP** |

`cargo check` exiting in ~1.2 s was mutation-checked: a deliberate type error
injected into `kernel/src/task/dir_inherit.rs` was reported (`error[E0308]`), so
the fast exit is incremental caching, not a skipped crate.

### The one boot failure is pre-existing

`bench_all_pass` fails with `FATAL: Failed to spawn bench-probe. Missing in disk
image?`. Attribution experiment: all ten changed/new files backed up, the six
tracked files `git checkout`-ed to HEAD, the four new files deleted, kernel
rebuilt, test re-run — **identical failure at HEAD**. Reproduced three times
total (twice with the change, once without), never flaky. Restored afterwards
and re-verified: build clean, `cargo fmt --all --check` clean, boot still
`PASS`.

So the stated `boot 54/54` baseline is stale; the real current baseline is
53/54. Not fixed here — the bench disk artifacts are outside this phase's file
ownership.

## Concerns / Blockers

### 1. Why a staging syscall rather than a spawn argument, and why it is sound

`SpawnFromElf` (a0..a3 = grant_id, len, path_hint_ptr, path_hint_len) and
`SpawnPinned` (a0..a3 = path_ptr, path_len, priority, core_id) already consume
all four argument registers, and `ostd::syscall` fixes the register file at four
across riscv64/aarch64/x86_64 — a fifth means touching the asm wrapper and each
arch's trap-frame extraction in `hal/`. A pointer in a spare register therefore
reaches only `SpawnFromPath` and `SpawnFromMem`, and `ostd::sys_spawn_from_path`
prefers the `SpawnFromElf` route whenever VFS is registered — so the primary
post-boot spawn path would have carried nothing, silently.

Widening `ViSpawnArgs` was rejected for a sharper reason: the kernel reads
`size_of::<ViSpawnArgs>()` bytes from the caller's struct, and the repo ships
prebuilt cell binaries (`kernel/src/embedded*/`) compiled against the narrower
layout. Growing it makes the kernel read adjacent stack bytes as a handle
pointer.

Soundness of the staged form: the set lives on the spawner's **own** TCB, so
concurrent spawners on different harts cannot interfere. A task cannot have two
spawns in flight — it is blocked in its own syscall — so "the next spawn"
is unambiguous. Consumption happens in `spawn_cell_task`
(`kernel/src/task.rs:553`) inside the scheduler-lock critical section that
created the child, *before* the guard drops; `spawn_with_stacks` calls
`push_ready` while that same lock is held, and any hart that could schedule the
child must first take it. There is no window in which a child runs with an unset
record. Failure paths clear unconditionally in all four spawn handlers.

### 2. Why the variable-length set is pulled, not pushed — and why that is sound

`CallerIdentity` is a fixed 32-byte trailer written past the payload in the
receiver's recv buffer. A handle set is variable-length: it does not fit, and
reserving a worst-case tail on every recv buffer would tax every message in the
system for a field almost none of them need. Truncating was not an option — a
truncated set reads as a *narrower* grant than the kernel recorded, which is the
exact silent-narrowing failure this work exists to prevent.

`QueryDirHandles` inverts the direction. Its trust basis is identical to the
trailer's: the record is produced by the kernel from live scheduler state and
written into the querying service's own buffer during that service's own
syscall — it is never relayed through a message any cell composed, so there is
nothing to forge. The size problem disappears because the service supplies a
buffer it sized itself; a buffer shorter than `DIR_ATTESTATION_LEN` returns
`BufferTooSmall` rather than a partial write.

Access is restricted to the registered `service::VFS` provider (resolved through
the SpawnCap-gated service registry, **not** by cell name — names come from a
spawner-chosen `path_hint` and are forgeable) and to a cell asking about its own
cell. Lock order is SCHEDULER → REGISTRY, matching `Scheduler::reap`
(`kernel/src/task/scheduler.rs:446`); REGISTRY is a leaf.

The record carries the child's `(cell_id, generation)` and the spawner's
`(cell_id, generation)`. A non-empty set with no named spawner fails to parse:
"these handles came from somewhere" is not a claim any service can check.

### 3. Where narrowing-only is actually enforced — NOT at spawn, and not fakeable

The ADR says a spawn requesting a handle its parent does not hold must fail the
spawn. **The kernel cannot enforce that half, and I did not pretend to.**

Two reasons, the second stronger than the first:

1. The kernel is not the authority. Deciding "did this parent hold handle H" is
   a question about the VFS's own table.
2. Even a kernel-side approximation would be **wrong**, not merely incomplete. A
   cell acquires handles from the VFS *after* it starts (`OpenDir` derives new
   ones), so `Task::inherited_dirs` is "what its spawner named at spawn", never
   "what it holds now". Checking a child's set against its parent's recorded set
   would reject legitimate narrowing of a handle the parent obtained at runtime —
   and it is precisely the second source of truth the ADR rejects.

What the kernel does enforce, all-or-nothing and loudly, in
`DirHandleSet::from_carrier`: version match, count within the structural bound,
no zero handle, no duplicate. Any violation fails `SpawnSetDirs` with
`InvalidInput` and a `log::warn!` naming the tid and the reason, so the spawn
that follows carries nothing rather than carrying a trimmed set. Nothing is ever
dropped silently — that is the kernel's half of "loudly".

The authority half must live in the VFS half of the phase, at bind time, and it
**must be all-or-nothing**: on seeing any handle the attested spawner did not
hold, refuse to bind the entire set rather than binding the valid subset.
Binding the subset is exactly the quiet downgrade the ADR forbids.

**Residual gap, stated plainly rather than papered over:** with the check at bind
time, an over-broad request does not fail the *spawn* — the child exists and its
first VFS call is refused. This is fail-closed (the child holds no filesystem
authority, not extra authority), but it is weaker than the ADR's wording, and
the ADR's wording is not achievable without the kernel making a synchronous IPC
call to the VFS from inside a spawn syscall — a layering inversion and a
deadlock hazard. If the phase wants a literal spawn-time failure, the VFS half
should expose a pre-validation call the spawner makes before `SpawnSetDirs`;
that is a courtesy check with a TOCTOU window, not enforcement, and the
all-or-nothing bind must remain the actual control.

### 4. Smaller notes

- `MAX_SPAWN_DIR_HANDLES = 8` is a judgement call from the ADR's "the sets are
  small". Raising it is a one-constant change but moves `DIR_ATTESTATION_LEN`, so
  it needs the version bump the record already carries.
- Threads do not get their own copy. The record lives on the cell's primary task
  and `attestation_for` resolves `cell_id → that task`, so any thread of the cell
  yields the same answer rather than an empty set of its own.
- Build artifacts `kernel/src/embedded/init`, `kernel/src/embedded-aarch64/init`
  and `kernel/src/embedded/kernel_fs.img` were regenerated by the prescribed
  verification commands. Not source edits.
- No `ostd` client wrapper was added — that belongs with the VFS half, which is
  where the first caller appears.

---

**Status:** DONE_WITH_CONCERNS
**Summary:** The kernel now carries a bounded, validated directory-handle set from spawner to child, records it on the child's task before the child can run, and attests its provenance to the filesystem service through a pull-based query syscall; it never interprets a handle. The one boot-suite failure is pre-existing and was proven so by reverting to HEAD and reproducing it.
**Verification:** 3-arch check/build clean · clippy `-D warnings` clean · `cargo fmt --all --check` clean · `cargo test -p api` 35+2 pass (17 new) · `check-baseline.sh` exit 0 · `qemu-boot-test.sh` → `PASS: shell prompt reached` · boot suite 53 passed / 1 failed (`bench_all_pass`, identical failure at HEAD with all changes reverted) · hotswap-smoke 11/11, 0 SKIP.
**Concerns/Blockers:** (a) Variable-length attestation is *pulled* via `QueryDirHandles`, not pushed in the 32-byte `CallerIdentity` trailer, which cannot hold it; the kernel writes the record into the VFS's own buffer during the VFS's own syscall, so the trust basis is identical to the trailer's while an undersized buffer is an error rather than a truncation. (b) Narrowing-only is NOT enforced in the kernel and cannot be — a kernel subset check would also be *incorrect*, since a parent may pass a handle it acquired after its own spawn. The kernel enforces only structural validity, all-or-nothing and loudly; the authority check must be an all-or-nothing bind in the VFS half, which means an over-broad request fails the child's first VFS call rather than the spawn itself. (c) `boot` baseline is 53/54, not the stated 54/54.
