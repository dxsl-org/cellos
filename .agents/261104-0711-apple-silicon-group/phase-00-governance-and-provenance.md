---
phase: 0
title: "Governance and provenance"
status: in-progress
priority: P2
effort: "0.5d + owner review"
dependencies: []
tier: thinking
checkpoint: true
---

# Phase 00: Governance and provenance (AS-0)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [ADR-0032](../../docs/decisions/0032-apple-silicon-secondary-lane.md); [ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md); [ADR-0029](../../docs/decisions/0029-platform-image-families-and-runtime-detection.md) §5–7; [ADR-0030](../../docs/decisions/0030-license-model-and-copyleft-boundary.md)
- [Research](research/asahi-linux-reference.md); [provenance ledger](provenance.md); [portfolio](../plan-portfolio.md); [lock inventory](../cell-contract-0024-locks.md)

## Overview

Turn the owner's decision ("lower priority than x86, but active, rolled out gradually") into recorded lane rules, correct the m1n1 licence example, register the plan, and prepare the first lock handoff (phase 02a). No code.

## Key Insights

- ADR-0029 required an ADR-0022 amendment before any group-3 work.
- ADR-0022's single-WIP rule would make "active" meaningless while Intel holds the lane, so the Apple lane gets its own WIP slot with absolute Intel precedence.
- m1n1 is GPL-2.0-or-later from v2.0.0. ADR-0030's "m1n1 (MIT)" example is true only up to v1.9.9.

## Requirements

- ADR-0032 written; amendment pointers added to ADR-0022, ADR-0029 (status line, group 3) and ADR-0030 (m1n1 example).
- Portfolio section for the Apple lane: milestone, consumer, acceptance scenario, ceiling.
- HCL §7 (Apple Silicon) with mandatory requirements and an empty table; roadmap pages updated.
- Provenance ledger created; tainted-session rule stated.
- Lock handoff request for phase 02a's file set (below). It becomes effective only when the owner approves it **and** no Intel slice holds those files.

## Architecture

Docs-only. Order: ADR → amendments → portfolio/HCL/roadmap → handoff request → owner checkpoint.

## Related Code Files

- Create: `docs/decisions/0032-apple-silicon-secondary-lane.md`, this plan directory.
- Modify: ADR-0022, ADR-0029, ADR-0030 (amendment notes only), `.agents/plan-portfolio.md`, `docs/hardware-compatibility-list.md`, `docs/roadmap/hardware-tracks.md`, `docs/roadmap/current-focus.md`, `docs/roadmap/haily-personal-assistant.md`, `docs/project-changelog.md`.
- Phase 02a handoff file set (request): `hal/arch/arm/src/aarch64/{boot,el2,trap,trap_el2,vcpu,paging}.rs`, `hal/arch/arm/src/aarch64/backend/{mod,qemu_virt}.rs`, `kernel/linker-aarch64.ld`, aarch64 image build step in `kernel/build.rs` or `scripts/`, `scripts/qemu-aarch64-test*.sh`, aarch64 CI job.

## Implementation Steps

1. Write ADR-0032 and the three amendment notes.
2. Register the plan in the portfolio and the roadmap pages; add HCL §7.
3. Check the lock inventory for the 02a file set. Record which relations must be released.
4. Owner checkpoint: confirm or amend ADR-0032 §2–§8, approve the 02a handoff.

## Todo List

- [x] ADR-0032 and amendment notes
- [x] Portfolio, HCL §7, roadmap, changelog
- [x] Provenance ledger
- [x] Lock inventory check for the 02a file set (2026-11-04: no `hal/arch/arm`, aarch64 linker or `qemu-aarch64` relation in `cell-contract-0024-locks.md`; the inventory is plan-based, so the handoff checks against active Intel slices)
- [ ] Owner checkpoint (lane rules + 02a handoff)

## Success Criteria

- [ ] The owner confirms the lane rules or records amendments in ADR-0032.
- [ ] The 02a handoff names its exact file set and activation condition (no Intel slice holds those files).
- [x] `grep -rn "m1n1 (MIT)\|m1n1\` (MIT)" docs/decisions` shows only the corrected wording with its version qualifier.

## Risk Assessment

- The Intel S1 slice (x86 breadth) is in progress. Its file set is x86-only today, so 02a can likely run in parallel. If a later Intel slice (07a per-CPU) touches `kernel/src/task/smp.rs`, Apple 02c waits.

## Security Considerations

No security gate is relaxed. Apple profiles stay `DmaIsolation::Required`; the hosted profile follows ADR-0031 D-VM.

## Next Steps

Phase 01 (needs hardware) and phase 02a (needs the handoff) can start independently.

## Deviation Log

None.
