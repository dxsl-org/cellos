---
phase: 1
title: "Select nightly and rebase the rust-std PAL patch"
status: completed
priority: P1
effort: "2d"
dependencies: []
tier: thinking
checkpoint: false
---

# Phase 1: Select nightly and rebase the rust-std PAL patch

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Fix the two pins everything else depends on: the nightly `N` and the Monty commit `MONTY_PIN`. Then make
`patches/rust-std-cellos.patch` apply to `N`'s `rust-src`. This is the highest-uncertainty step, and later phases
consume its outputs.

## Requirements

- Functional: `N` has `rust-src`, `rustfmt`, `clippy` and `llvm-tools-preview` for the host. It installs targets
  `riscv64gc-unknown-none-elf`, `aarch64-unknown-none-softfloat` and `x86_64-unknown-none`, and `rustc +N -V` reports
  ≥ `1.99.0-nightly`, so Monty's `rust-version = "1.98"` holds without shims.
- Functional: `MONTY_PIN` is a 40-char commit whose workspace has `ruff_python_parser` **0.0.21** with the optional `stacker`
  feature. **Not `v1.6.0`**: that release pins ruff 0.0.14, where stacker is mandatory (red-team #1). Candidate order:
  1. the commit of tag `v1.7.0-beta.1`, if it has ruff 0.0.21;
  2. otherwise the probed `5f4a3dfc493538bb9dc319974393391ba9e7cf14`.
  Record why in the Deviation Log.
- Functional: the rebased patch keeps every invariant `std-smoke` asserts, including PAL-019 fail-closed entropy and PAL-031 buffer checks.
- Non-functional: no semantic PAL change; edits only follow upstream `library/` moves.

## Architecture

The overlay flow is unchanged (`scripts/build-cellos-sysroot.sh`). Only the base revision moves.
Rebase method: apply the old patch with fuzz/3-way onto pristine `rust-src` of `N`, resolve the rejects, and regenerate a single
`diff -ruN` patch. Known move: in the installed 1.100 nightly (2026-08-16), `sys/mod.rs` changes `alloc` from line 3
to a `pub mod alloc` at line 8. Phase 3 re-inventories this, and `test_validator.py:179-182` checks the order.
The staging cache stamp (`build-cellos-sysroot.sh:39-50`) is keyed on the patch digest only. Key it on
`patch digest + rustc commit` so a toolchain switch, including a rollback, always restages.

## Assumptions

- **Claim:** a recent nightly carries all four components for all three targets.
  **Confidence:** medium. **How to verify:** `rustup toolchain install nightly-YYYY-MM-DD --profile minimal -c rust-src,rustfmt,clippy,llvm-tools-preview -t <3 targets>`.
- **Claim:** target JSON schema, `-Z json-target-spec` and `-Z build-std` work unchanged on `N`.
  **Confidence:** medium. **How to verify:** step 5.
- **Claim:** tag `v1.7.0-beta.1` has ruff 0.0.21.
  **Confidence:** medium. **How to verify:** `git show <tag>:Cargo.toml | grep ruff_python_parser`.

## Related Files

- Modify: `patches/rust-std-cellos.patch`
- Modify: `scripts/build-cellos-sysroot.sh` (`EXPECTED_COMMIT`, `+nightly-…` literals, stamp key)
- Modify: `targets/*-unknown-cellos.json` (only if the spec schema changed)

## Implementation Steps

1. Pick `N`: install the newest nightly that satisfies the components and targets. Record `N` and its 9-char commit.
2. Pick `MONTY_PIN` per the candidate order above.
3. Gate: throwaway `cargo +N check` of `monty` at `MONTY_PIN` for `x86_64-unknown-cellos`, with stock `rust-src`, no `ruff/stacker`
   feature, `--cfg getrandom_backend="custom"`, and **no** `-Zcrate-attr`. If a newer API is missing, move to a newer `N` and log it.
4. Rebase the patch onto `N`. Diff the PAL hook files old vs new and record the diff summary in the Deviation Log.
5. Update `build-cellos-sysroot.sh` (pin, commit, stamp key). Run it for all three cellos targets.
6. Boot `std-smoke` on riscv64, aarch64 and x86_64, with `run-std-smoke-qemu.sh` locally pointed at `N` (committed in phase 2).

## Success Criteria

- [x] `build-cellos-sysroot.sh` passes for all three cellos targets on `N`. Switching back to the old pin restages: the stamp differs.
- [x] `run-std-smoke-qemu.sh` prints the std-smoke PASS marker on riscv64, aarch64 and x86_64.
- [x] The step 3 gate finishes at `MONTY_PIN` without `-Zcrate-attr`.

## Evidence

- `N` = `nightly-2026-11-21`, `rustc 1.102.0-nightly (c7d5ba59d 2026-11-20)`; all four components and three targets installed.
- `MONTY_PIN` = `5f4a3dfc493538bb9dc319974393391ba9e7cf14` (tag `v1.7.0-beta.1`, 2026-11-20, `ruff_python_parser 0.0.21` with optional `stacker`, `rust-version = "1.98"`).
- `cargo +nightly-2026-11-21 check -p monty` at the pin (host target): Finished, no shims.
- Rebased patch round-trips byte-identically against the staged tree with `patch -p1 -F0` (29 files).
- `build-cellos-sysroot.sh` PASS for riscv64gc/aarch64/x86_64-unknown-cellos.
- `run-std-smoke-qemu.sh {x86_64,riscv64,aarch64}`: `[std-smoke] PASS: All Rust std PAL invariants verified successfully!` on all three (logs `/tmp/stdsmoke-*.log`, not retained).

## Security Considerations

The PAL is the std↔kernel ABI boundary. A rebase must not weaken PAL-019 (no synthetic entropy) or PAL-031
(validation before the syscall). Phase 3 has the rebased hooks re-reviewed independently.

## Risk Notes

- May→November `library/std/src/sys` refactors can be large. If reject resolution runs past about a day, try an older
  1.99+ nightly closer to the old base, then log it.
- If no candidate passes, escalate: the toolchain decision goes back to the user.

## Deviation Log

- Deviation: step 3 gate ran on the host target, not `x86_64-unknown-cellos`.
  Why: a cellos-target check needs the rebased PAL, which did not exist yet at step 3.
  Impact: the cellos-target compile of Monty moves to phase 5 step 4.
  Revert: n/a.
- Surprise: upstream changed PAL-internal APIs between the two nightlies, not just context. The changes needed:
  (a) `sys/alloc` now expects free functions `alloc`/`dealloc`/`realloc` per platform (`use X as imp`) instead of a per-platform `impl GlobalAlloc for System`. `sys/alloc/cellos.rs` was rewritten to forward to `pal::alloc`, with `realloc` = `realloc_fallback` (the same alloc+copy+dealloc as the old default). `cellos` joined the `alloc_zeroed` fallback list (alloc + zero, the old default).
  (b) futex moved to `sys::sync::futex`; added a `target_os = "cellos" => pub use crate::sys::pal::futex::*` arm.
  (c) `core::intrinsics::abort` became `abort_immediate` (`pal/cellos/common.rs`).
  (d) Upstream `personality/mod.rs` dropped `motor` from the abort-stub arm. `cellos` stays in that arm (behaviour unchanged).
  (e) `sys/io/error/mod.rs`: the fuzzed hunk landed after the closing brace. It was fixed by adding `cellos` to the `generic` arm, which is now a single-line `any(..)`.
  Why: the `library/std/src/sys` refactor between 2026-05-01 and 2026-11-21.
  Impact: these are internal PAL API changes. `compiler-strategy-decision.md:43` lists "internal PAL API change" as an invalidation trigger, so phase 3 has the independent reviewer check (a)–(e) explicitly.
  Revert: restore the previous `patches/rust-std-cellos.patch` and pin.
- Decision: the sysroot staging stamp is now `<patch sha256>-<rustc commit>` (`scripts/build-cellos-sysroot.sh:40-43`), and the script uses one `TOOLCHAIN` variable.
  Why: red-team #15 (stale staging after a toolchain change).
  Impact: the first run after upgrade restages once.
  Revert: n/a.
- Deviation: step 6 needed `rust-toolchain.toml` moved to `N` (a phase 2 edit) first. `cellos-sign` F5 refuses an active rustc that differs from the pin.
  Why: the run script builds bootstrap cells and the kernel with the repo toolchain.
  Impact: the toolchain file and the two std run scripts were moved in phase 1.
  Revert: same commit set.
