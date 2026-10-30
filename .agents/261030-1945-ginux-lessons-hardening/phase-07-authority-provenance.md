---
phase: 7
title: "Issuer-derived authority provenance and isolated SMP witness"
status: pending
priority: P1
dependencies: [1]
tier: thinking
---

# Phase 07: Issuer-derived authority provenance

## Overview

Apply item 4 to Cellos's typed issuer model under Spec 25 §§3.1–3.3. Ginux also separates capability authority from process ancestry; Cellos must use its own issuer-local model rather than assume one sole-root table. Memory safety, an ELF name, a signature and a newer generation each fail to grant semantic resource rights.

## Requirements and architecture

- Remove the old invariant `CapSet(child) ⊆ boot_ceiling(path) ∩ CapSet(spawner)` as a universal rule. Current launch branches differ: Root intersects its path ceiling; Ceiling intersects its reviewed ceiling; User intersects caller caps (`governed_spawn.rs:127-149`). Kernel-authorized bootstrap/profile grants must not be mistaken for amplification merely because init lacks the same flattened bitset.
- Validate each effective grant against its actual issuer/root authorization, delegator live rights (if delegated), destination immutable admission ceiling, receive policy and applicable operator policy (Spec 25:44-56). Rights narrowing is non-increasing, allowing equal rights where authorized; require delegable authority and bounded acyclic lineage, not a strict reduction on every hop.
- Treat root issuance, delegation and reference close/revoke distinctly. An issuer can create an authorized new object; it need not possess a parent handle to that not-yet-created object. Creator death is not automatically revocation of an independently held grant.
- Kernel checks kernel handles and attests holder/transfer identity. VFS/NET/state issuers retain their own semantic authorization tables; no duplicate global kernel ACL and no attempt to decode every opaque service-local handle.
- Observer-read/service-write split is demonstrated only for an actual resource supporting separate operations and admitted consumers. It is not a universal CapSet mask retrofit.
- Keep boot authority ceiling tied to existing authenticated artifact/policy admission; a signed canonical sole-root manifest and sealing changes remain a proposed ADR, not new authority inferred from a signature.
- Bound checks on invocation; no whole-CSpace audit, lineage scan of arbitrary size, signature verification, allocation or copy added on every local SAS request.
- An offline/quiescent audit uses bounded snapshots/versioned retry without reversing scheduler/authority lock order; reports exact coverage. It may verify issuer evidence references but cannot certify opaque issuer ACLs by itself.

## Related Files

Actual root issuance/delegation/publication sites in `kernel/src/cell/cspace.rs`, loader launch profiles and `governed_spawn.rs`; issuer-specific tests at the owning service. The legacy `CapTable` is deleted only if reference and resource-lifetime closure proves no live consumer; preserve CSpace file-backing helpers. LSP references precede exported-symbol removal.

## Implementation Steps

1. Census real root/bootstrap/profile issuers, delegation/receive paths and service-local resources against Spec 25. Identify exact missing checks without introducing a second registry.
2. Correct only uncovered paths; migrate all real consumers. Track public/persistent/semantic deltas for Law-1, not merely struct sizes.
3. Reuse/extend an isolated lifecycle `test-hooks` image with explicit 1/2-CPU launch and positive AP work. It must not overwrite production witnesses or widen normal bootstrap rights.
4. Exercise excessive rights, wrong object type/issuer/holder, stale incarnation, expired/revoked lineage and forbidden post-start receipt. Positive controls cover legitimate kernel bootstrap, equal-right delegation and actual authorized issuer minting.
5. Draft the signed-boot-manifest ADR with root/update/seal/recovery boundaries and a named observer/service example. No frozen ABI implementation in this phase without its approval.

## Success Criteria

- [ ] Actual runtime grants have valid issuer-derived provenance within recipient ceilings; ordinary caller escalation is refused without rejecting reviewed bootstrap/profile launches.
- [ ] Issuer-local object negatives run through the real issuer; a kernel snapshot alone is not evidence for VFS/NET semantics.
- [ ] Same authority behavior works for SAS and Domain Cells; no mandatory per-request allocation, copy or global audit.
- [ ] 1/2-CPU private witness runs with positive AP marker; production lifecycle regression remains unchanged.
- [ ] New signed-manifest ADR stays proposed; all actual Law-1 changes remain independently gated.

## Assumptions

Existing issuer/caller closure and exact source grants may have advanced since prior recon; reread before coding. Legacy CapTable deadness is PRIOR until references are verified.

## Security and risk assessment

A broad Kernel-issuer escape tag is forbidden. Quiescent inspection cannot acquire all locks arbitrarily. Undo by reverting and cold boot; no authority is restored by replaying old handles.

## Deviation Log

SAS review: issuer provenance replaces process ancestry; equality-preserving attenuation and service-local authority remain intact; private witness has no dependency on new ABI headers.
