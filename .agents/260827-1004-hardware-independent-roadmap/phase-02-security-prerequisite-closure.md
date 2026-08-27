---
phase: 2
title: "Close Hardware-Independent Security Defects"
status: pending
priority: P1
effort: "8d"
dependencies: [1]
tier: thinking
---

# Phase 02: Close Hardware-Independent Security Defects

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `docs/roadmap/open-risk-register.md`
- `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md`
- `.agents/260821-0642-app-tiers-completion/phase-06-tier1-rust-std-pal.md`

## Overview

Close three correctness defects without claiming production admission or rust-std readiness. Each Build remains behind its owning approval set.

## Key Insights

The defects are hardware-independent, but not approval-independent. Signature encoding and PAL backing have different authorization paths.

## Requirements

- Resolve `CELLOS-LOADER-SIG-001` by authenticating all load-affecting ELF metadata before writes.
- Resolve `PAL-031` by validating the complete caller-owned writable range before any mutable access.
- Resolve production `PAL-019`: omit `dev-weak-rng`; return real entropy or observable zero/error.
- Preserve frozen syscall ABI and developer/production posture separation.

## Architecture

`final ELF provenance → signature verification → relocation validation → owned-page write`; `GetRandom range → ownership/bounds validation → entropy source → complete write or explicit failure`.

## Assumptions

- **Claim:** Remediation can land before an external rollback floor after its owning approvals.
  **Confidence:** medium
  **How to verify:** require both Phase 03 approvals for signature encoding; require named G4/PAL approvals plus the implementation checkpoint for PAL work, unless a separately approved kernel-security child supersedes that ownership.

## Related Files

- Modify: `kernel/src/signing.rs`, `kernel/src/loader.rs`, `kernel/src/loader/reloc*.rs`
- Modify after Phase 05 handoff: `kernel/src/task/syscall.rs`
- Modify: `kernel/src/task/drivers/virtio_rng.rs`, `kernel/Cargo.toml`
- Emit: focused evidence for Phase 08; do not edit shared status ledgers

## Implementation Steps

1. Split signature, random-pointer, and entropy work into separately approved children.
2. Obtain both admission approvals—security owner and independent non-author reviewer—before signed-byte changes.
3. Obtain owning G4/PAL approvals and checkpoint before PAL children unless superseded explicitly.
4. Pin signed bytes and deny section/`.rela.dyn` mutation before relocation.
5. After Phase 05 hands off `syscall.rs`, validate the entire random-output range before mutable access.
6. Make entropy completion atomic; add hostile direct-syscall and production-feature tests.
7. Emit evidence; leave floor, anchors, physical cases, runtime, admission, and promotion gates open.

## Todo List

- [ ] Obtain both required admission approvals for signature encoding.
- [ ] Obtain the owning approvals/checkpoint for each PAL remediation child.
- [ ] Close `CELLOS-LOADER-SIG-001`, `PAL-031`, and production `PAL-019` independently.

## Success Criteria

- [ ] Load-affecting metadata mutations fail before task creation or relocation writes.
- [ ] Every hostile `GetRandom` pointer class fails before memory access.
- [ ] Production builds cannot silently use predictable RNG bytes.
- [ ] No target triple, PAL, admission, or production qualification claim is added.

## Security Considerations

Independent security review is mandatory. No shim may admit legacy unsigned production artifacts.

## Risk Assessment

Canonicalization may invalidate fixtures; update controlled fixtures and provenance tooling together, never weaken verification.

## Next Steps

Prepare three narrow child plans. Build none until its exact approvals are recorded; coordinate `syscall.rs` after Phase 05.

## Deviation Log

None.
