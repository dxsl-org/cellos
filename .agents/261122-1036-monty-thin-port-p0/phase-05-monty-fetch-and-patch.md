---
phase: 5
title: "Monty fetch, platform patch and supply-chain gate"
status: completed
priority: P2
effort: "0.5d"
dependencies: [4]
tier: medium
checkpoint: false
---

# Phase 5: Monty fetch, platform patch and supply-chain gate

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Bring upstream Monty into the build the way mlibc is brought in: the pinned commit fetched into a gitignored
directory, then a small ordered patch set. No Monty source is committed.

## Requirements

- Functional: `scripts/fetch-monty.sh` is idempotent. It fetches `MONTY_PIN` (chosen in phase 1) into
  `third_party/monty-src/`, resets it clean, applies `third_party/monty/patches/*.patch` in order, and prints HEAD.
- Functional: patch `0001-gate-ruff-stacker-off-cellos.patch`:
  - drops `features = ["stacker"]` from the workspace `ruff_python_parser` (0.0.21) dependency;
  - re-adds it in `crates/monty/Cargo.toml` under `[target.'cfg(not(target_os = "cellos"))'.dependencies]`.
  - Host builds keep stacker; cellos builds lose `psm`, `stacker` and their libc and asm requirements.
- Functional: `deny.toml` gets named `[[licenses.exceptions]]` for exactly the crates `cargo deny` flags at
  `MONTY_PIN`. Expected candidates are `BlueOak-1.0.0` (`minicbor*`) and `Unicode-DFS-2016` (`unicode_names2` and/or `ruff_unicode_names2`).
  Take the names from the deny output, not from this plan. The allow list is unchanged.
- Non-functional: the P0 patch set is build-metadata only. Every patch gets a purpose line and an
  "upstreamable: yes/no + why" note in `third_party/README.md`.

## Architecture

```text
scripts/fetch-monty.sh ──fetch MONTY_PIN──▶ third_party/monty-src/      (gitignored)
                       ──git apply───────▶ third_party/monty/patches/0001-gate-ruff-stacker-off-cellos.patch
cells/demos/monty-smoke ──path dep──────▶ third_party/monty-src/crates/{monty,monty-types}   (phase 6)
```

`{ workspace = true }` dependencies resolve through `third_party/monty-src/Cargo.toml`. Monty's `[patch]` and
`[profile]` are ignored for a path dependency. Without stacker, the host must compile every source with
`source_scan_threshold = 0` (`crates/monty/src/source_nesting.rs:1-9`). Phase 6 tests both sides of that bound.

## Assumptions

- **Claim:** nothing else in the cellos graph re-enables `ruff_python_parser/stacker`.
  **Confidence:** medium. **How to verify:** `cargo tree -e features -i stacker --target <cellos json>` in phase 6 is empty.
- **Claim:** cargo-deny evaluates the cell's separate workspace with the root policy.
  **Confidence:** medium. **How to verify:** `cargo deny --manifest-path cells/demos/monty-smoke/Cargo.toml --config deny.toml check`
  in phase 6; the precedent is `ci.yml:2828-2831`.

## Related Files

- Create: `scripts/fetch-monty.sh`, `third_party/monty/patches/0001-gate-ruff-stacker-off-cellos.patch`
- Modify: `third_party/README.md` (`## monty`: upstream, pin, MIT, purpose, patches, refresh procedure), `.gitignore`, `deny.toml`

## Implementation Steps

1. Write `fetch-monty.sh` from step 1 of `scripts/build-mlibc.sh` (`git init` once, `fetch --depth 1 origin $MONTY_PIN`,
   `checkout -f`, `clean -ffdx`, `git apply` each patch, `set -euo pipefail`). The header notes that moving the pin
   requires rerunning the phase 6 witness.
2. Produce 0001 with `git diff` in a pristine checkout of the pin.
3. README section (with the refresh procedure: bump the pin, `git apply --check` each patch, rerun the witness), plus the `.gitignore` line.
4. `cargo +N check --manifest-path third_party/monty-src/crates/monty/Cargo.toml -Z build-std=core,alloc,std,panic_abort -Z json-target-spec --target targets/<arch>-unknown-cellos.json`
   with `RUSTFLAGS='--cfg getrandom_backend="custom"'` and `__CARGO_TESTS_ONLY_SRC_ROOT`, for all three architectures.
5. Run the script twice; the HEAD is identical and the tree clean apart from the patch.

## Success Criteria

- [x] `fetch-monty.sh` succeeds from a clean clone and again on a second run.
- [x] Step 4 passes on all three cellos targets with no `psm`/`stacker` compiled.
- [x] Root `cargo deny --all-features check` still passes.

## Evidence

- `fetch-monty.sh` run three times in a row: the same HEAD `5f4a3dfc…` each time, with only the patched files modified.
- `cargo +nightly-2026-11-21 check` of `monty` passes on x86_64, riscv64gc and aarch64 `*-unknown-cellos`.
- `cargo tree -e features -i stacker` for the cellos target reports nothing.
- Root `cargo deny --all-features check` reports `advisories ok, bans ok, licenses ok, sources ok`. The witness workspace is checked against the root policy and reports `bans ok, licenses ok, sources ok`.

## Security Considerations

The pin is a commit hash, so a moved tag cannot change what is built. Without stacker, an unscanned deep source
overflows the native stack into the guard page and kills the Tier 2 cell. That is contained, but it is a DoS,
which is why the threshold-0 rule is mandatory.

## Risk Notes

- `MONTY_PIN` may be a pre-release (`1.7.0-beta.1`). Moving to the first stable release with ruff 0.0.21 is a later pin bump.
- Fetching an unadvertised commit by SHA relies on GitHub allowing it for public repos, which the mlibc script already depends on.

## Deviation Log

- Surprise: building (not checking) Monty for `x86_64-unknown-cellos` hit `LLVM ERROR: Do not know how to split the result of this operator` in `sha1`, `sha2` and `jiter`. The cellos x86_64 spec is soft-float with `-sse`. sha1/sha2 have portable backends selected by cfg; jiter uses SSE2 intrinsics on every x86_64 target.
  Why: phase 1 only type-checked (`cargo check`), which never reaches LLVM codegen.
  Impact: added `third_party/monty/patches/jiter/0001-gate-x86_64-simd-on-sse2.patch` (14 cfg sites gated on `target_feature = "sse2"`; jiter's host tests still pass). `fetch-monty.sh` now downloads `jiter-0.19.0.crate`, verifies its SHA-256 (`4027f861…`, matching crates.io) and patches it into the gitignored `third_party/monty-deps/`. The witness uses `[patch.crates-io]` and selects `sha1_backend`/`sha2_backend="soft"`.
  Revert: delete the jiter patch, the deps block of the script, the `[patch.crates-io]` entry and the two cfgs.
- Decision: rejected enabling `+sse,+sse2` for the whole cell (it breaks `compiler_builtins` with "Do not know how to soften this operator's operand") and per-package profile rustflags for jiter (same LLVM error under `+soft-float`).
  Why: a source-level gate is the only form that compiles.
  Impact: none beyond the jiter patch.
  Revert: n/a.
- Deviation: four `deny.toml` license exceptions were added by name (`minicbor`, `minicbor-serde`: BlueOak-1.0.0; `unicode_names2`, `ruff_unicode_names2`: Unicode-DFS-2016). The monty path dependencies are pinned `=1.7.0-beta.1` (cargo-deny `wildcards = "deny"`).
  Why: these were the only rejections cargo-deny reported for the witness workspace.
  Impact: `deny.toml`. The root run warns `license-exception-not-encountered` because only the witness workspace uses them.
  Revert: remove the four entries.
