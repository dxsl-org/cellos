# 2026-09-29 — Tier 1 Rust `std` status check: two blocking defects found and fixed

## Why
Requested status check of the `std` library (Tier 1 `rust-std` PAL). Reading the roadmap
said "no Cellos PAL, target JSON, … Tier 1 `rust-std` runtime"; the tree said otherwise, and
the only way to know was to run the lane end to end.

## What the live run showed
`scripts/build-cellos-sysroot.sh` failed: `patch: **** malformed patch at line 86`.
The overlay patch was not applicable at HEAD, and the script's "stage only if the staging
directory does not exist" rule had been hiding that since 2026-09-18. After repairing the
patch, the sysroot built and the std cell compiled, but the QEMU lane died at link:
`rust-lld: error: undefined symbol: main` from ostd's `_start`.

## Fixes landed
1. `patches/rust-std-cellos.patch` — four new-file hunks (written by hand in `3712c0a47`)
   were missing the `+` prefix on their body lines, and two hunk counts were stale:
   `pal/cellos/alloc.rs` declared 99 lines for 103, `pal/cellos/time.rs` declared 61 for 66;
   `random.rs` (1 line) and `thread.rs` (7 lines) lost prefixes inside otherwise correct
   hunks. Normalised all four to canonical unified-diff form; 23/23 files now apply.
2. `scripts/build-cellos-sysroot.sh` — the staging tree is re-created whenever
   `sha256sum patches/rust-std-cellos.patch` differs from `target/cellos-rust-src/.patch.sha256`,
   instead of being reused unpatched forever.
3. `libs/ostd/src/{entry,runtime,lib}.rs` — the seven entry-macro sites gated `no_mangle` on
   `target_os = "none"` (since `670e7d397`), so cells built for the JSON targets
   (`os = "cellos"`) lost the unmangled `main` that `_start` calls. The gate is now
   `not(any(test, unix, windows, target_family = "wasm"))`: hosted test binaries keep
   libtest's entry, bare-metal targets (`none` and `cellos`) emit `main`. Spelling it as
   "no hosted family" instead of naming `cellos` avoids `unexpected_cfgs` in the ~71 cell
   crates that expand these macros, which `clippy -- -D warnings` would reject.

## Verification (this host, pinned `nightly-2026-05-01`, `f53b654a8`)
- Fresh staging + patch: `scripts/build-cellos-sysroot.sh` PASS for
  `riscv64gc-`, `aarch64-`, `x86_64-unknown-cellos`.
- Runtime: `BOOT_TIMEOUT=90 scripts/run-std-smoke-qemu.sh riscv64` →
  `[std-smoke] PASS: All Rust std PAL invariants verified successfully!`
  (alloc 1000×2 KiB cycles, `Instant`, `yield_now`, parallelism = 1, `env::consts::OS`,
  fail-closed fs/net/process, argv).
- Cross-arch link: std-smoke links for `aarch64-unknown-cellos` and `x86_64-unknown-cellos`
  (PIE ELF each); only riscv64 has a QEMU boot runner.
- Lint: `cargo clippy -p app-init -p driver-virtio-blk -p service-config -p service-platform
  --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` clean;
  `cargo test -p app-shell --target x86_64-unknown-linux-gnu --no-run` emits zero
  `unexpected_cfgs` warnings; `rustfmt --check` clean.

## Open (recorded in `.agents/TODO.md`)
- No CI job runs the std lane; both defects above were invisible to CI.
- `pal/cellos/alloc.rs` ignores `layout.align() > 16` (GlobalAlloc contract gap).
- `docs/roadmap/runtime-and-platform-tracks.md` and the Spec 23 evidence files still claim
  the PAL/target/sysroot do not exist.

## Re-pin (same session, owner request)
`PAL-IMPLEMENTATION-CHECKPOINT` condition 6 authorises re-binding the records when a covered
input changes. Five inputs had drifted since the 2026-09-16 binding: `kernel/Cargo.toml`,
`kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`
(kernel-security group) and `libs/ostd/src/startup.rs` (cellos-backing group).

- `pal-hook-support-map.json`: 4/6 inventory `sha256` refreshed, `inventory_digest`
  `80721763…` → `da119cd4…`; the 46 pinned rust-src digests, `sys_module_manifest`, and
  `toolchain.source_digest` were verified unchanged.
- `approval-input-manifest.json`: all 106 inputs re-verified, the five drifting ones plus the
  support-map file entry refreshed; manifest digest `99cf7d24…` → `e30a3826…`.
- Decision package + 4 approval records re-bound to the new manifest digest (support-map
  digest `4f9be413…` → `ec4d5de6…` in the package), each with a one-line re-bind note that
  says explicitly this is a digest re-bind, not a new signer decision.
- `python3 -m pytest tests/rust-std-promotion -q` → **33/33 PASS** (was 2 failed / 31 passed);
  no other file in the repo references the superseded digests.

## Follow-up (same session)

**Allocator honours `Layout::align()` > 16.** The staged PAL always returned `block + 16`
with a 16-byte-aligned block, so every over-aligned request (`#[repr(align(N))]`, SIMD,
over-aligned slices) got a misaligned pointer — a `GlobalAlloc` contract violation (UB in
std). Reproduced before the fix with a new smoke assertion:
`Aligned64 #2 at 0x1080238d0 (mod 64 = 16)` → panic, no PASS marker. The check must go
through `core::hint::black_box`: printing `addr % 64` directly prints 0 for a *misaligned*
pointer, because the compiler folds the residue from the pointer's provenance.

The fix aligns the payload in absolute terms (a free block's start is only 16-byte
aligned, so the offset differs per block), stores the offset in the word immediately
before the payload — an over-aligned request reserves `HEADER_SIZE + 8` there so the word
never overlaps the 16-byte header — and `dealloc` reads it back. Requests with
`align <= 16` keep the previous layout (`+16`), so ordinary blocks did not grow. After:
`scripts/run-std-smoke-qemu.sh riscv64` PASS with 4 x 64 B and 2 x 256 B at aligned
addresses and intact payloads, warning-free.

**CI lane added.** `.github/workflows/ci.yml` gains `rust-std-lane`: promotion contract
tests (`unittest discover`), sysroot overlay for all three targets, and the QEMU smoke
boot. Nothing else in CI touched the lane, which is why the malformed patch and the lost
`main` symbol were invisible.

**Docs.** `docs/roadmap/runtime-and-platform-tracks.md` now states that the in-tree PAL,
target specs, sysroot overlay and QEMU/CI lane exist while qualification, triple
publication and promotion remain open. `docs/specs/23-native-sdk-contract.md` is *not*
edited: the SDK contract is content-addressed (ledger pins a snapshot revision and
deliberately never reads the amendable file), so correcting its C2-RST row needs a new
contract revision plus a ledger re-base — recorded in `.agents/TODO.md` as an owner action.

## Spec 23 re-base: recorded (2026-09-29)

Amending the contract requires re-basing `source_binding` (the live-bytes equality check in
`validator.bind_source`), and the ledger permits that inside exactly one carrier: a
`lifecycle_transition`, whose mutable set includes `source_binding`. `record_correction` may
touch `subjects` and `blockers` only, and the (3,4)/(4,5) schema migrations are exhausted, so
a prose correction has to ride a real phase step. Carriers checked: Phase 05 (Manifest-v2
tooling) is `completed` in its own plan and phase doc while the ledger still said `PLANNED`;
Phase 06 (Tier 1 rust-std PAL) documents itself as pending/dependency-blocked, so recording it
IMPLEMENTED would over-claim.

What landed:

- `fd3d12ae` (commit A) — the amendment (C2-RST row + "Known gaps" sentence now name the
  in-tree PAL, the three target specs and the QEMU/CI lane), archived as
  `docs/evidence/spec23-native-sdk-contract-9265afc81b15.md`, plus
  `docs/evidence/app-tier-phase05-implementation.log`, which re-runs the Phase 05 verification
  at `85df7fc0f` (API manifest 8/0, kernel host 184/0, ELF inspector corpus 6/6, RV64 clippy
  `-D warnings` clean).
- Commit B — ledger event 12: `lifecycle_transition` phase 5 `PLANNED → IMPLEMENTED`,
  `steward`/`reviewer` = `lungmat8`/`datgausaigon`, `changes` = `[source_binding,
  phase_lifecycle]`, evidence = the log + `docs/specs/05-application.md` as the bound
  artifact, `implementation.revision` = `85df7fc0f`. `source_binding.ratified_revision` =
  `fd3d12ae`. The seed fixture's binding, its event `state_digest`/`hash` and
  `baseline_prefix` were re-derived (a digest change without the hash chain would trip
  "history does not bind full state").

Verification: `scripts/validate-app-tier-acceptance.py docs/app-tier-acceptance-ledger.json
--baseline <pre-amendment ledger> --baseline-root <pre-amendment worktree>` → `PASS:
C9=NOT_COMPLETE`; `python3 -m unittest discover -s tests/app-tier-acceptance` → 81/81 OK.
The ratified matrix digest (`f742a6ec…`) is unchanged, which is the property the schema-v5
design requires of an amendment.

## Phase 06 recorded (2026-09-29, owner decision)

The owner asked whether Phase 06 could be completed too. Mechanically yes — a `lifecycle_transition`
PLANNED -> IMPLEMENTED is one adjacent step and needs no binding change — but the phase's own
documents said "Pending — dependency-blocked" and its success criteria require a live benchmark
(>=30 repetitions per promoted cell) plus named approvals, so the claim had to be scoped. The owner
chose the bounded form: record the implementation, keep qualification open, and correct the docs.

Event 13 records Phase 06 `IMPLEMENTED` with `docs/evidence/app-tier-phase06-implementation.log`
(three sysroot overlays PASS, QEMU std-smoke PASS with the over-aligned witnesses, promotion suite
33/33, plus CI run 36570111921 where the new `rust-std-lane` is green at `2a67570b8`) and
`patches/rust-std-cellos.patch` as the bound artifact; `implementation.revision = 2a67570b8`.

What stays explicitly blocked, in the ledger text and the docs: the validator remains fixture-only
and non-promotional, PAL-019/PAL-031 remain `Deferred`, the named human approval rows remain
`NOT GRANTED`, and there is no live capture, published triple, readiness or promotion — so `c9`
stays `NOT_COMPLETE` and phases 07/08 stay `PLANNED`.

Also landed: the CI lane's first run was red at the promotion-test step because two tests read the
maintainer's absolute rust-src path from the pinned artifacts; `tests/rust-std-promotion/rust_src.py`
now resolves it through the installed pinned toolchain (never a skip) and the two pinned test inputs
were re-pinned. The Phase 06 plan/doc edits needed a third re-pin of the same manifest.
