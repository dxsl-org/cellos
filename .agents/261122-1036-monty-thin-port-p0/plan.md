---
title: "Monty thin port P0: toolchain ≥1.98 + platform port + x86_64 Tier 2 witness"
description: "Bump the repo toolchain so upstream Monty builds unmodified at its MSRV, add a pin+patch fetch of Monty, and prove a Tier 2 std cell runs Python on x86_64 QEMU"
status: pending
priority: P2
effort: 9d
branch: main
tags: [infra, experimental]
blockedBy: []
blocks: []
created: 2026-11-22
---

# Monty thin port: P0

## Overview

Monty (pydantic/monty, MIT) stays an upstream dependency. Cellos carries only platform patches:
a pin plus `third_party/monty/patches/*.patch`, fetched into a gitignored `third_party/monty-src/`
the way `scripts/build-mlibc.sh` handles mlibc. Language features go upstream, never into the patch set.
User decisions (2026-11-22): bump the repo toolchain inside P0, and P0 ends with an x86_64 Tier 2 QEMU witness;
riscv64 and aarch64 only need to link.

Codebase map: [scout report](./scout-report.md). The red-team review on 2026-11-22 found 2 blocking and 6 major
issues; all are folded into the phases below.

## Phases

| Phase | Name | Status | Checkpoint |
|-------|------|--------|------------|
| 1 | [Select nightly and rebase the rust-std PAL patch](./phase-01-nightly-select-and-pal-rebase.md) | completed | |
| 2 | [Move the repo to the new pin](./phase-02-repo-toolchain-migration.md) | completed | |
| 3 | [PAL compiler-strategy re-decision for the new nightly](./phase-03-pal-compiler-redecision.md) | completed (signed dmin 2026-11-22) | |
| 4 | [Spec 23 amendment and slice-A checkpoint](./phase-04-spec23-amendment-and-checkpoint.md) | completed (event 19; checkpoint green locally) | ✓ |
| 5 | [Monty fetch, platform patch and supply-chain gate](./phase-05-monty-fetch-and-patch.md) | completed | |
| 6 | [Tier 2 monty-smoke cell and x86_64 QEMU witness](./phase-06-monty-smoke-tier2-witness.md) | completed | ✓ |
| 6a | Kernel: 2 MiB user stack for kernel-resolved `/bin/monty-smoke` (large-stack arena), user decision 2026-11-22; see phase 6 Deviation Log | completed | (in 6) |

Slice A (phases 1–4) is a self-contained toolchain upgrade. Slice B (phases 5–6) needs slice A.

## Dependencies

- Phases run in order 1 → 2 → 3 → 4 → 5 → 6. Each phase consumes the previous one's output: pin `N` → repo green on `N` →
  PAL re-signed for `N` → contract re-bound → Monty fetch → witness.
- **Required authority:**
  - Phase 3: a new compiler-strategy decision signed by both COMPILER-INTEGRATION roles
    (`artifacts/compiler-strategy-decision.md:41-43` invalidates the decision on any rustc or rust-src change).
  - Phase 4: steward and reviewer for the ledger `source_amendment` event.
  - Without these sign-offs slice A does not ship.

## Out of scope (P1+, recorded so it is not silently dropped)

- `max_memory` enforcement: an `ostd` allocator hook feeding `monty_types::resource::LIVE_MEMORY`.
- VFS `OsFunctionCall` backend, REPL, `cells/runtimes/python`, `python`/`python-net` split, hostile suite.
- Upstream PRs (stacker gate, cellos backends for getrandom and stacker). These need explicit user go-ahead.
- Running the witness on every PR. `rust-std-lane` runs only on self-hosted or heavy runs (`ci.yml:516`).

## Rollback

- Slice A: revert the commit set. Governance is append-only, so the revert needs a new `source_amendment` back to
  `nightly-2026-05-01` and a PAL record noting the earlier decision is restored. Phase 1 keys the sysroot staging
  cache on the rustc commit, so a rollback cannot reuse stale staging.
- Slice B: delete `cells/demos/monty-smoke`, `third_party/monty/`, the fetch, build and QEMU scripts, and their catalog, lanes,
  allowlist, CI and `deny.toml` rows.
