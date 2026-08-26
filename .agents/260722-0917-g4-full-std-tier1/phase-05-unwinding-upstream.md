# Phase 05 — Unwinding + upstream tier-3 target

## Context Links
- Plan: [plan.md](plan.md) · Depends on: P1-P4
- rustc-TCB: `docs/specs/16-rustc-tcb.md` (F5 toolchain pin, P0 incident protocol) · Reliability:
  `docs/specs/12-reliability.md` (never-die supervisor = the panic=abort recovery story)
- Precedent: Hermit target spec `base/hermit.rs` (`panic_strategy: Abort`), platform-support tier-3 listing

## Overview
- **Priority:** P3 (long-term). **Status:** pending. **Now-able:** fork-rebase-cadence plan now; code far later.
- Two long-horizon tracks: (1) **unwinding / `catch_unwind`** to lift the panic=abort limitation; (2)
  promote `x86_64-unknown-cellos` from JSON-target + rust-src fork to a **built-in tier-3 target** and
  establish the ~6-week fork rebase cadence toward upstreaming.

## Key Insights
- **panic=abort first was deliberate** (locked decision 5): the never-die supervisor restarts a crashed
  cell (spec 12), so abort is an acceptable recovery story for G4 launch. Unwinding is additive, not a prereq.
- Hermit ships `panic_strategy: Abort` **at the target-spec level** — a hard choice, not a Cargo opt-in.
  Cellos's built-in target does the same initially; flipping to `Unwind` is a spec change + real machinery.
- Unwinding on a from-scratch OS needs: `eh_frame`/`.eh_frame_hdr` retained by the linker script, a
  personality routine, and an unwinder (Rust's `unwinding` crate — pure Rust, `#![no_std]`, or a ported
  libunwind). `catch_unwind` then works; some async/test crates assume it at task boundaries.
- **Built-in target** (compiler fork) unlocks precompiled std + potential CI + eventual upstream tier-3
  (`host_tools: false`, `std: true`). Until then, JSON-target + `-Zbuild-std` is fine (P1-P4).
- Upstream tier-3 requires a named maintainer, docs, and no burden on other targets — a policy exercise,
  not just code.
- **[m2] The version-lock quad has no CI gate.** Four coupled forks/crates (forked rust-src + forked
  `polling` + forked `mio` + `cellos-abi`) rebase on a cadence but nothing catches a **partial** rebase.
  **Specify a single CI gate:** on any toolchain-pin bump, build **and boot-run** `std-smoke` +
  `tokio-axum-hello` end-to-end (signed, x86_64). The rebase "done" criterion **is** that gate green.

## Requirements
- **Functional:** `std::panic::catch_unwind` unwinds a panicking Tier 1 cell frame instead of aborting;
  `#[should_panic]`-style patterns and unwind-relying crates work. A built-in `x86_64-unknown-cellos`
  (and `aarch64-`) target with precompiled std usable without `-Zbuild-std`.
- **Non-functional:** unwinding is **opt-in per build** where possible (panic=abort remains the default,
  cheapest, and safest for RT/embedded cells); toolchain stays pinned (F5); rebase cadence documented.

## Architecture / data flow
```
panic!() ──▶ (abort mode) sys_exit(nonzero) ──▶ supervisor NotifyOnExit ──▶ restart cell   [default]
panic!() ──▶ (unwind mode) begin_unwind ──▶ personality ──▶ unwinding crate walks eh_frame
          ──▶ run Drop glue ──▶ catch_unwind boundary catches ──▶ Result::Err
linker script retains .eh_frame + .eh_frame_hdr (cell .ld)
```

## Related Code Files
- **Modify (std fork):** `sys/pal/cellos/mod.rs` (`abort_internal` → unwind entry); add
  `library/std/src/sys/personality/cellos.rs` or reuse the generic gcc personality; panic runtime wiring.
- **Add dep:** `unwinding` crate (pure-Rust unwinder) or ported libunwind for the target.
- **Modify:** cell linker scripts (`cell-build` build.rs) to retain `.eh_frame`/`.eh_frame_hdr`.
- **Create (compiler fork, P5b):** `compiler/rustc_target/src/spec/base/cellos.rs` +
  `targets/{x86_64,aarch64}_unknown_cellos.rs`; `bootstrap.toml` target entry; build precompiled std.
- **Create:** `docs/g4-fork-maintenance.md` (rebase cadence, pin-bump protocol, soundness-hole P0 link).

## Implementation Steps
1. **(Now)** Write the fork-maintenance + rebase-cadence doc (~6-week bump; how to re-apply the
   `sys/*/cellos.rs` patch set; how a nightly bump interacts with F5 pin + spec 16 §5.5).
1b. **(m2)** Stand up the single CI gate: any toolchain-pin bump builds + boots (signed, x86_64)
   `std-smoke` + `tokio-axum-hello` end-to-end. "Rebase done" = gate green.
2. Retain `.eh_frame` in cell linker scripts; add the `unwinding` crate; wire a personality routine.
3. Flip a test build to `panic=unwind`; validate `catch_unwind` + Drop-glue-on-unwind in QEMU.
4. Keep `panic=abort` the default; make unwind opt-in per cell profile.
5. (P5b) Promote to built-in target in the compiler fork; build precompiled std; drop `-Zbuild-std` need.
6. (P5c) Prepare the upstream tier-3 submission (maintainer, docs, CI note).

## Todo List
- [ ] fork-maintenance + rebase-cadence doc (now-able)
- [ ] (m2) single CI gate: pin bump → build+boot std-smoke + tokio-axum-hello (signed, x86_64)
- [ ] linker retains .eh_frame/.eh_frame_hdr
- [ ] `unwinding` crate + personality routine
- [ ] `catch_unwind` + Drop-on-unwind verified in QEMU
- [ ] panic=abort stays default; unwind opt-in
- [ ] (P5b) built-in target + precompiled std
- [ ] (P5c) upstream tier-3 submission drafted

## Success Criteria
- QEMU x86_64: a cell built with `panic=unwind` catches a panic via `catch_unwind`, runs `Drop` for
  live guards during the unwind, and continues. Serial oracle: `STD-UNWIND: PASS`.
- A build using the built-in target compiles `std-smoke` **without** `-Zbuild-std` (precompiled std).
- Rebase doc lets a nightly bump re-apply the fork patch set with a bounded, repeatable procedure.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| **Unwinder on bare SAS is genuinely hard** (personality, eh_frame, cross-cell unwind semantics) | H×H | Use the pure-Rust `unwinding` crate; keep panic=abort the default; scope unwind to intra-cell only (never unwind across a cell boundary — that is a kill+restart, spec 12) |
| Fork rebase churn as upstream std `sys/*` refactors again (it already moved once, FIXME 117276) | H×M | Keep the patch set minimal (thin `sys/*/cellos.rs` + cfg arms); track the FIXME; ~6-week cadence; vendor cellos-abi to decouple |
| **[m2] Partial rebase of the 4-fork quad ships silently broken** | M×H | Single CI gate builds+boots std-smoke + tokio-axum-hello end-to-end on every pin bump; gate-green = rebase done |
| Nightly bump silently changes std ABI / soundness (spec 16 §5.5) | M×H | F5 pin; run miri on cellos-abi unsafe; treat a soundness hole as P0 per §5.5 |
| Unwind tables bloat RT/embedded cells | M×M | panic=abort default for RT/Nano profiles; unwind opt-in only for server/PC app cells |
| Upstream tier-3 needs a committed maintainer indefinitely | M×M | Decide maintainer before submission; tier-3 = no CI burden on others, so cost is bounded |

## Security Considerations
- Unwinding must **never cross a cell boundary** — a panic in cell A cannot unwind into cell B's frames
  (would violate isolation). Cross-cell failure stays kill+restart (supervisor). Enforce: cell entry is a
  catch-all boundary that converts an escaped unwind into `sys_exit`.
- A compiler fork widens the TCB surface (spec 16 §1: rustc is the TCB). Minimize compiler-side changes
  (target spec only, no codegen changes); prefer JSON-target + rust-src fork as long as viable.

## Next Steps
- Terminal phase. On completion, G4 "done" per roadmap: full std, tokio ecosystem, os::cellos, zero C in
  Tier 1 TCB, with an upstream-tier-3 path and a maintained fork cadence.
