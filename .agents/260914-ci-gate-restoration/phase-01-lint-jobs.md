# Phase 01 — The three lint jobs

**Status**: completed for the lint debt; the Lint job is now blocked only by a governed record it must not edit
**Ceiling**: hosted runner

## What was red

Seven jobs had been red on every push for at least a day (verified across eight consecutive runs
before this work started, so none of it was caused by the G2 AI lane's pushes):

| Job | Root cause | State now |
|---|---|---|
| `Clippy (x86_64)` | `cellos-kernel` for `x86_64-unknown-none`: `map_or(false, ..)`, `iter().any(== x)` | **green** |
| `Clippy (aarch64)` | same kernel lints under `aarch64-unknown-none-softfloat` | **green** |
| `Lint (fmt + clippy)` | (a) clippy debt across `libs/api`, `libs/cellos-fs`, `libs/ostd`, `cells/*`; (b) `cargo fmt` drift in 12 files; (c) stale generated metrics; (d) **governed ledger drift** | (a)(b)(c) green; (d) reported, not edited |
| `C2C Broker Oracle` | `local_c2c_broker_oracle_meets_baseline_contract` fails | not this lane |
| `Network Data-Path Integration` | 8 of 54 tests fail | not this lane |
| `RedoxFS /srv Integration Test` | guest prints `ALL TESTS PASSED`, host test panics at `tests/redoxfs-srv.rs:119` | not this lane |
| `QEMU Hypervisor Boot-to-Shell` | exits 1 after `Shell focus acquired` | not this lane |

## The lint fixes

All mechanical, each verified with the exact command its job runs
(`cargo clippy … -- -D warnings` for the job's target), plus `cargo fmt --all --check`:

- **`libs/api`**: no-op pointer casts. `c_char` is `u8` on bare-metal targets but `i8` on hosts, so
  `.cast::<u8>()` is the portable spelling rather than deleting the cast (which would break host
  builds). The SPSC ring's named interior-mutable `const EMPTY_SLOT` became an inline-const array
  repeat, which also takes its capacity from `RING_CAPACITY` instead of a literal list of 16.
- **`libs/cellos-fs`**: `div_ceil`; `MemDisk` holds `Rc`, not `Arc` — it is a single-threaded
  host-test/power-cut-fuzz device and `Arc<RefCell<..>>` asserted a `Send + Sync` it cannot provide.
- **`libs/ostd`**, **`kernel`**: `is_multiple_of`, a `# Safety` section on `init_custom_heap_raw`,
  `is_some_and`, `contains`.
- **`cells/drivers/dwc2-usb`**: dropped `| (0 << n)` register terms (the field layout stays as a
  comment above each construction), `div_ceil`, index-free FIFO packing loops.
- **`cells/tools/shell`**, **`cells/tests/bench`**, **`cells/tests/posix-shim-test`**,
  **`cells/tests/tier2-exploit`**: `for` loops, collapsed `if`s, `is_empty` beside `len`,
  `saturating_sub`, `to_string` over a no-argument `format!`, c-string literals, `null_mut`.

Two of them were code smells rather than noise, and are called out in the commit: `MemDisk`'s
`Arc<RefCell<..>>`, and the ring buffer's named interior-mutable const.

Verification of the *behaviour* of the touched libraries: `cargo test -p api` (101 + 2 passed),
`cargo test -p cellos-fs` (1 + 9 + 2 passed), a release build of the riscv64 kernel, and the QEMU
gates the AI lane owns (`http-infer`, `hypha-*`, both AI oracle legs) all pass afterwards.

## What this lane will not do

`scripts/validate-app-tier-acceptance.py` fails with `source import or digest drift`, and 8 tests in
`tests/app-tier-acceptance` error out. Both trace to one fact:

- `docs/specs/23-native-sdk-contract.md` was amended in commit `81dbb81c` (2026-09-06, "feat(rpi3):
  qualify physical boot…") — the C2-MID row's grant-authority note and its witness range changed,
  next to a 51-line `libs/ostd/src/grant.rs` change — and the app-tier ledger's
  `source_binding.sha256` was never re-bound (`matrix_sha256` still matches; the whole-file digest
  does not, and the recorded `ratified_revision` carries the older bytes).

Re-binding that digest asserts the amendment was reviewed and re-imported, and the ledger is the
project's authoritative acceptance record with its own event/attestation machinery — there is no
sanctioned scripted path for it, and it is not a lint. It belongs to the lane that owns Spec 23, and
the two lines above are everything that lane needs.
