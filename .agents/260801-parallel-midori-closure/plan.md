---
title: "Parallel Midori Closure Plan"
description: "Bounded parallel workstreams to close Midori phase 02 runtime evidence and phase 04 /bin/vfs fold, with phase 07 peer-death readiness audited only."
status: blocked
priority: P1
effort: 2.5d
branch: feat/wx-post-reloc-and-f1-signing
tags: [critical, runtime, security]
blockedBy: [phase-01-p02-runtime-verify-fast-IPC-gap]
blocks: [midori-phase08-stack-sizing, package-distribution-bin-write, cap-revocation-phase02]
created: 2026-08-01
---

# Parallel Midori Closure Plan

## Verdict

Run two implementation streams plus one audit stream in separate worktrees. Accept A and B for implementation. Reject C for implementation because no async IPC submission currently registers target dependencies; keep C as a readiness audit/ADR package only. Reject broker/shell deprivilege because it starts service-ID/broker work outside the requested bound.

## Workstreams

| Stream | Branch | Worktree | Phase | Status |
|---|---|---|---|---|
| A | `codex/midori-p02-runtime-verify` | `.worktrees/midori-p02-runtime-verify` | [Phase 01](./phase-01-p02-runtime-verify.md) | partial / blocked |
| B | `codex/midori-vfs-region-fold` | `.worktrees/midori-vfs-region-fold` | [Phase 02](./phase-02-vfs-region-fold.md) | complete (branch-complete; pending merge gate) |
| C | `codex/midori-peer-death-cq-audit` | `.worktrees/midori-peer-death-cq-audit` | [Phase 03](./phase-03-peer-death-cq.md) | complete (audit-only) |

## Dependency Graph

```text
A ───────────────┐
B ──► integration├──► main branch merge gate
C audit ─────────┘
```

- A is verification-only and can run immediately.
- B depends on already-present mask and boot-ceiling widening; source confirms those prerequisites.
- C has CQ foundation, but lacks a real async IPC submission that records target dependency. It must produce an ADR/readiness report, not code.
- Evidence reports: [Phase 01 runtime verification](./reports/midori-p02-runtime-verify-report.md), [Phase 02 region fold](./reports/midori-vfs-region-fold-report.md), [Phase 03 readiness audit](./reports/peer-death-cq-readiness.md) and [ADR stub](./reports/adr-peer-death-cq-owner-stub.md).

## Merge Order

1. Merge A first if it changes test harness only; it establishes the runtime baseline.
2. Merge B second; it changes admission/policy and must be validated before scheduler changes.
3. Do not merge C code in this batch. Merge only its report/ADR if useful; implementation waits for an async IPC submission owner.
4. Phase 02 is branch-complete in its worktree and remains pending the merge gate.

## Shared Integration Gate

- `CARGO_INCREMENTAL=0 cargo check -p vicell-kernel`
- `CARGO_INCREMENTAL=0 cargo check -p service-vfs -p app-vfs-test -p app-srv-test`
- `scripts/build-test-hooks-ci.sh`
- `scripts/qemu-boot-test.sh` or the existing boot-suite lane that waits for `vfs-test`.
- `scripts/build-srv-test-ci.sh` then `cargo test --manifest-path tests/integration/Cargo.toml --test redoxfs-srv -- --nocapture`

## Stop Conditions

- Stop B if `/bin/vfs` with `0b1111` policy cannot parse as valid before raw-grant removal.
- Stop B if VFS loses `block_regions == 0b1111` after policy intersect.
- Stop C at audit if implementing target-gone would require inventing a submission owner, new syscall number, new `WaitCompletion` source bit, or wire-frame shape change.
- Stop all merges if A shows phase 02 runtime failure unrelated to harness flakiness.

## Unresolved Questions

- Exact command for the full 3-arch suite differs by local environment; each stream must record the command actually run.
- C's open question is not the result code first; it is which real async IPC submission owns target dependency registration.
