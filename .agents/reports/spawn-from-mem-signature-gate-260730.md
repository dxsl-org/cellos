# SpawnFromMem signature/manifest gate — 2026-07-30

Branch: `feat/wx-post-reloc-and-f1-signing` (no rebase, no amend; work left uncommitted)

## What was wrong

`Syscall::SpawnFromMem` called `super::spawn_from_mem(...)` directly
(`kernel/src/task/syscall.rs:2637`, pre-fix), so it never reached the Ed25519 signature gate or the
manifest-privilege gate that live only in `crate::loader::spawn_gated` (`kernel/src/loader.rs:110`).
Unsigned ELFs loaded even under `signing-required`; the shell exposes the hole as
`exec <file>` behind only a 4-byte magic check.

## Fix

One gate for all spawn paths. `SpawnFromMem` now calls
`crate::loader::mem_spawn_gate::spawn_from_mem_gated`, which is a thin wrapper over the existing
`spawn_gated` — no second policy was written, and `spawn_gated` itself is unchanged.

Files:

- `kernel/src/loader/mem_spawn_gate.rs` — new, 98 lines. `spawn_from_mem_gated()` +
  `mem_label()` (the untrusted-name reduction) + `is_label_char()`.
- `kernel/src/loader.rs` — `pub mod mem_spawn_gate;` (2 lines).
- `kernel/src/task/syscall.rs` — `SpawnFromMem` arm rerouted; the pointer-validation prologue is
  unchanged, the previously uncommented `unsafe` block now carries a `// SAFETY:` note, and
  `ViError::PermissionDenied` is now mapped to `SyscallError::PermissionDenied` (it used to
  collapse to `InvalidInput`).
- `kernel/src/loader/elf_tests.rs` — 4 boot-time tests for the label derivation, registered in
  `run_all()`.

### Requirement 4 — how the caller-supplied name is prevented from selecting path caps

The name is never used as a path. `mem_label()` reduces it to `/mem/<component>` where
`<component>` is the final path component of the name, filtered to
`[A-Za-z0-9._-]`, truncated to 64 chars, and replaced by `cell` when it is empty, `.` or `..`.
Every path-keyed decision in `spawn_gated` is a whole-string comparison against a `/bin/` path, and
a `/mem/`-prefixed single-component label matches none of them:

| Decision in `spawn_gated` | Match form | Result for `/mem/<x>` |
|---|---|---|
| manifest privilege gate | `!path.starts_with("/bin/")` | treated as a **user cell** → any manifest with `flags != 0` is DENIED |
| `legacy_path_caps` | `starts_with("/bin/")` then `ends_with` | `CapSet::EMPTY` |
| `CapSet::with_path_caps` (pcie_driver/platform/supervisor) | `matches!(path, "/bin/nvme" \| …)`, `==` | no match → all three false |
| `policy::lookup` | `e.path == path` | `NoEntry` (POLICY.BIN holds only `/bin/` paths) |
| `policy::is_trusted_core` | `matches!(path, "/bin/vfs" \| "/bin/shell" \| "/bin/net")` | false → no fail-closed recovery grant |
| `/bin/vfs` block-region 0b1000 | `path == "/bin/vfs"` | no match |
| input-cell registration | `ends_with("/bin/input")` | no match |

The invariant that makes the suffix rows hold is that the component contains no `/`, so
`ends_with("/bin/anything")` can never succeed. Keeping only the final component is what enforces
it; `is_label_char` also rejecting `/` is redundant today and is documented as such (the mutation
run below proves the redundancy — flipping only the filter is not observable).

Net effect on caps: a cell spawned from caller-supplied bytes can request at most
`CapSet::from_manifest(m)` for a manifest with `flags == 0`, which is `CapSet::EMPTY` (every field
of `from_manifest` derives from a flag bit), and any `flags != 0` manifest is denied outright.
So the reroute grants strictly nothing that the old direct call granted — it cannot escalate.
Additionally the child's ceiling is now `Spawner::User(caller_id)` rather than "no ceiling at all",
and the deny path logs `[loader] DENY {:?}` and returns `PermissionDenied` (no panic added).

### Requirement 3 — CellId derivation not regressed

`spawn_gated` passes the same `CellId(0)` sentinel and the same empty `allowed_drivers` vec to
`task::spawn_from_mem` that the syscall used to pass (`kernel/src/loader.rs:187`), so
`task::spawn_cell_task` still overwrites `task.cell_id` with `CellId(tid)` before the task is
reachable (`kernel/src/task.rs:563`). No caller patches `cell_id`. A `SpawnFromMem` cell therefore
still never carries `CellId(0)`, so neither fault handler can see a user-mode fault attributed to
cell 0.

## Verification

Executed, all green on the final source:

```
rustfmt --edition 2021 --check <4 owned files>                                          → exit 0
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc → exit 0
cargo check -p vicell-kernel --target x86_64-unknown-none        -Z build-std=core,alloc → exit 0
cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc → exit 0 (still links)
cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings → exit 0
cargo fmt --all --check                                                                 → exit 0
```

No new warnings (the only two are the pre-existing "no strip tool" / "kernel_fs.img missing" build
script notes). `cargo fmt --all` itself was never run; only the four owned files were formatted with
`rustfmt`. `git status` shows only those four paths.

**Label logic executed for real** via an out-of-tree host harness
(`{scratchpad}/labelharness`, std lib crate that `#[path]`-includes the real
`mem_spawn_gate.rs` under a stub `loader` parent with a real `libs/types` path dep):
3 tests, 60+ hostile names including every privileged install path, each prefixed/suffixed/
NUL-padded variant, `..`, `//`, non-ASCII, and a 5000-char name — all pass; asserts that no label
equals or ends with any privileged path, that the component holds no `/`, that the label stays
≤ `MAX_CELL_PATH`, and that `spawn_gated` receives `/mem/vfs` when the caller names `/bin/vfs`.

Mutation-checked: reverting the component extraction to `let base = caller_name;` fails 2 of the 3
harness tests. Flipping only `is_label_char` to admit `/` fails none — which is why the code and
this report call that filter redundant defense-in-depth rather than the enforcing mechanism.

The harness earned its keep: it caught an assertion of mine claiming `/mem/x` contains exactly one
`/`. Because `elf_tests.rs` tests are boot-time `assert!`s, that wrong assertion would have
**panicked the kernel on every boot** and no local command would have caught it. The in-tree test
now asserts the component is separator-free instead.

## Not verified (no QEMU, no cross-gcc — cannot boot)

The 4 new `elf_tests` functions compile and are registered in `run_all()`, but they have **never
executed in-kernel**. Their assertions were re-run verbatim-equivalent on host (above), so I have
high confidence they do not panic, but "the boot-time suite passes" is an unverified claim.

Observable `exec` behaviour I could not verify:

1. **`exec <file>` still spawning at all.** The happy path is untested end-to-end. In a dev build
   (`signing-required` off) an unsigned file must still load, which is what the code does by
   reusing `spawn_gated`'s `None => {}` dev arm, but I never saw a cell start.
2. **`exec` of a cell whose manifest declares any privilege is now DENIED** (`PermissionDenied`,
   `[loader] DENY …: user cell over-declares caps`). Previously it spawned with zero caps and broke
   later at its first privileged syscall. Affected images are the ones with `flags != 0`:
   `/bin/vfs`, `/bin/net`, `net-broker`, the drivers, `supervisor`, `platform`, `init`,
   `hypha/core`, `hypha/tools/spawn`, `silo`. Demo/test cells (`tetris*`, `doom`, `lua`, `*-test`)
   declare all-false so they are unaffected — but I could not run `exec` to confirm which images a
   developer actually launches this way.
3. **Memory quota now applies to `exec`'d cells.** `spawn_gated` calls
   `cell_quota::register(cell_id, DEFAULT_QUOTA_BYTES)` = 16 MiB. Before, a `SpawnFromMem` cell was
   never registered and `charge()` fell through to `usize::MAX` — i.e. uncapped. A cell exec'd from
   the shell that needs more than 16 MiB of heap (doom is the plausible candidate) would now start
   and then fail an allocation. I cannot measure any cell's live footprint without booting. This is
   consistency with every other spawn path, not a new limit invented here, but it is the change
   most likely to be noticed.
4. **Cell name in `ps` is now the basename.** The task name passed down is the label's component,
   so `exec /bin/tetris` shows `tetris` where it used to show `/bin/tetris`. Cosmetic; unverified.
5. **`exec` still prints no reason on denial.** `ostd::sys_spawn_from_mem` collapses every negative
   return to `SyscallError::Unknown`, so the shell prints its generic failure line while the reason
   is only in the kernel log. Unchanged by this work, and I could not observe the message.

## Notes, not changed

- `kernel/src/main.rs:692` spawns `init` via `task::spawn_from_mem` without the gate. Left alone
  deliberately: those bytes are embedded in the kernel image itself (same signature domain as the
  kernel), and gating them would make the root of trust depend on a policy `init` has not yet
  loaded. Worth an explicit comment there, which I did not add — outside the fix's scope.
- `SpawnFromMem` still requires **no `SpawnCap`** (unlike `SpawnFromPath`/`SpawnFromElf`, which call
  `caller_has_spawn`). It is bounded now only by the per-cell syscall allowlist. Adding the check is
  a one-line change I deliberately did not make: it would change who may call the syscall, which is
  a separate policy decision from closing the signature hole, and could break `exec` for any cell
  lacking `spawn`. Recommend it as a follow-up.
- `SpawnFromMem` still ignores `args_ptr`/`args_len` (the shell passes `cmd_args`, the kernel drops
  them); `SpawnFromPath`/`SpawnFromElf` do the personal-ARGV-slot transfer. Pre-existing, untouched.
- `xmas_elf::find_section_by_name` / `raw_data` can panic on a malformed section header. This was
  already reachable from `SpawnFromMem` before the fix (`task::spawn_from_mem` calls
  `get_section(".rela.dyn")`, `kernel/src/task.rs:695`), so the reroute adds no new panic class — it
  moves the parsing earlier, before any task exists. Hardening `get_section` is a separate job.

**Status:** DONE_WITH_CONCERNS
**Summary:** `Syscall::SpawnFromMem` now goes through the single existing `loader::spawn_gated`
admission gate, so the Ed25519 signature check (fail-closed under `signing-required`) and the
manifest-privilege check apply to caller-supplied ELF bytes; the caller-supplied name is reduced to
a `/mem/<component>` label that provably matches none of the `/bin/` patterns those path-keyed
grants use, so no path-based privilege can be forged.
**Verification:** all five required commands pass (`cargo check` rv64 + x86_64, `cargo build`
aarch64 — still links, `cargo clippy -D warnings` rv64, `cargo fmt --all --check`), plus 3
out-of-tree host tests over 60+ hostile names, mutation-checked (reverting the component extraction
fails them). No runtime/boot verification: no QEMU, no cross-gcc.
**Concerns/Blockers:** Forgery prevented by never treating the name as a path — `mem_label()` keeps
only the final path component, filters it to `[A-Za-z0-9._-]`, and prefixes `/mem/`, so the label is
never `/bin/`-prefixed (manifest-privilege gate treats it as a user cell), never equals a privileged
install path (`with_path_caps`, `/bin/vfs` block-region grant, `policy::lookup`,
`is_trusted_core` all miss), and — because the component holds no `/` — can never satisfy an
`ends_with("/bin/…")` test (`legacy_path_caps`, input-cell registration). A `flags != 0` manifest is
denied outright on this path, so requested caps are provably `CapSet::EMPTY`. Unverified `exec`
behaviour: (a) the happy path — that an unsigned dev ELF still spawns — was never executed;
(b) `exec` of a privilege-declaring image (`/bin/vfs`, `/bin/net`, drivers, `hypha/core`, …) now
returns `PermissionDenied` instead of spawning a cap-less cell; (c) `exec`'d cells are now capped at
the 16 MiB default memory quota where they were previously uncapped — doom is the plausible casualty
and I cannot measure it; (d) the `ps` name changes from `/bin/tetris` to `tetris`.
