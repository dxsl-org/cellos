# Phase 02 — Kernel-attested caller identity + VFS read gating

- Phase: `phase-02-vfs-read-gating` | Plan: `.agents/260727-2101-midori-lessons-cellos/`
- Status: **completed** (code + compile-time verification); runtime UNVERIFIED — no QEMU on this box
- Branch: `feat/wx-post-reloc-and-f1-signing` (5 pre-existing commits untouched, nothing committed)

---

## The ABI change

`Recv`'s **fourth argument was unused** and every pre-existing caller passes 0. Passing
`api::caller_identity::RECV_ATTEST_CALLER` there makes the kernel write a 32-byte
`CallerIdentity` (`cell_id`, `generation`, `sender_tid`; tagged, little-endian) into the
**last `CALLER_IDENTITY_LEN` bytes of that receiver's own recv buffer**, after the payload
copy.

Appended at the END, as required — and with a blast radius of zero on every other receiver,
because the trailer is opt-in per `recv` call. No message enum widened, no discriminant
moved, no new syscall number, no new allowlist bit, no extra round trip.

The kernel writes the trailer **last**, which is what makes it unforgeable: a sender that
pads its message across the whole buffer only overwrites bytes the kernel then replaces.

Type lives in a new `libs/api/src/abi/caller_identity.rs` rather than inside the
786-line `abi/syscall.rs`; `ViSyscall::Recv`'s doc comment records the flag.
`docs/specs/17-ipc-wire-contract.md` gains §11 (normative) plus §1/§5/§9 cross-references
and an amendment-log row.

## Files Modified

| File | Δ | What |
|------|---|------|
| `libs/api/src/abi/caller_identity.rs` | **new**, 190 | `CallerIdentity`, trailer encode/parse, 5 tests |
| `libs/api/src/abi.rs` | +1 | register module |
| `libs/api/src/abi/syscall.rs` | +9 | document `Recv` a3 flag |
| `libs/ostd/src/syscall.rs` | +24 | `sys_recv_attested` |
| `libs/ostd/src/fast_ipc.rs` | +21/−6 | handler signature; cell-local copy passes `None` (fail-closed) |
| `kernel/src/fast_ipc.rs` | +25/−4 | handler signature; `call_vfs` resolves identity from scheduler state |
| `kernel/src/task/syscall.rs` | +62/−6 | `attested_identity_of`, `write_caller_identity`, `Recv{attest_caller}`, a3 decode, 4 delivery points |
| `kernel/src/task/tcb.rs` | +24 | `Task::cell_generation` + monotonic source |
| `kernel/src/task/scheduler.rs` | +8 | thread inherits its cell's generation |
| `cells/services/vfs/src/caller.rs` | **new**, 90 | `Caller`, constructible only from an attestation, 3 tests |
| `cells/services/vfs/src/access.rs` | 126 (was 107) | live `can_read`/`can_write`, exact-then-prefix lookup, 5 tests |
| `cells/services/vfs/src/access/rules.rs` | **new**, 88 | rule tables + the reasoning |
| `cells/services/vfs/src/subtree.rs` | **new**, 50 | per-file subtree walk for quota release |
| `cells/services/vfs/src/dispatch.rs` | 348 (was 307) | 7 read gates, `Option<Caller>` boundary, quota-release fix |
| `cells/services/vfs/src/pending.rs` | 189 | owner is `Caller`; stores path; `owned_path`; +2 tests |
| `cells/services/vfs/src/handle_table.rs` | 194 | owner is `Caller`; stores path; `path_of`; +2 tests |
| `cells/services/vfs/src/quota.rs` | 151 (was 86) | path→writer record, `release_path` |
| `cells/services/vfs/src/main.rs` | +40/−16 | `sys_recv_attested`; fast handler gates `GetFile` |
| `docs/specs/17-ipc-wire-contract.md` | +72 | §11 + cross-refs + amendment row |
| `.agents/.../phase-02-vfs-read-gating.md` | +60 | § Deviation Log (written as work happened), todos, status |

`access.rs` = **126 lines**, under the 200 limit. `dispatch.rs` grew to 348; the subtree walk
moved out to `subtree.rs` to offset the gating.

## Tasks Completed

- [x] Kernel attests `cell_id`; **`grep -rn 'CellId(sender' cells/services/vfs/` = 0 hits** (was 1)
- [x] Fast-IPC carries identity (see Concerns for exactly how)
- [x] CellId reuse: **generation counter** chosen and implemented
- [x] `release` credits the charged owner, not the requester
- [x] All 7 read ops gated: `Stat`, `ListDir`, `GetFile`, `ReadAsync`, `Poll`, `ReadGrant`, `ReadFileGrant`
- [x] Rule shape: whole-path entries first, prefix fallback; unresolvable identity → deny at the boundary
- [x] Both `#[allow(dead_code)]` markers gone from `access.rs` (grep = 0)
- [x] `/bin/` kept read-all
- [~] `/srv/` **not** tightened — deliberate, see Concerns

### Gate 0 (forgeability) — answered by reading, not by spawning

`spawn_gated` verifies the Ed25519 signature over the **ELF bytes**, not over the path
(`kernel/src/loader.rs:122-151`), and the cell name is `path.rsplit('/').next()` of the
spawner-supplied `path_hint` (`kernel/src/loader.rs:182`). So a cell spawned as
`path_hint = "/bin/vfs"` **is** named `vfs`, and any ACL keyed on a cell name is forgeable.
That result is what shaped the rule table: **no shipped rule discriminates on the cell.**

## Design notes worth knowing

- **`Poll` and `ReadGrant` carry no path**, so `PendingRead` and `HandleEntry` now store the
  path and both ops **re-authorize** it. The open-time check proves only what policy said
  then; a handle outlives a rule change. That is what makes 7/7 real rather than 5/7 plus
  two ownership checks.
- **Error codes.** Rule denials return `Err(3)`. Two deliberate exceptions, preserved from
  the shipped owner-check work: a `Poll` on another cell's handle still returns `Err(4)`
  (same as stale) and a `ReadGrant` on another cell's cap still returns `GrantDone{bytes:0}`
  (same as unknown). Distinguishing them would turn the sequential handle/cap space into an
  existence oracle. A denial on a handle the caller *does* own returns `Err(3)` and leaks
  nothing it did not already know.
- **`TryRecv` / `RecvTimeout` / `RecvScatter` do not attest.** `RecvTimeout` already uses a3
  for its deadline; `RecvScatter` has no single buffer tail. Recorded as normative in §11.3:
  a service that authorizes must receive with `Recv`.
- **Quota accounting stays keyed on `CellId` alone** (no generation): a respawned service
  should inherit its predecessor's usage, because the bytes are still on disk. Only
  *handles* compare the generation.

## Verification

Every command below was run; outcomes are verbatim, nothing inferred.

| Command | Result |
|---|---|
| `cargo check -p service-vfs --no-default-features --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | pass |
| `cargo clippy -p service-vfs --no-default-features --target riscv64gc-... -Z build-std=core,alloc -- -D warnings` | pass, 0 warnings |
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | pass |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | pass |
| `cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | pass — codegen + link, still linkable |
| `cargo clippy -p vicell-kernel --target riscv64gc-... -- -D warnings` | pass (extra, not required) |
| `cargo fmt --all --check` | pass, no diffs |
| `python3 scripts/cellos-sign --check --strict` | pass (`F5` pinned nightly; `F1` 77 crates / 337 files, unsafe in 46 allowlisted files) |
| every cell that talks to VFS: `cargo check -p {service-httpd, app-srv-test, app-vfs-test, app-net-tools, app-shell, app-wasm, app-init}` for riscv64 | all pass |

Kernel checks complete in well under a second, so I confirmed the harness is honest by
injecting `fn __probe() -> u32 { "not a u32" }` into `kernel/src/fast_ipc.rs` — 2 errors
reported — then restored the file from a scratchpad copy.

### Tests that actually executed

**In-tree, real `cargo test`** — 5 tests, `cargo test -p api --target x86_64-unknown-linux-gnu`
(17 lib tests total pass):

```
abi::caller_identity::tests::trailer_round_trips ... ok
abi::caller_identity::tests::untagged_tail_is_not_an_identity ... ok
abi::caller_identity::tests::cell_zero_is_rejected ... ok
abi::caller_identity::tests::short_buffer_yields_no_identity ... ok
abi::caller_identity::tests::identity_is_read_from_the_tail_not_the_head ... ok
```

**Out-of-tree host harness** — 20 tests over `access.rs`, `access/rules.rs`, `caller.rs`,
`pending.rs`, `handle_table.rs`, all passing. These are **not** in-tree tests and **no repo
command runs them**: the VFS cell is a `no_std`/`no_main` bin crate whose deps do not build
for the host, and `--cfg test` without `--test` strips `#[test]` functions outright. The
harness is a std lib crate in the scratchpad that reaches the real files through symlinks
(`#[path]` resolves a module's children relative to its own directory, hence the symlink
mirror rather than direct paths). It compiles the actual working-tree source text.

Deny-path tests added, and **mutation-checked** — each mutation was applied to *copies*, never
the repo:

| Mutation | Test that caught it |
|---|---|
| generation dropped from `Caller` equality | `same_cell_different_generation_is_a_different_principal`, `a_respawned_cell_does_not_inherit_its_predecessors_handles`, `a_respawned_cell_cannot_poll_its_predecessors_slot` |
| `unwrap_or(false)` → `unwrap_or(true)` in `can_read`/`can_write` | `a_path_matching_no_rule_is_denied_both_ways` |
| owner filter removed from `path_of` / `owned_path` | `path_of_is_visible_to_the_owner_only`, `owned_path_is_visible_to_the_issuing_cell_only` |
| magic check removed from the trailer parser | `untagged_tail_is_not_an_identity` |
| `cell_id == 0` rejection removed | `cell_zero_is_rejected` |

8/8 targeted mutations caught. **No runtime, boot, or integration result is claimed** — this
box has no QEMU, no cross-gcc, no cross-objcopy.

## Concerns

### How identity reaches the fast-IPC path

`kernel/src/fast_ipc.rs` — not the `libs/ostd` copy — is the canonical dispatch table, and
`call_vfs` there is `#[no_mangle]` **kernel code** that a cell's undefined `call_vfs` import
resolves to via `resolve_export`. So it does not need the caller to supply identity, and must
not accept it: every argument on that path is chosen by the cell being authorized. It calls
`crate::task::syscall::attested_identity_of(crate::task::current_task_id())` — live scheduler
state — and passes the result to the handler.

Ordering matters and is deliberate: identity resolution takes the scheduler lock, so it runs
**before** `SieGuard::disable()`. Holding that lock across the handler would deadlock the VFS
backends' own spinlocks.

`VfsFastHandler` gained a leading `caller: Option<CallerIdentity>`; `vfs_fast_handler` gates
`GetFile` with `can_read` exactly as the ecall path does, and denies on `None`. The per-cell
`libs/ostd` copy of `call_vfs` passes `None` — fail-closed — because inside a cell there is no
attested answer. Today that copy is unreachable anyway (each non-PIE cell links its own
`VFS_HANDLER_PTR`, which is null in clients, so `call_vfs` returns 0 and the caller falls back
to ecall), but the fail-closed default is what keeps it from becoming a bypass later.

Residual, and pre-existing: in a single address space a cell that can write VFS's memory can
rewrite the trailer, the handle tables, or the rule table directly. The magic tag distinguishes
"kernel wrote identity" from "stale payload bytes"; it is not a defence against that, and the
module doc says so.

### CellId reuse: generation counter, and why

Chosen: **generation counter** (`Task::cell_generation`, minted from a monotonic `AtomicU64` in
`Task::new`, overridden in `Scheduler::spawn_thread` with the parent cell's value). VFS's
`Caller` is `(cell, generation)` and both handle tables compare the pair.

Rejected invalidate-on-death: it needs a kernel→VFS channel announcing cell death — more ABI
surface than the whole change above — and it **fails open** for the window between the death
and VFS processing the notice.

Honest scope of the benefit: I verified `Scheduler::next_task_id` starts at 1 and only ever
increments, so `CellId` is **not recycled today** and the generation is defence-in-depth rather
than a patch for a leak that is currently flowing. It is what preserves the guarantee if tid
allocation ever changes, and it costs 8 bytes in a trailer that needed padding anyway. The
thread-inheritance step is not optional: without it a thread and its own cell would be two
different principals and a thread could not reach state its own cell opened.

### Reads left permissive, and why

- **`/srv/` not tightened to vfs/net/shell.** This needs a kernel-vouched binding from a
  calling cell to which program it runs. There is none: signatures cover bytes not paths, the
  cell name comes from a spawner-chosen `path_hint` (Gate 0 above), and `shell` has no entry in
  `api::syscall::service` to resolve through the registry. The two alternatives are both worse
  than the status quo — keying on the cell name ships the exact forgeable ACL Gate 0 forbids,
  and allowing only VFS/NET (resolvable via `LookupService`) breaks `ls /srv` from the shell and
  `cells/tests/srv-test`. The *shape* is shipped so a future row is a data change: `EXACT_RULES`
  (whole-path, checked first) + `PREFIX_RULES` (fallback). `EXACT_RULES` is empty.
- **Root `/` kept read-all.** Denying it is the real deny-by-default, and I started to. But the
  ramfs root holds paths outside every other prefix that cells read at startup —
  `cells/services/net-broker/src/identity.rs:24` reads `/etc/cellos/cluster.cfg`. Tightening it
  would break cluster boot with an opaque `PermissionDenied`, and I cannot boot to confirm the
  full set of such readers. Left permissive.
- Net effect on reads: the shipped table is broad on purpose, so the read gating's live value
  today is the **deny paths** — an unattested or unresolvable caller is refused on all 7 ops, a
  path matching no rule (any relative path) is refused, and handle/cap ownership plus generation
  is enforced. Narrowing per prefix is a data change from here.

### Scope and ownership

Touched outside the plan's File Ownership table, unavoidably: `kernel/src/task/tcb.rs` +
`kernel/src/task/scheduler.rs` (the generation needs a `Task` field and thread inheritance),
`kernel/src/fast_ipc.rs` + `libs/ostd/src/fast_ipc.rs` (the only places the fast-IPC requirement
can be met), `libs/ostd/src/syscall.rs` (`sys_recv_attested`). Logged in the phase's
§ Deviation Log.

**No conflict with the concurrent phase.** It holds `kernel/src/{loader.rs, main.rs, policy.rs}`,
`kernel/src/task/cap.rs`, `scripts/sign-policy.py`, and `kernel/src/loader/boot_ceiling*` — all
untouched here, verified against `git status` at the end of the run. `cargo fmt --all --check`
passes, and I formatted only my own files with `rustfmt` (never `cargo fmt --all`, which would
rewrite their in-flight work).

### Follow-ups

- A host-runnable test target for the VFS authorization modules, or equivalent cases in
  `tests/integration/vfs-*`. Right now 20 real tests exist that no repo command executes.
- The quota writer map does not survive a hot-swap (`all_entries`/`restore` carry only `used`).
  After one, a delete credits nobody — errs toward over-charging, never toward free quota.
- `WriteGrant` is still fail-closed; it needs cap→path routing before it can be authorized.
- Verify on hardware/QEMU: boot to shell, then `ls /bin`, `cat /data/*`, `ls /srv`. Gating reads
  is exactly the change that breaks boot silently, and I could not run it.

---

**Status:** DONE_WITH_CONCERNS
**Summary:** The kernel now attests the caller's `cell_id` (plus a cell generation) on every
opted-in `Recv` and on the fast-IPC path, VFS derives identity from nothing else, and all seven
read ops are gated on `can_read` with deny-by-default on unresolvable identity. The shipped rule
table stays deliberately broad: `/srv/` could not be tightened without either a forgeable
cell-name ACL or breaking `ls /srv`.
**Verification:** All 8 required commands run and passing — `cargo check -p service-vfs` +
`clippy -D warnings` (riscv64, `--no-default-features`), `cargo check -p vicell-kernel` (riscv64,
x86_64), `cargo build -p vicell-kernel` (aarch64-softfloat, links), `cargo fmt --all --check`,
`python3 scripts/cellos-sign --check --strict`; plus all 7 VFS-client cells checked for riscv64
and `cargo clippy -p vicell-kernel -D warnings`. Tests that **actually executed**: 5
`caller_identity` tests in-tree via `cargo test -p api`, and 20 VFS authorization tests via an
out-of-tree host harness (no repo command runs those — cell crates are `no_std`/`no_main` bins);
8/8 targeted mutations caught. **Zero runtime/boot results** — no QEMU, no cross-gcc here.
**Concerns/Blockers:** (1) Fast-IPC identity comes from `kernel::fast_ipc::call_vfs` resolving
`attested_identity_of(current_task_id())` from scheduler state before `SieGuard::disable()`, never
from an argument; the `libs/ostd` per-cell copy passes `None` and so fails closed. (2) Chose the
**generation counter** over invalidate-on-death, because invalidation needs a kernel→VFS death
channel and fails open in the notification window — but `next_task_id` is strictly monotonic
today, so this is defence-in-depth, not a leak being patched. (3) Reads left permissive and
flagged rather than guessed: `/srv/` (no kernel-vouched cell→program binding; `shell` has no
service id) and root `/` (net-broker reads `/etc/cellos/cluster.cfg` on the boot path, and I
cannot boot to enumerate the rest).
