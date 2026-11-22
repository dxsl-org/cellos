---
phase: 2
title: "Move the repo to the new pin"
status: in-progress
priority: P1
effort: "1.5d"
dependencies: [1]
tier: medium
checkpoint: false
---

# Phase 2: Move the repo to the new pin

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Replace `nightly-2026-05-01` / `f53b654a8` with `N` in every live non-PAL file, and fix what the new compiler and
clippy reject. PAL-governed records belong to phase 3, Spec 23 to phase 4.

## Requirements

- Functional: every CI job that builds Rust uses `N`; there is no mixed-pin state.
- Functional: kernel, HAL, libs and cells build on all three bare-metal targets with unchanged flags (`-D warnings` where CI uses it).
- Non-functional: fixes are behaviour-preserving. `scripts/cellos-sign --check --strict` gates any `unsafe`
  or attribute change. `tcb-unsafe-baseline.toml` ceilings may drop, never rise.
- Non-functional: archival data is not rewritten.

## Architecture

Search: `rg -l "nightly-2026-05-01|f53b654a8|1\.97\.0" -g '!target' -g '!.agents' -g '!docs/evidence'`. Classify each hit:

| Class | Files | Action |
|---|---|---|
| Live: toolchain / CI | `rust-toolchain.toml`; `.github/workflows/{ci,docs,perf,release,security}.yml`; `scripts/ci-runner/setup-runner.sh` | edit |
| Live: scripts | `scripts/run-std-smoke-qemu.sh`, `scripts/run-std-parity-benchmark.sh` | edit |
| Live: tests | `scripts/test_cellos_sign.py:1268-1269` (echoes the channel), `tests/app-tier-acceptance/fixtures.py:90`, `scripts/app_tier_acceptance/source.py:97` | edit |
| Live: comments | `libs/posix-shim/src/lib.rs:24`, `libs/posix-shim/src/stdio_fmt.rs` | edit |
| PAL-governed | `tests/rust-std-promotion/rust_src.py`, approval records, manifests | **phase 3** |
| Archival / synthetic | `scripts/rust_std_promotion/{validator.py:26,benchmark-run.schema.json}` and `tests/rust-std-promotion/fixtures/*.json` (synthetic data under the benchmark approval; no builder exists); `kernel/src/embedded-x86_64/manifest.json` (build provenance; CI reads only `kernel_features`, `ci.yml:1182`); `scripts/test_compare_bench_results.py` (test data); `build/g2-j-cow/*.json`; changelog history | keep; list in Deviation Log |

## Assumptions

- **Claim:** `scripts/ci-local.py run <job>` reproduces CI jobs closely enough.
  **Confidence:** high (its docstring). **How to verify:** `python3 scripts/ci-local.py list`.
- **Claim:** no kernel `#![feature]` used today was removed by `N`.
  **Confidence:** medium. **How to verify:** step 3.

## Related Files

- Modify: the live files in the table above.
- Modify: source files the new rustc or clippy rejects. Keep a list in the Deviation Log.
- Modify: `scripts/tcb-unsafe-baseline.toml`, only via `scripts/cellos-sign --write-baseline` and only downward.

## Implementation Steps

1. `rust-toolchain.toml` `channel = N`. Update the workflows, `setup-runner.sh` and both run scripts.
2. Update `test_cellos_sign.py`, `fixtures.py:90` and `source.py:97` to `N`. Nothing validates `source.py:97` against
   Spec 23 §3.1 (denominators are checked only for USABLE cells, `ledger.py:36-55`, and there are none). Phase 4 still aligns
   the prose so the contract stays truthful.
3. Run `ci-local.py run lint`, `run build` (every matrix row), `run aarch64-lint`, `run x86_64-lint` and `run unit-tests`. Fix minimally.
4. Run `scripts/cellos-sign --check --strict`. If tokens dropped, run `--write-baseline` in the same commit.
   Run `python3 scripts/gen-tier1-closure.py --check`.
5. Run `python3 scripts/test_cellos_sign.py` and the app-tier-acceptance tests.

## Success Criteria

- [x] The rg in § Architecture returns only rows classed archival or PAL-governed. Remaining live hits: Spec 23 §3.1 (phase 4) and `tests/rust-std-promotion/rust_src.py` (phase 3).
- [ ] `ci-local.py run` passes for `lint`, `build` (all rows), `aarch64-lint`, `x86_64-lint` and `unit-tests`. Run so far: `unit-tests` passes; the three clippy gates pass with `-D warnings`; `fmt --check` is clean. The `lint` job's remaining failure is `tests/rust-std-promotion` (phase 3). `build` (all rows) has not been run as a job.
- [x] `cellos-sign --check --strict` and `gen-tier1-closure.py --check` pass.
- [x] `test_cellos_sign.py` (156) and `tests/app-tier-acceptance` (88) pass.

The rust-std-promotion tests are expected to fail here, because `rust-toolchain.toml` is a covered input. They are phase 3's criterion.

## Security Considerations

A compiler bump can change codegen under kernel hardening flags (`-Z cf-protection=full`, `code-model=kernel`,
`no-redzone`, AArch64 PAC/BTI). Phase 4's checkpoint runs the boot, domain and hostile lanes on all architectures. Never drop
`RUSTFLAGS` to get a green build.

## Risk Notes

- A removed `#![feature]` that needs a design change: stop and escalate rather than patching around it.
- Perf history restarts at `N`: the perf compatibility key includes the toolchain (`bench_results.py:1197-1214`).
  Phase 4 records new baselines.

## Deviation Log

- Surprise: the new clippy produced 94 diagnostics over 52 files: `chunks_exact_to_as_chunks` 67, deprecated `fetch_update` 13, `needless_range_loop` 6, `needless_bool` 6, `new_instead_of_clear` 1, `needless_late_init` 1, `stable_features` 1. Lists: `reports/lint-A.tsv`, `reports/lint-B.tsv`.
  Why: the lints are new or stricter between 1.97 and 1.102.
  Impact: behaviour-preserving rewrites by two delegated workers (`LintFixKernelCoreLibs`, `LintFixCellsAppLibs`), then `cargo fmt`. Two narrowly scoped `#[allow]`s, each with a reason: `kernel/src/dev/pcie_ecam.rs` `find_vendor_device`, where check order matters on x86_64, and `cells/services/input/src/session.rs` `wipe`, where `Vec::new()` frees memory that `clear()` would keep. All three clippy gates now pass.
  Revert: revert the commit.
- Deviation: posix-shim's `#![feature(c_variadic)]` and `allow(unused_features)` were removed (stable since 1.99).
  Why: `stable_features` warning under `-D warnings`.
  Impact: `libs/posix-shim/src/lib.rs`.
  Revert: restore both lines.
