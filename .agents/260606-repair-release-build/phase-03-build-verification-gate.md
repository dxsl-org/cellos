---
phase: 03
title: Real-Build Verification Gate
priority: P1
status: planned
depends_on: ["02"]
risk: low
---

# Phase 03 — Real-Build Verification Gate

## Context Links
- Report: [../reports/debugger-260605-2239-release-build-broken-at-head.md](../reports/debugger-260605-2239-release-build-broken-at-head.md) ("Why it went unnoticed")
- [run.ps1](../../run.ps1) (the `if (-not (Test-Path $kernel))` stale-binary guard @18)

## Overview
- **Priority:** P1
- **Description:** The release build was broken for multiple commits yet undetected because
  (a) only `cargo check` ran during development — it skips inline-asm codegen and linking — and
  (b) `run.ps1` rebuilds only when the binary is absent, so a stale binary masked the breakage.
  Add a gate that runs the **real** `cargo build --release` (and a boot smoke test) so this class
  of failure fails loudly and immediately.

## Key Insight
`cargo check` ≠ build. It does not validate inline-asm immediates or run the linker — exactly the
two failure modes that broke the kernel (csrsi immediate, PIE relocation, undefined symbols).
Any gate that only runs `check` provides false confidence.

## Implementation Steps
1. **CI job** (if CI exists — check `.github/workflows/`): add a step that runs
   `cargo build --release -p vicell-kernel` on the riscv64 target and fails the job on any error.
   Keep it separate from / in addition to any existing `cargo check` step.
2. **Boot smoke test:** if a headless QEMU boot harness exists (boot_banner integration test),
   wire it into the gate so a green build that fails to boot is also caught. Use a timeout +
   capture the `ViCell>` prompt (or the boot banner) as the pass signal; kill QEMU after.
3. **run.ps1 hardening (optional, low-risk):** add an opt-in `-Rebuild` switch (or a `build.ps1`)
   that forces a fresh `cargo build --release` so devs can re-verify without manually deleting
   the binary. Do NOT change the default boot path behavior.
4. Document in `docs/` (getting-started or a CI note) that release build + boot is the real gate,
   not `cargo check`.

## Todo List
- [ ] CI step: real `cargo build --release` (fail on error)
- [ ] Boot smoke test wired into the gate (if harness exists)
- [ ] Optional: `-Rebuild` switch / `build.ps1` for forced fresh build
- [ ] Doc note: `cargo check` is not a build gate

## Success Criteria
- A deliberately reintroduced asm/link error (e.g. revert the csrsi fix on a scratch branch)
  makes the gate **fail** — proving it catches the class of bug that slipped through.
- Green path: build + boot-to-`ViCell>` both pass in the gate.

## Risk Assessment
- **CI runtime cost (Low).** A release kernel build + short QEMU boot is minutes; acceptable.
- **QEMU availability in CI (Low-Med).** If CI lacks qemu-system-riscv64, gate the boot step
  behind tool-availability and keep the build step unconditional.

## Next Steps
- With a green release build, return to the Reliability track and boot-verify Phase 00
  (`.agents/260605-2107-full-reliability-track/phase-00-fault-path-crash-safety.md`).
