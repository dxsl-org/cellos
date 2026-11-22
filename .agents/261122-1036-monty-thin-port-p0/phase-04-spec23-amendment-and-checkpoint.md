---
phase: 4
title: "Spec 23 amendment and slice-A checkpoint"
status: completed
priority: P1
effort: "1d"
dependencies: [3]
tier: medium
checkpoint: true
---

# Phase 4: Spec 23 amendment and slice-A checkpoint

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

This phase re-binds the ratified Spec 23 text to the new pin through the ledger's prose-only `source_amendment`
event. It then runs the full slice-A checkpoint on all three architectures, including the boot, Tier-2 domain and
hostile lanes.

## Requirements

- Functional: Spec 23 §3.1 (`docs/specs/23-native-sdk-contract.md:107-119`) names `N`. Nothing else in the spec changes.
  The matrix digest covers only §5 (`scripts/app_tier_acceptance/source.py:44-58`), so the `source_amendment` rules hold
  (`ledger.py:260-274`).
- Functional: exactly one new ledger event in the push (trusted-baseline rule; `docs/project-changelog.md` 2026-11-20 entry).
- Functional: the spec edit (A), its archived snapshot and the ledger event (B) land in **one push**, after reviewer approval.
  A alone turns CI red: `app_tier_acceptance/validator.py:41` compares the live spec digest with `source_binding`.
- Functional: new perf baselines are recorded for `N`. History resets because the compatibility key includes the toolchain.

## Architecture

```mermaid
sequenceDiagram
  participant S as Steward
  participant R as Reviewer
  S->>S: commit A: Spec 23 §3.1 prose (pin N)
  S->>S: archive docs/evidence/spec23-native-sdk-contract-<sha12>.md (bytes of A)
  S->>S: commit B: source_amendment event (ratified_revision = A), event 14 as template
  S->>R: request review of A+B
  R-->>S: approve
  S->>S: push A+B together; CI validates against trusted baseline
```

## Assumptions

- **Claim:** event 14 is a valid template, and `--baseline`/`--baseline-root` validate a candidate locally.
  **Confidence:** high (verified by red-team). **How to verify:** `jq '.events[13]' docs/app-tier-acceptance-ledger.json`.
- **Claim:** the steward and reviewer roles are available.
  **Confidence:** unknown. **How to verify:** ask the user for names; never self-approve.

## Related Files

- Modify: `docs/specs/23-native-sdk-contract.md` (§3.1 only)
- Create: `docs/evidence/spec23-native-sdk-contract-<sha12>.md`
- Modify: `docs/app-tier-acceptance-ledger.json`, `docs/app-tier-acceptance-matrix.md`
- Modify: `docs/roadmap/runtime-and-platform-tracks.md:107-134`, `docs/project-changelog.md`, `CHANGELOG.md`
- Modify: perf baseline files as written by the perf workflow tooling

## Implementation Steps

1. Edit §3.1 (lead sentence + three tuple rows). Commit A.
2. Archive A's bytes under the `source.snapshot_path` naming rule.
3. Build the event from event 14. Compute digests with the `scripts/app_tier_acceptance` helpers. Commit B.
4. `python3 scripts/validate-app-tier-acceptance.py --baseline <prior ledger> --baseline-root <checkout at prior commit>`.
5. Update the roadmap track text and the changelogs. Record new perf baselines with the perf tooling.
6. Run the checkpoint below; get reviewer approval; push A+B.

## Success Criteria

- [x] `validate-app-tier-acceptance.py` passes against the trusted baseline `706bda5570` (`PASS: C9=NOT_COMPLETE`). The bare call requires a baseline once a new event exists. `tests/app-tier-acceptance` passes 88/88.
- [x] Checkpoint, all run locally with `ci-local.py run` on `N`. Pass: `lint`, `build` (3 rows), `aarch64-lint`, `x86_64-lint`, `unit-tests`, `qemu-boot`, `qemu-aarch64-boot`, `qemu-x86_64-boot`, `rust-std-lane`, `x86-c2c`, `rv64-tier2-disk`, `qemu-x86-tier3-hostile`, `f1-admission` and `boot-suite`. `security`: cargo-deny on all three workspaces passes, and `cargo audit` passes after installing cargo-audit locally (exit 0; one already-allowed advisory, RUSTSEC-2026-0253 `lru`).
- [x] `main` was green before the change for these jobs. The two local-only failures are recorded in the Deviation Log and are not baseline failures.

## Evidence

- Commit A `fbd3bfb832` (Spec 23 §3.1 plus snapshot `docs/evidence/spec23-native-sdk-contract-4106d2612594.md`; matrix digest `f742a6ec…` unchanged).
- Commit B `be1b8277e0` (event 19 `source_amendment`, steward lungmat8, reviewer datgausaigon; approved per the user, 2026-11-22).
- Follow-ups `07b412dae7` (seed fixture re-derived), `5a689a83dd` (`LARGE_STACK_ARENA` classified; generated status refreshed) and the `--discard-locals` commit (below).
- Not run: the KVM, hardware and nightly-tier lanes outside this list. Perf baselines were not recorded: no live perf run exists for `N` yet, and history restarts at the new toolchain key.

## Security Considerations

The hostile and domain lanes are included because a compiler bump can silently change hardening codegen.
A regression there blocks slice A regardless of other results.

## Risk Notes

- Some heavy lanes (KVM, hardware) may not run locally. Record which jobs ran where. A job that did not run is
  reported as not run, never as passed.

## Deviation Log

- Surprise: `rv64-tier2-disk` and `boot-suite` failed with `FAIL: cell_store.img (40 MB) exceeds the 32 MB P6 window`. The new LLVM keeps tens of thousands of RISC-V `.L0` assembler labels in the symbol table (cellbox: 19k → 62k symbols, `.symtab`+`.strtab` +1.5 MB). `.text` is actually smaller (1.56 → 1.44 MB).
  Why: an LLVM/rustc change between the two nightlies.
  Impact: `libs/cell-build` now passes `--discard-locals` to the linker for every cell. Cellbox went from 5.1 MB to 3.0 MB (old toolchain: 3.7 MB) and ocel to 3.9 MB (old: 4.3 MB). Both lanes now pass. x86_64/aarch64 boots and the Monty witness were re-run with the flag.
  Revert: remove the link arg (the P6 window then overflows again on `N`).
- Deviation: in local `boot-suite`/`rv64-tier2-disk` runs, `doom`/`tetris-lua` fail to link (no picolibc/newlib), the same as CI, where the build script treats them as optional. `zig-hello` fails because `gen_disk.ps1` builds a Windows-style path (`zig-out\bin`) when zig is installed on Linux. CI does not install zig, so the lanes were re-run with `~/.local/bin` (zig) removed from `PATH`. Neither is caused by this plan.
  Why: local environment differs from CI.
  Impact: none on CI.
  Revert: n/a.
- Deviation: the trusted-baseline validation used a `git clone --shared` checkout of `706bda5570` as `--baseline-root`. An archive extract fails `ratified revision does not contain exact source`, because the validator needs git history.
  Why: tool requirement.
  Impact: none.
  Revert: n/a.
