# Phase 06 — VFS half: directory handles, with the filesystem service as the authority

Date: 2026-07-31 · Branch: `feat/wx-post-reloc-and-f1-signing` (not rebased, not amended)
ADR: `docs/specs/09c-vfs-directory-capabilities-adr.md` · Kernel half: commit `6d3bcc10`

## What landed

The VFS now issues, validates and revokes directory handles, and a cell that has
migrated cannot name a path at all. `kernel/src/task/dir_inherit.rs` and the
kernel spawn path are untouched.

The pioneer, `/bin/vfs-test`, acquires its directories, works entirely through
handles, seals itself, and then gets `Err(3)` for `Write { path }` — observed in
serial output, in a suite that runs.

### Files

| File | Δ | Role |
|---|---|---|
| `libs/api/src/services/dir_name.rs` | +132 new | Raw-byte component and root-path rules, plus the join they make sound |
| `libs/api/src/services/dir_name_tests.rs` | +319 new | 26 host tests: each traversal shape separately, discriminant stability, wire round trip |
| `libs/api/src/services/ipc.rs` | +115 | Nine request variants + `VfsResponse::DirHandle`, all appended; `is_path_addressed` |
| `libs/api/src/abi/dir_handles.rs` | +8 | serde on `ViDirHandle` only |
| `libs/api/src/services.rs` | +2 | module registration |
| `libs/ostd/src/syscall.rs` | +41 | `sys_query_dir_handles` |
| `cells/services/vfs/src/dirs.rs` | +161 new | Handle table: issue, resolve, per-cell bound |
| `cells/services/vfs/src/dirs/lifecycle.rs` | +187 new | Contact, seal, generation purge, transitive revocation |
| `cells/services/vfs/src/dirs/bind.rs` | +104 new | All-or-nothing bind of an attested set |
| `cells/services/vfs/src/dir_admission.rs` | +52 new | Pull the kernel's record, once per cell generation |
| `cells/services/vfs/src/dispatch_dirs.rs` | +183 new | The handle-addressed arms |
| `cells/services/vfs/src/paths.rs` | +84 new | `write_file` / `unlink_file`, shared by both addressing models |
| `cells/services/vfs/src/dispatch.rs` | −45/+46 | Admission, seal gate at entry, delegation; two arms now call the shared helpers |
| `cells/services/vfs/src/main.rs` | +20 | module wiring, fast-path seal gate |
| `cells/services/vfs/src/manager.rs` | +5 | `VfsManager::dirs` |
| `cells/tests/vfs-test/src/dircap.rs` | +309 new | The pioneer scenario, 24 assertions |
| `cells/tests/vfs-test/src/main.rs` | +16 | `vfs_raw` (hand-encoded messages) + scenario hookup |
| `docs/specs/09-vfs.md` | +47 | §2b, the model as built |
| `docs/specs/17-ipc-wire-contract.md` | +28 | byte-0 range widened to `0x16`, amendment log |

`cells/services/vfs/src/access.rs` unchanged, 132 lines.

### Shape

- **Table** (`dirs.rs`) — one `BTreeMap<u64, DirEntry>` for the whole service.
  Each entry names its owner `(CellId, generation)`, the absolute directory, and
  the handle it was derived from. Lookup filters on owner, so unknown and
  not-yours are the same answer and sweeping handle values reveals nothing. `0`
  is never issued. Bounded at 32 handles per cell.
- **Resolution** — `(dir, name)` → the component is checked as raw bytes and the
  result joined onto the directory's path. With separators excluded, `..` as a
  whole component is the only remaining way to name a parent, so rejecting it
  closes the set. Nothing is normalised before the check.
- **Revocation is transitive** — `CloseDir` walks the derivation graph to a
  fixpoint, across cells: an inherited entry records the *spawner's* handle as
  its parent, so a spawner giving up a handle takes its children's with it.
- **Hot-swap** — seeing a higher generation for a cell purges the previous
  instance's entries outright rather than filtering them out on lookup. A
  replacement inherits nothing, including the predecessor's seal.
- **Binding** (`bind.rs` + `dir_admission.rs`) — on first contact the service
  pulls `QueryDirHandles`, resolves every named value against the *spawner's* own
  entries, and inserts nothing until the whole set has checked out. One failure
  refuses all. A partial insert is unwound rather than left reachable.
- **The seal** — refused at dispatch entry via `VfsRequest::is_path_addressed`,
  which is an exhaustive match, so a variant added later does not compile until
  someone has decided which side it falls on.

## Verification

| Command | Result |
|---|---|
| `cargo check -p service-vfs --no-default-features --target riscv64gc-… -Z build-std` | clean |
| `cargo clippy -p service-vfs --no-default-features --target riscv64gc-… -- -D warnings` | clean |
| `cargo clippy -p app-vfs-test --target riscv64gc-… -- -D warnings` | clean |
| `cargo check -p vicell-kernel --target riscv64gc-… -Z build-std` | clean |
| `cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std` | clean |
| `cargo fmt --all --check` | clean |
| `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api` | **61 + 2 passed, 0 failed** (26 new; was 35+2) |
| `pwsh -NoProfile -File ./gen_disk.ps1` | exit 0 |
| `bash scripts/qemu-boot-test.sh …/release/vicell-kernel` | **`PASS: shell prompt reached`** |
| `cargo test --test boot -- --test-threads=1` | **54 passed, 0 failed** |
| `cargo test --test vfs-quota -- --test-threads=1` | **1 passed, 0 failed, no SKIP** |
| `cargo test --test redoxfs-srv -- --test-threads=1` | **3 passed, 0 failed, no SKIP** |
| `cargo test --test hotswap-smoke -- --test-threads=1` | **11 passed, 0 failed** |
| `cargo test --test handoff -- --test-threads=1` | **26 passed, 0 failed** |
| out-of-tree harness over the real `dirs*.rs` source | **24 passed, 0 failed** |

Suites run serially. `boot` came in at **54/54**, above the 53/54 baseline: the
missing `bench-probe` that failed `bench_all_pass` is present in a `gen_disk.ps1`
image and absent from a `build-boot-ramdisk-ci.sh` one. The baseline was an
artefact of the image, not of the kernel.

### The table logic is executed, not just compiled

Cell crates cannot be host-tested, so `dirs.rs`, `dirs/lifecycle.rs`,
`dirs/bind.rs` and `caller.rs` run through an out-of-tree harness that symlinks
the real source text (`scratchpad/dirs-harness`). 24 tests: cross-cell
unreachability, respawn isolation, seal one-wayness, three-deep transitive
revocation, revocation crossing into a child cell, and five bind-refusal shapes.

**Mutation-checked** against copies with three defects introduced —
`bind_inherited` binding the valid subset instead of refusing, revocation
stopping at the named handle, `resolve` trusting the name. 6 tests failed, 18
passed; each defect was caught by the tests that exist to catch it.

### The attestation path is live, not merely written

`QueryDirHandles` is gated to the registered VFS provider, and a denial is
indistinguishable from "no set" at the call site — so a permanently-denied query
would have looked exactly like correct behaviour. Proven with a temporary probe
in `admit`: the test-hooks boot printed `[vfs] PROBE: provenance query OK`,
meaning the syscall was permitted, the record was written into the service's own
buffer, and it parsed. Probe removed, binary re-checked with `strings` (2 → 0
occurrences), and the suite re-run afterwards.

### The pioneer

`vfs-test` reports **68 PASS, 0 FAIL** (was 44). The decisive lines, in order:

```
[PASS] dircap: acquiring /tmp yields a directory handle
[PASS] dircap: `..` is refused
[PASS] dircap: `../..` is refused
[PASS] dircap: an absolute name is refused
[PASS] dircap: an embedded traversal is refused
[PASS] dircap: `.` is refused
[PASS] dircap: an empty name is refused
[PASS] dircap: a backslash name is refused
[PASS] dircap: a control byte is refused
[PASS] dircap: odd UTF-8 in a name is refused
[PASS] dircap: revoking a handle revokes what was derived from it
[PASS] dircap: the cell gives up naming paths
[PASS] dircap: Write{path} is refused after sealing
[PASS] dircap: the refused write left nothing behind
[PASS] dircap: GetFile / Stat / ListDir / Unlink / Mkdir / ReadAsync are refused after sealing
[PASS] dircap: widening by acquiring a new root is refused after sealing
[PASS] dircap: handle operations still work after sealing
```

Each traversal shape is asserted against all five name-taking operations, not
one. `odd UTF-8` is sent as hand-encoded wire bytes (`vfs_raw`), because Rust
will not let a `&str` hold an overlong `/` — it is the one shape the type system
cannot stop a caller from producing.

## Concerns

### 1. Nine variants, not six — three of them were unavoidable

The brief named six. `OpenRootDir { path }` is the bootstrap: `OpenDir` derives
from a handle you already hold, so with no acquisition the interface is
unreachable. It stays ACL-gated, is classified path-addressed, and is therefore
refused after sealing — a sealed cell cannot widen itself. This is Capsicum's
shape: open your directories, then `cap_enter()`.

`SealPaths` is the flag; without a way to set it the phase's primary criterion is
not reachable. `CloseDir` gives ADR point 1 a caller — transitive revocation with
no operation that triggers it is an untested claim, and it is now asserted both
in the harness and in QEMU.

`ListAt` takes no name. Listing a subdirectory means holding a handle to it; a
`name` here would be a second resolution path to keep in step with the first.

### 2. The flag is set two ways, and only one of them is self-imposed

A cell seals itself with `SealPaths`. Separately, **any cell the kernel attests
an inherited set for is sealed on first contact, whether or not the bind
succeeded.** The second is the one that makes this enforcement rather than
etiquette, and the "whether or not" is deliberate: if a refused bind left path
strings open, failure would *widen* the child's reach relative to success.

Self-sealing is still a real control even though the cell asks for it. It is a
one-way authority reduction the cell cannot undo — the same ratchet as
`cap_enter`. What it is not is a control against a cell that never seals, and
that is exactly why the path-string variants must eventually be deleted.

### 3. Where narrowing-only is enforced, and what that costs

In `bind_inherited`, all-or-nothing, exactly as the kernel-half report required.
The consequence the ADR names is real and now observable: **an over-broad spawn
does not fail the spawn.** The child exists, its whole set is refused, it is
sealed with zero handles, and its first filesystem call fails. Fail-closed — the
child holds no filesystem authority, not extra — but later than its cause.

No cell stages a handle set today, so the bind path is exercised only by the
harness (nine tests) and by the live `NothingNamed` case in QEMU. The first real
inheriting spawn will be the first end-to-end exercise of a non-empty bind.

### 4. `VfsRequest` now runs past byte-0 `0x0F`

Variants 14–22 encode to `0x0E`–`0x16`, numerically overlapping
`INPUT_EVENT_OPCODE` (`0x10`), `NET_READY` (`0x11`) and `REACTOR_WAKE` (`0x12`)
in the spec-17 §3 registry. Safe by receiver, on the same grounds those three are
safe against each other: the VFS is never a focus target, declares
`network = false` with no net op in its allowlist, and runs a plain
`sys_recv_attested` loop rather than a reactor. Recorded in §3 with the standing
obligation to re-check before variant 23, or before the VFS ever becomes a focus
target, net-interest owner, or reactor host.

Discriminants 0–13 are untouched, verified by a host test that asserts `GetFile`
encodes to byte 0 and `ReadFileGrant` to byte 13.

### 5. The fast-IPC path now declines a cell it has not met

`vfs_fast_handler` runs with interrupts disabled and cannot make the attestation
syscall, so it cannot know whether an unseen cell should already be sealed.
Serving it would have served a path read to a cell that must be refused one. It
now returns 0 for an unseen cell, which `call_vfs` callers already treat as "fast
path unavailable" and retry as an ecall; the ecall registers the cell and every
later fast call is served. Cost: one ecall per cell, once. The shell exercises
this on every boot and the boot suite is 54/54.

### 6. The pioneer is `/bin/vfs-test`, not a demo cell — deliberate

The brief said "a small demo, NOT the shell". No demo cell is spawned during any
suite that runs here: init's demo list is explicitly on-demand from the shell.
Migrating one would have produced no observable evidence without also editing the
image build scripts, which this phase does not own. `vfs-test` is small, is not
the shell, is already auto-spawned by init, and is asserted by two suites. It
runs the migration in full and seals last, since sealing is one-way.

## Out of scope — what I found

### `GetFile`'s raw pointer

Still `VfsResponse::DataPtr { ptr, len }`, still permanent unrevocable read
authority in a single address space. Live callers: `cells/tools/shell/src/cmd_fs.rs:341`,
`cells/tools/wasm/src/main.rs:100`, `cells/runtimes/lua/src/bindings_vfs.rs:57` and `:95`.
None of them keeps the pointer past the immediate copy, so converting it to a
time-bounded grant is a change to four call sites plus the two service arms
(`dispatch.rs` and the fast handler), not a redesign. The fast-IPC path is the
awkward part: it exists *because* `GetFile` is ~3 cycles, and a grant round trip
gives that up. `ReadFileGrant` already provides the grant-shaped equivalent, so
the honest options are "route the four callers to `ReadFileGrant` and delete
`GetFile`" or "keep the fast path and accept the pointer". Worth deciding
explicitly rather than by attrition.

Note that `GetFile` is classified path-addressed, so a sealed cell already cannot
obtain such a pointer. The hole is open only for cells that have not migrated.

### Deleting the path-string variants

Mechanically ready — `is_path_addressed` already enumerates exactly the set to
delete, and the exhaustive match means the compiler will find every arm. Blocked
on migrating the remaining callers: the shell (`cmd_fs.rs`), the lua runtime,
wasm, `libs/ostd/src/clients/vfs.rs`, `libs/ostd/src/fs.rs`, `srv-test`,
`http-smoke`. The shell is the hard one — it takes paths from a human and would
need a working-directory handle plus per-argument resolution.

### Pre-existing bug found on the way past

`libs/ostd/src/clients/vfs.rs:37` `VfsClient::read_file` sends `GetFile` and
matches only `VfsResponse::Data`, but the service answers `GetFile` with
`DataPtr`. Every call therefore returns `Err(ViError::IO)`. Not touched — outside
this phase, and no in-tree caller depends on it (`sdk-demo` uses `stat`). Worth a
one-line fix or a deletion when the `GetFile` decision above is made.

### No `ostd` client wrappers for the new operations

Deliberately not written. The pioneer constructs requests directly, as the rest
of `vfs-test` does, so a wrapper would have been unexercised API surface guessing
at the shape the next migration needs. The first cell that migrates against
`VfsClient` should define it.

---

**Status:** DONE
**Summary:** The filesystem service now issues, validates and transitively revokes directory handles keyed per cell; names are refused on raw bytes before any normalisation; an inherited set is bound all-or-nothing only after the service confirms the attested spawner genuinely held every handle in it; and the pioneer cell, having sealed itself, is refused every path-string operation.
**Verification:** 3-arch check/build clean · clippy `-D warnings` clean on `service-vfs` and `app-vfs-test` · `cargo fmt --all --check` clean · `cargo test -p api` 61+2 pass (26 new) · out-of-tree harness over the real table source 24/24, mutation-checked (3 defects introduced → 6 failures) · `gen_disk.ps1` exit 0 · `qemu-boot-test.sh` → `PASS: shell prompt reached` · boot **54/54** · vfs-quota **1/1** · redoxfs-srv **3/3** · hotswap-smoke **11/11** · handoff **26/26**, all serial, no SKIP.
**Concerns/Blockers:** (a) **The pioneer's `Err(3)` is confirmed in serial output** — `[PASS] dircap: Write{path} is refused after sealing`, followed by `the refused write left nothing behind` and the same refusal for `GetFile`/`Stat`/`ListDir`/`Unlink`/`Mkdir`/`ReadAsync`/`OpenRootDir`, while handle operations keep working; `vfs-test` reports 68 PASS / 0 FAIL. (b) **No traversal form reached the backend.** All seven required shapes plus backslash and a control byte were refused, each asserted against all five name-taking operations; `..`, `.`, empty and the length limit are refused in `validate_dir_component` before any join, and `/abs` and `a/../../b` die on the separator rule before the `..` comparison is even reached. The odd-UTF-8 form was sent as hand-encoded wire bytes and refused at decode, so the resolver never saw it. `the handle resolved inside the directory it names` confirms positively that a legitimate `WriteAt` landed at `/tmp/dircap/note.txt` and nowhere else. (c) Three variants beyond the six named were unavoidable — acquisition, sealing, and revocation each have no counterpart in the six, and the phase criterion is unreachable without the second. (d) An over-broad inherited set fails the child's first filesystem call rather than the spawn, as the ADR states; no cell stages a set yet, so the non-empty bind path is covered by the harness, not by QEMU. (e) `VfsRequest` byte-0 now overlaps three registered non-postcard values, safe by receiver and recorded in spec 17 §3 with a re-check obligation before variant 23.
