---
phase: 4
title: "Document Open Issues And API Drift"
status: completed
priority: P1
effort: "4h"
dependencies: [1]
tier: thinking
---

# Phase 4: Document Open Issues And API Drift

## Overview

Record verified code issues and stale API references without fixing implementation code. This gives the next implementation plan a trustworthy backlog.

## Requirements

- Functional: document production trust gaps, POSIX syscall gaps, net-broker partial wiring, net wake/poll status, and stale network docs paths.
- Non-functional: issue status must separate defect, deferred design, host-gated evidence, and historical note.

## Architecture

Data flow: Phase 1 issue evidence enters security/API/known-issue docs; transformed output is a small issue register with owner, evidence, impact, next verification, and rollback notes.

## Assumptions

- **Claim:** A separate `docs/known-issues.md` is clearer than burying verified issues inside roadmap prose.
  **Confidence:** medium
  **How to verify:** if roadmap split already has a concise current-status issue table, skip the new file and link there.

## Related Files

- Modify: `docs/security-model.md`
- Modify: `docs/network-api.md`
- Create or modify: `docs/known-issues.md`

## Implementation Steps

1. Security issue: cite zero production keys in `kernel/src/signing.rs:33` and `kernel/src/policy.rs:87`; explain `signing-required` and `policy-required` are opt-in features from `kernel/Cargo.toml:83` and `kernel/Cargo.toml:86`.
2. POSIX issue: cite `kernel/src/task.rs:1270`, `:1404`, `:1408`, `:1455`, and `:1460`; enumerate syscall callers from Phase 1.
3. Net-broker issue: cite dispatch TODOs in `cells/services/net-broker/src/main.rs` and unwired companion modules.
4. Net wake issue: cite `cells/services/net/src/main.rs:66` and `:192`; keep roadmap claim at `docs/project-roadmap.md:1352` as partial/deferred if retained.
5. Network API drift: replace stale `cells/services/net/src/lib.rs` and `tests/integration/network_loopback.rs` references with actual files or mark removed.
6. Add next-action labels: docs-only follow-up, needs design plan, needs hardware, or needs implementation.

## Success Criteria

- [ ] Every open issue includes source evidence and a next owner.
- [ ] No issue is marked complete without a verification command or hardware evidence.
- [ ] Network API references point to existing tracked files.

## Security Considerations

This phase is security-sensitive because wording can change threat posture. Default statement: dev/test signing exists; fleet-secure admission remains open until immutable key provisioning, mandatory features, and secure boot anchoring are proven.

## Risk Notes

- High likelihood x high impact: security wording may accidentally weaken or overstate guarantees. Mitigation: use exact feature names and placeholder-key evidence.
- Medium likelihood x medium impact: issue register can duplicate roadmap current-status. Mitigation: make one canonical owner and link from the other.
- Rollback: remove issue register changes and restore touched docs; no source state changes. Irreversible part: none.

## Deviation Log

Open risks were consolidated into `docs/roadmap/open-risk-register.md` rather
than creating a separate `docs/known-issues.md`.
