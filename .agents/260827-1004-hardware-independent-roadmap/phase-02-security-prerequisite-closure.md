---
phase: 2
title: "Close Hardware-Independent Security Defects"
status: blocked
priority: P1
effort: "8d"
dependencies: [1]
tier: thinking
---

# Phase 02: Close Hardware-Independent Security Defects

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `docs/roadmap/open-risk-register.md` (historical only; source state is rechecked below)
- `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md`
- `.agents/260821-0642-app-tiers-completion/phase-06-tier1-rust-std-pal.md`
- `.agents/260827-1015-sas-caller-owned-range-ledger/plan.md`

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

## Closure Evidence

- `CELLOS-LOADER-SIG-001` remediation is present: the signed payload is the final ELF excluding only its 64-byte signature payload; section headers, names, offsets, relocation metadata, ELF/program headers, and loadable bytes stay authenticated. The boot signing self-test passed in QEMU, but it does not mutate a signed ELF's load-affecting metadata; that focused negative test remains required.
- `PAL-031` has a bounded source implementation under the user's 2026-08-27 authorization for this child only. `GetRandom` validates the original descriptor, preflights the capped span before entropy lookup, and on RV64 SAS accepts only a caller user stack, writable page in the caller Cell's root ELF image, or caller-owned grant. The final check retains the scheduler or corresponding grant-table lock through the ≤64-byte checked copy, serializing segment/stack retirement and grant removal with the commit. Private-domain and non-RV paths retain their existing copy semantics. The implementation compiles in production and `native-domains,test-hooks` tuples; it is not PAL/runtime/promotion approval.
- The targeted QEMU fixture now decodes raw opcode 214, rejects a peer SAS stack, and commits a 65-byte-capacity caller stack through a test-only deterministic entropy source. Runtime evidence is still unavailable: the guest panics before this fixture in unrelated atomic-publication assertions (`AP-05`, `AP-06`, `AP-10`, `AP-11`), while the standard runner is separately blocked by unrelated F1 signing checks. Required hostile classes, retirement/revoke races, manifest rebinding, and named approvals remain open.
- `PAL-019` source and production tuple were checked. `virtio_rng::get_random` returns zero while no real entropy source exists; with `--no-default-features --features production-relay-image`, `dev-weak-rng` is excluded and `GetRandom` returns zero rather than synthetic success. The production RV64 build passed.
- The PAL approval package cannot be re-used: its required kernel-security inventory no longer matches four current inputs (`kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, and `libs/ostd/src/syscall.rs`). The current records remain unsigned `NOT GRANTED`; their checkpoint remains `BLOCKED`.

## Todo List

- [ ] Obtain both required admission approvals for signature encoding after focused signed-ELF metadata-mutation evidence.
- [ ] Obtain named PAL/runtime/security records and rebind the stale approval-input manifest; the user authorized only the bounded ledger/evidence child.
- [ ] Run the isolated opcode-214 hostile/race matrix after repairing the unrelated test-runner gates; do not close `PAL-031` from compilation alone.
- [ ] Record final `PAL-031` approval/checkpoint evidence only after the full matrix passes.
- [x] Close the `PAL-019` production predictable-RNG path independently.

## Success Criteria

- [ ] Load-affecting metadata mutations fail before task creation or relocation writes.
- [ ] Every hostile `GetRandom` pointer class fails before memory access.
- [x] Production builds cannot silently use predictable RNG bytes.
- [ ] No target triple, PAL, admission, or production qualification claim is added.

## Security Considerations

Independent security review is mandatory. No shim may admit legacy unsigned production artifacts.

## Risk Assessment

Canonicalization may invalidate fixtures; update controlled fixtures and provenance tooling together, never weaken verification.

## Next Steps

Prepare three narrow child plans. Build none until its exact approvals are recorded; coordinate `syscall.rs` after Phase 05.

## Deviation Log

- 2026-08-27: Reconciled source against the stale risk register. `CELLOS-LOADER-SIG-001` is implemented but lacks its focused runtime proof. `PAL-019` production fail-closed behavior compiled successfully. The original `PAL-031` defect was source-remediated only under the later bounded-child authorization; it remains blocked by targeted runtime proof and named approval decisions—not new hardware.
- 2026-08-27: The selected PAL-031 direction is a caller-owned SAS range ledger. The scoped plan at `.agents/260827-1015-sas-caller-owned-range-ledger/` preserves the frozen `min(len,64)` ABI and requires final authorization to serialize the ≤64-byte commit against task retirement and grant removal. It remains blocked on the full hostile/race evidence, manifest rebinding, and named approvals.
- 2026-08-27: Approval sequencing is internally blocked: this Phase requires approval before PAL build work, while `PAL-IMPLEMENTATION-CHECKPOINT` requires implemented/evidenced security backing before named signatures. The named owners must explicitly authorize the bounded ledger/evidence child without treating it as PAL/runtime/promotion approval.
- 2026-08-27: The PAL inventory validator failed because four required source digests drifted. Approval records are therefore not approvable as-is; no approval or checkpoint state was inferred or changed.
- 2026-08-27: The user authorized the bounded SAS ledger/evidence child, without granting PAL, runtime, promotion, or named-owner approvals. Source now validates the original `GetRandom` descriptor before entropy, derives SAS ownership from live stack/root-segment/grant records, and serializes the final checked commit with the applicable removal lock. The raw-opcode-214 fixture uses a `test-hooks`-only deterministic source so production keeps zero-byte behavior. Production and native test builds compile; QEMU reaches unrelated atomic-publication failures before the fixture, so `PAL-031` remains blocked.
