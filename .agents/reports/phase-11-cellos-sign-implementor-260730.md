# Phase 11 — `cellos-sign` implementation report (2026-07-30)

Plan: `.agents/260727-2101-midori-lessons-cellos/phase-11-cellos-sign-f1.md`
Baseline inventory: `.agents/reports/phase-11-f1-baseline-inventory-260730.md`
Deviations are logged live in the phase file's § Deviation Log (8 entries) — not restated here.

## What shipped

`scripts/cellos-sign` (CLI) + `scripts/cellos_sign/` (package: `scan` `allowlist` `policy`
`toolchain` `signing` `cli`, each under 200 lines). Two modes:

* `--check` — F1 attribute layer + F1 token layer + F5 toolchain, pure source parsing.
* `--sign ELF…` — runs the same check first; `run_sign` has no path to the signing call
  that skips it, and returns exit 1 with `REFUSED: not signing` when the check fails.

`scripts/lib-sign-cells.sh::sign_cells` — the only signing path in the image scripts — now
makes exactly one `cellos-sign --sign` call instead of two `sign-cell.py` calls. The
sign-only path is gone from that helper.

The production-key guard lives in `scripts/sign-cell.py` itself, not only in the wrapper, so
calling the low-level signer directly cannot mint a production signature outside CI.

`scripts/unsafe-allowlist.toml` — 45 `[[file]]` + 25 `[[crate]]` entries, every one with
`class` / `reason` / `approver` / `date`; temporary ones also carry `review_by` +
`tracking`. A malformed entry is a hard error (`AllowlistError`), never a silent skip.

## Decision: `check-cells-unsafe-ratchet.py` was replaced, not wrapped

Deleted. Its token rule and its 49-entry Python `ALLOWLIST` moved verbatim into
`scripts/unsafe-allowlist.toml`, read by `cellos_sign/policy.py`. Both CI workflows
(`ci.yml`, `security.yml`) now call `cellos-sign --check --strict`.

Rationale: the brief's hard constraint was that CI must not run two divergent copies of the
same rule. A shim would have satisfied that on day one and failed on day one hundred — the
shim is the copy nobody edits, so the two drift the moment someone adds an allowlist entry to
only one. One rule, one allowlist file, one invocation. The ratchet's behaviour is preserved
exactly: same `git ls-files` scope, same comment-stripping detection, same
unused-entry reporting (now for crate entries too).

`cargo-deny` was deliberately NOT added to the new job: `ci.yml`'s Security Scan already runs
it over the same workspace.

## Shell unsafe removal — 36 → 2

`cells/tools/shell/src/shell_state.rs` (new, 217 lines) holds what were `static mut` /
`UnsafeCell` + hand-written `unsafe impl Sync`, now `spin::Mutex` (`ostd::sync::Mutex`) and
atomics. Capacity and truncation limits are byte-for-byte the ones the fixed arrays imposed
(16 vars, 8 functions, 31-byte keys, 127-byte values, 479-byte bodies, NUL truncation), so
scripts relying on them behave identically.

Real changes, not lint dodges:

* `OutputSink::Buffer(*mut Vec<u8>)` → a capture **stack** (`Mutex<Vec<Vec<u8>>>`). Nested
  captures (`$(...)` inside a pipeline stage) push and pop; `CaptureGuard` pops on every exit
  path including panic.
* `CURRENT_STDIN: *const [u8]` → `Mutex<Vec<u8>>`; `shell_stdin()` returns owned bytes. Costs
  one clone per stdin-reading built-in, buys the removal of the pointer laundering.
* `get_var`/`get_function` returned `&'static str` borrowed out of a `static mut`; they now
  return `Option<String>`.
* Four `from_utf8_unchecked` calls → checked `from_utf8(..).unwrap_or("")`; each slice is
  ASCII by construction at the call site, so the fallback is unreachable and the property is
  now provable rather than asserted in a comment.
* `shell_test` PASSED/FAILED counters → `AtomicU32`.
* `ConfigClient`: `unsafe impl Sync` gone (`Mutex` supplies the bound). `get()` used to
  return a `&str` pointing into a buffer the *next* `get()` overwrites — unsound, not merely
  unsafe. It now leaks the returned string via `Box::leak`, with the contract written on the
  function. See Deviation Log #4: the correct fix is changing `ViConfig::get` to return
  `String`, which is `libs/api` and outside this phase.

The 2 remaining blocks are both in `cmd_fs.rs` and could not be made safe from this crate:
`ostd::fast_ipc::call_vfs` is an `unsafe fn` in ostd, and the `VfsResponse::DataPtr` path
dereferences a raw pointer into the VFS Cell's memory. Both vanish when `DataPtr` does.
Allowlisted with `review_by = 2026-10-28`, `tracking = "midori-lessons phase 06"`.

## Attribute migration — 16 → 51 crates

51 crate roots across 35 crates gained `#![forbid(unsafe_code)]`. This immediately broke
46 of them: `#[no_mangle]` is an *unsafe attribute*, so a hand-written entry point is a hard
error under `forbid`. Resolved by `ostd::cell_main!` (new, `libs/ostd/src/entry.rs`), which
emits the attribute from inside ostd where the `unsafe_code` lint does not fire — the same
property `app_entry!` already relied on. The macro doc says plainly what this does and does
not buy: the duplicate-symbol hazard is unchanged, and no unsafe *code* is hidden; what it
buys is that the rest of the crate is held to `forbid` instead of the whole crate being
exempted for one attribute. Exported symbol and ABI are preserved (`extern "C"` arm for the
three crates that used it). Seven stale "we cannot use forbid because of no_mangle" comments
were corrected.

## vfs / silo temporary entries

`cells/services/vfs/src/dispatch.rs` carries an entry naming the two caller-blocks writes at
`dispatch.rs:195` (`ReadGrant`) and `dispatch.rs:249` (`ReadFileGrant`), whose entire
soundness argument is "the caller's `ipc_call` blocks until we reply, so it cannot free the
grant" — with `review_by = 2026-10-28` and `tracking = "midori-lessons phase 07 — audit the
two caller-blocks assumptions"`. The other five vfs files and `service-silo/src/vmm.rs` carry
the same date and phase-07 pointer. `--check` reports any entry past `review_by`.

## Verification

Everything below was run; nothing is inferred.

| Check | Result |
|---|---|
| `python3 scripts/cellos-sign --check --strict` | `OK: F5` · `OK: F1 — 76 crates and 335 files scanned; unsafe confined to 45 allowlisted files`, exit 0 |
| `python3 scripts/test_cellos_sign.py` | 18 tests, OK |
| `cargo check` × 51 clean crates, `riscv64gc-unknown-none-elf -Z build-std` | 0 errors, 0 warnings |
| `cargo clippy` × same 51 | 0 errors, 0 warnings |
| `cargo check` × 14 allowlisted crates (shell, drivers, net, compositor, input, silo, wasm, …) | 0 errors each |
| `cargo check -p service-vfs --no-default-features` (riscv) | clean |
| `cargo check -p app-init -p app-sys-tools -p service-config --target aarch64-unknown-none` | clean |
| `cargo check -p service-config -p app-init --target x86_64-unknown-none` | clean |
| `cargo check -p app-shell` with and without `--features shell_test` | clean, clippy clean |
| `cargo fmt --all -- --check` | clean |
| `cargo test -p types -p api -p text-engine` (host) | ok |
| Break test — `unsafe {}` added to `cells/demos/hello` | `FAIL: F1 … [token] app-hello :: cells/demos/hello/src/main.rs`, exit 1 |
| Break test — attribute removed from the same crate | `FAIL: F1 … [attribute] app-hello :: …/main.rs`, exit 1 |
| Break test — `RUSTUP_TOOLCHAIN=nightly-2025-01-01` | `FAIL: F5 — active rustc d117b7f21183 is not the pinned nightly-2026-05-01 (f53b654a8882)`, exit 1 |
| Break test — allowlist entry aged to 2025-01-01 | `NOTE: … cells/demos/doom/src/main.rs (approved 2025-01-01, 575d ago)` |
| Coupling — `--sign` with a failing check | `REFUSED: not signing`, exit 1, target ELF has **0** `__ViCell_sig` sections afterwards |
| Prod key outside CI via `cellos-sign` | `REFUSED`, exit 3 |
| Prod key outside CI via `sign-cell.py` directly (bypass attempt) | `REFUSED`, exit 1 |
| Signature format unchanged | old `sign-cell.py` output vs new `cellos-sign` output over the same ELF: `cmp` → **byte-identical** |

### What the missing toolchain left unverified

No QEMU, no `riscv64-unknown-elf-gcc`, no cross objcopy. Therefore **not** run:

* image lanes / disk-image build, and the boot of a signed image;
* the end-to-end sign→verify round trip on a real cell ELF. Host ELFs cannot substitute: gcc
  places the ELF header inside the first `PT_LOAD`, so objcopy's `e_shoff` rewrite lands
  inside the signed payload and verification fails — **for the old pipeline exactly as much as
  the new one**, which is why the byte-identity result above is the meaningful evidence.
  Confirmed by diffing the payload hash before/after embed on a host ELF (`600` bytes,
  `333ccf8f…` → `59d4f304…`, segment table unchanged). `scripts/test-cell-signing.sh` covers
  the real round trip wherever a cross toolchain exists; it self-skips here.
* `lua`, `tetris-lua`, `tetris-c`, `doom` and `littlefs2-sys`-default crates do not compile on
  this machine — pre-existing, and all are allowlisted, so none was modified.

## Files

New: `scripts/cellos-sign`, `scripts/cellos_sign/{__init__,scan,allowlist,policy,toolchain,signing,cli}.py`,
`scripts/unsafe-allowlist.toml`, `scripts/test_cellos_sign.py`,
`cells/tools/shell/src/shell_state.rs`, `libs/ostd/src/entry.rs`.
Deleted: `scripts/check-cells-unsafe-ratchet.py` (`scripts/f1_scan.py` moved to
`scripts/cellos_sign/scan.py`).
Modified: `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`, `.github/workflows/{ci,security}.yml`,
`docs/security-model.md`, `libs/ostd/src/lib.rs`, 7 shell sources, 51 cell crate roots
(+ 7 stale-comment fixes).

## Follow-ups

1. `ViConfig::get` should return `String` (`libs/api/src/services/config.rs`); removes the
   `Box::leak` in `ConfigClient` (Deviation Log #4).
2. Spec 18 §1 still says "25 of 71 cell crates carry the attribute" and its cross-reference
   table still names only `sign-cell.py`. Left untouched — the phase marks Spec 18 read-only,
   and §1 is the ADR's historical context. Worth a corrective note from whoever owns it.
3. Run the image lanes and `scripts/test-cell-signing.sh` on a machine with the cross
   toolchain before merge — the one class of regression this environment cannot rule out.
4. Drive the two `cmd_fs.rs` blocks to zero as part of phase 06 (`DataPtr` removal), and the
   vfs/silo entries as part of phase 07; both are dated and tracked in the allowlist.
