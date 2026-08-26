---
title: "Application tier taxonomy alignment"
description: "Lock Cellos app tier/runtime/SDK terminology and plan non-breaking code migration."
status: completed
priority: P2
effort: 15h
branch: main
tags: [docs, architecture, sdk, manifest, compatibility]
created: 2026-08-20
---

# Application Tier Taxonomy Alignment

## Decision

Canonical app execution classes are:

- Tier 1: Trusted SAS Cell.
- Tier 2: Native Domain Cell, accepted but not implemented.
- Tier 3: VM Guest.

`Tier 1b` and `Tier 3b` are legacy guide aliases only. `rust-no-std`,
future `rust-std`, `ffi-posix`, `lua`, and `linux-guest` are runtime profiles.
The SDK uses named modules, not numbered tiers. Product roadmap remains
Stage G1-G5.

## Phases

| Phase | Status | Effort | Owner files | Depends on |
|---|---:|---:|---|---|
| [01 Docs taxonomy sweep](phase-01-docs-taxonomy-sweep.md) | completed | 5h | `docs/` active docs only | accepted decision |
| [02 Code terminology migration](phase-02-code-protection-class-aliases.md) | completed | 4h | `libs/api`, `libs/ostd`, `kernel`, `libs/zig-syscall` | phase 01 |
| [03 Verification gates](phase-03-verification-gates.md) | completed | 3h | tests/scripts/docs checks | phase 02 |
| [04 Future Tier 2 split](phase-04-native-domain-followup.md) | completed | 3h | design/spec only; runtime implementation remains separately gated | phase 02 |

## Dependency Graph

`ADR/docs -> compatibility aliases -> parser/tests/build checks -> later Tier 2 mechanism`.

## Open Questions

- `docs/coding.md` and `docs/engineering-standards.md` are absent in this checkout;
  `docs/code-standards.md` was used as the local standards fallback.
- `.claude/scripts/set-active-plan.cjs` is absent, so active-plan sync could not run.

## Progress

All four plan phases are complete. Phases 01–03 delivered the taxonomy,
compatibility aliases, and verification gates. Phase 04 delivered the accepted
Tier-2 design/implementation gate in Spec 22; it did not implement native
domains, loader admission, page-table switching, or runtime containment.

## Evidence

- `git status --short` showed Spec 22 and amended project/spec/roadmap docs,
  with no kernel or library source changes in the current worktree.
- `git diff --check` completed with no output and exit 0.
- `docs/specs/22-native-domain-cell-implementation-gate.md` is present and
  explicitly states “Not implementation approval”; it defines architecture,
  scheduler, syscall-copy, IPC/grant, DMA, negative-test, admission, and
  rollback gates.
- The amended Specs 18/19 and security model cross-reference Spec 22 and retain
  the statement that Tier 2 is accepted but unimplemented.

## Handoff

The taxonomy alignment and Tier-2 design gate are complete. A future
implementation plan must independently approve and implement the gates in Spec
22, including hostile native negative tests and rollback. No Tier-2 runtime
behavior is authorized by this plan.
