# VFS handle-table owner checks — phase 02, step 3 only

Plan: `.agents/260727-2101-midori-lessons-cellos/phase-02-vfs-read-gating.md`
Branch: `feat/wx-post-reloc-and-f1-signing` · Date: 2026-07-30

## What changed

Both VFS handle tables now refuse a lookup whose recorded owner is not the caller,
and refuse it *without* consuming the entry.

**`cells/services/vfs/src/pending.rs`** (+96/−16)

- `PendingRead` gains `owner: CellId`; `insert(owner, data)` records it.
- `poll(caller, handle)` compares before removing:
  ```rust
  match self.slots.get(&handle) {
      Some(slot) if slot.owner == caller => self.slots.remove(&handle).map(|s| s.data),
      _ => None,
  }
  ```
  The order is the point. The pre-fix `slots.remove(&handle)` meant a non-owner
  sweeping `Poll{1..N}` both *read* every other cell's pre-read file contents and
  *destroyed* them. Comparing after a remove would have closed the read but kept
  the destroy.
- Module doc now states that handle IDs are sequential from 1 and therefore not
  secrets — confidentiality rests on the owner comparison alone.

**`cells/services/vfs/src/handle_table.rs`** (+94/−12)

- `get_mut(caller, cap)` → `self.entries.get_mut(&cap.0).filter(|e| e.owner == caller)`.
- `remove(caller, cap)` uses the same compare-then-remove shape as `poll`.
- `insert_ro` reordered to `(owner, cap, data_ptr, data_len)` so identity is the
  first argument across the whole module. Safe: `insert_ro` has **no callers
  anywhere** in the tree (that is what the module-level `#![allow(dead_code)]`
  covers), so `HandleTable` is empty at runtime today and `ReadGrant` already
  returned 0 bytes for every cap. The owner check there is prophylactic, for when
  the write path is wired.
- Two wrong doc comments fixed. Module header no longer claims "Per-cell open file
  handle table" — it says the `CapId` keyspace is shared across all client cells,
  which is exactly why each entry records an owner and each lookup compares it.
  `owner` no longer says "(for quota accounting)" — it is the authorization
  subject on every lookup, and the quota subject.

**`cells/services/vfs/src/dispatch.rs`** (+94/−53, net ~+8 lines of logic)

- Caller identity is derived in **one** place, at the top of `handle_request`:
  `let caller = types::CellId(sender as u64);`. The six per-arm copies of that
  same expression are gone; every arm reads `caller`. Swapping in a kernel-attested
  `cell_id` is now a one-line change at line 34.
  `grep -rn "CellId(sender" cells/services/vfs/src/` → **1** hit (line 34).
  The phase's "0 hits" criterion belongs to step 4 and is untouched.
- Deny-by-default at the boundary: `sender == 0` is not a resolvable identity, so
  `handle_request` returns `Err(3)` before constructing a `CellId`. `main.rs:135`
  already filters `sender > 0`; this guards the derivation itself.
- Three call sites routed (the complete set — see below):
  `pending.insert(caller, data)` (ReadAsync, :171), `pending.poll(caller, handle)`
  (Poll, :185), `handles.get_mut(caller, CapId(cap))` (ReadGrant, :207).

**Full call-site set, verified rather than assumed.** `grep -rn "\.handles\|\.pending\|insert_ro"`
over `cells/ libs/ tests/` returns exactly four uses of these tables:
`dispatch.rs` ReadAsync / Poll / ReadGrant, plus the constructors in `manager.rs:68,76`.
`HandleTable::remove` and `insert_ro` have no callers. The phase file's
`dispatch.rs:173` pointer was the ReadGrant `get_mut` site, which now sits at :207
after the edits.

**No error-code oracle.** Wrong owner returns the same reply as stale/unknown —
`Err(4)` for `Poll`, `GrantDone { bytes: 0 }` for `ReadGrant`. A distinguishable
"not yours" would turn the sequential keyspace into an existence oracle for other
cells' handles. Spec 17 wire shapes are unchanged; `libs/api` and the kernel were
not touched.

Out of scope and deliberately untouched: the 7 read ops are still ungated
(`can_read` stays `#[allow(dead_code)]`), `access.rs` was not opened (still 107
lines), `quota.rs` unchanged.

## Identity soundness — sys_spawn check

`grep -rnE "sys_spawn\(" cells/ libs/ostd/src` returns **two** hits, both
definitions and neither in a cell: `libs/ostd/src/syscall.rs:233` (the wrapper) and
`libs/ostd/src/task.rs:38` (`ostd::task::spawn` calling it). **No cell calls
`sys_spawn` today.** Every cell-spawning call in `cells/` is
`sys_spawn_from_path` / `_from_mem` / `_from_elf` / `_pinned`, which go through the
loader and get `cell_id == CellId(tid)`. So `CellId(sender as u64)` is a correct,
unique per-caller identity for every caller that exists right now, and comparing
it closes the cross-cell hole. It would misattribute a *thread* (own tid, inherited
parent `cell_id`) — latent, not live. The derivation comment says so.

## Verification

| Command | Result |
|---|---|
| `cargo check -p service-vfs --no-default-features --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | pass |
| `cargo clippy -p service-vfs --no-default-features --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` | pass, 0 warnings |
| `cargo fmt --all --check` | pass (after `rustfmt --edition 2021` on the 3 owned files; the only diff was in `dispatch.rs`) |
| `python3 scripts/cellos-sign --check --strict` | pass — `F5` pinned nightly OK, `F1` 77 crates / 337 files, unsafe confined to 46 allowlisted files |

`cargo check` was proven non-vacuous: a deliberate `let _probe: u8 = handle;`
inserted into `poll` produced `error[E0308]: mismatched types`, and the probe was
removed with an edit (never `git checkout`). Post-revert check, clippy and fmt all
re-run clean. `git status --short` shows only the 3 owned files — no intersection
with any parallel phase.

**Not run, and not claimable:** no QEMU / cross-gcc here, so nothing was booted and
no `tests/integration/vfs-*` suite ran. No runtime behaviour is asserted.

### Tests — 8 added, 8 executed, mutation-checked

Tests live in `#[cfg(test)] mod tests` inside `pending.rs` (4) and
`handle_table.rs` (4), matching where this cell's siblings keep theirs
(`cells/services/net/src/tls/*`, `cells/tools/shell/src/parser.rs`).

`service-vfs` is a `no_std` / `no_main` **bin** crate with no host-buildable test
target (`ostd`, `driver-disk`, `fatfs`, `redoxfs` do not build for
`x86_64-unknown-linux-gnu`), so `cargo test -p service-vfs` cannot reach them, and
neither can the riscv commands: I verified empirically that `--cfg test` *without*
`--test` strips `#[test]` functions entirely (a deliberate type error inside such a
function was not reported), so no repo command typechecks these modules. That is a
pre-existing gap in this crate, not something this change introduced.

To avoid claiming untested code, the two files were executed verbatim through an
out-of-tree host harness at
`/tmp/claude-1000/-home-dmin-cellos/785c7a7e-3edf-4857-8b20-8b5c0eb1b66d/scratchpad/vfs-table-harness`
— a std lib crate that `#[path]`-includes the two real source files and depends on
the real `libs/types` and `libs/api`. Under `cargo test` the in-crate `cfg(test)`
modules compile and run:

```
running 8 tests
handle_table::tests::cap_sweep_yields_nothing_to_a_non_owner ... ok
handle_table::tests::get_mut_rejects_another_cells_handle ... ok
handle_table::tests::owner_can_reach_and_close_its_own_handle ... ok
handle_table::tests::remove_rejects_another_cells_handle_and_keeps_the_entry ... ok
pending::tests::handle_sweep_neither_reads_nor_destroys_another_cells_slot ... ok
pending::tests::poll_rejects_a_handle_that_was_never_issued ... ok
pending::tests::poll_rejects_another_cells_handle ... ok
pending::tests::poll_returns_data_to_the_issuing_cell ... ok
test result: ok. 8 passed; 0 failed
```

The exact source text in the repo ran; the *test runner* was out of tree.

Mutation check, so the passes are not vacuous: copies with the owner comparisons
reverted to the pre-fix shape (`slots.remove(&handle)` ignoring owner;
`entries.get_mut(&cap.0)` / `entries.remove(&cap.0)` ignoring owner) fail
**5 of 8** — `handle 1 readable by a non-owner`,
`assertion failed: table.get_mut(CELL_A, B_CAP).is_none()`,
`assertion failed: table.remove(CELL_A, B_CAP).is_none()`, and both direct
wrong-owner tests. The 3 that pass either way are the positive path and the
never-issued-handle case, as expected.

The two attacker-shaped tests are the ones the phase asked for:
`handle_sweep_neither_reads_nor_destroys_another_cells_slot` sweeps `Poll{1..64}`
as cell A, asserts every probe returns `None`, then asserts cell B's slot still
yields its original bytes; `cap_sweep_yields_nothing_to_a_non_owner` sweeps
`CapId(0..64)` through both `get_mut` and `remove` as cell A and then confirms B's
handle survived.

## Concerns

1. **In-tree tests are not compiled by any repo command.** Executed out of tree
   (above), but they will silently rot. A host test target for these two tables, or
   equivalent cases in `tests/integration/vfs-*`, is a real follow-up. Logged in the
   phase § Deviation Log.
2. **The fast-IPC path carries no caller identity at all.** `ostd::fast_ipc::call_vfs`
   (`libs/ostd/src/fast_ipc.rs:134`) invokes `vfs_fast_handler` (`main.rs:95`) with
   `(req, out)` and no sender — it never enters `dispatch::handle_request`. Harmless
   for this step: that handler serves only `GetFile`, which touches neither table.
   But step 8 (gate `GetFile`, the highest-value gate because it hands out a raw
   `DataPtr`) has **nothing to gate on** over that path. This must be part of the
   step-4 ABI design, not discovered afterwards.
3. **`HandleTable` is unreachable state today.** `insert_ro` has no callers, so
   `ReadGrant` returns 0 bytes for every cap regardless of this change. The owner
   check is correct but currently unexercised at runtime; it matters when the write
   path lands.
4. **Adjacent, not touched:** `cells/services/compositor/src/main.rs` shows the same
   pattern half-applied — `table.get_mut(cap).filter(|s| s.owner == sender)` at
   :220/:235/:253 but a bare `table.get_mut(cap)` at :277/:328/:369/:386. Different
   service, out of this phase's ownership. Flagging only; no code changed.
5. No call site could not be routed. All four uses of the two tables are accounted
   for.

**Status:** DONE
**Summary:** Both VFS handle tables now record an owner and refuse — without consuming the entry — any lookup from a cell that does not own it, with caller identity derived in exactly one place in `dispatch.rs` so the pending ABI change is a one-line swap. 8 new owner-check tests pass and 5 of them fail against the pre-fix code.
**Verification:** `cargo check` + `cargo clippy -D warnings` (`-p service-vfs --no-default-features --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`), `cargo fmt --all --check`, `python3 scripts/cellos-sign --check --strict` — all pass; check proven non-vacuous with a deliberate type error. The 8 unit tests **ran and passed** against the exact repo source text, but via an out-of-tree host harness — no repo command compiles this crate's `cfg(test)` modules (proven: `--cfg test` without `--test` strips `#[test]` fns). No QEMU/cross-gcc: nothing booted, no integration suite run, no runtime claim made.
**Concerns/Blockers:** No unroutable call site; all four table uses covered. **No cell calls `sys_spawn` today** — only `libs/ostd/src/syscall.rs:233` and `libs/ostd/src/task.rs:38`, both definitions — so `CellId(tid)` is a sound per-caller identity now. Two follow-ups: these tests need a target that actually builds them, and the fast-IPC `GetFile` path carries no caller identity at all, which step 8 cannot gate without the step-4 ABI.
