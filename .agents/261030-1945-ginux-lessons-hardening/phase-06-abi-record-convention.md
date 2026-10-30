---
phase: 6
title: "Boundary-specific ABI records and pre-publication refusal"
status: pending
priority: P2
dependencies: [1]
tier: medium
---

# Phase 06: Boundary-specific ABI records and pre-publication refusal

## Overview

Item 5 is a rule for serialized/untrusted/versioned boundaries, not a reason to wrap every native Rust value or SAS method invocation in a Linux-style bootstrap header.

## Requirements and architecture

- Internal Rust types and same-build typed calls remain typed; do not add magic/version/size fields or dynamic validation to them without a boundary requirement.
- Serialized records, guest/native ABI, persisted metadata and independently updated component boundaries specify identity/type, version policy, exact/bounded length, reserved and unknown-bit handling, ownership and validation-before-effect. Use an existing typed envelope or exact-length contract when adequate; not every record needs all header fields.
- Feature bits represent real negotiated behavior, never test stages or capability grants. A version supported by policy is not rejected merely for being older (CellManifest v1 is currently accepted).
- No mass layout conversion. Any actual public bytes/SDK or error semantic change follows Law-1. Fixed producer/consumer format remains sufficient for admitted same-build components.
- Signature coverage is preserved. Mutations intended to test parsing must be re-signed by an authorized private test pipeline or injected before the decoder in a private fixture. Never patch after signing and then call signature failure parser evidence; never weaken the signer.

## Related Files

`docs/code-standards.md` boundary guidance; actual Spec 17 path found before editing; `libs/api/src/abi/manifest_parse.rs` and existing mutation tests as evidence, not rewrite targets. Extend an isolated x86 image/runner with signed refusal fixtures if existing runtime coverage is insufficient.

## Implementation Steps

1. Classify ABI families by actual trust/update boundary; document current contracts and specific gaps rather than enforcing one header format.
2. Reuse existing manifest decoding and compatibility tests. Construct correctly signed malformed test fixtures with separate positive-control fixtures.
3. Observe unknown version/flag/reserved refusal before task, authority, device or resource publication. Also observe valid supported old/current records admitted.
4. Verify production images/signing paths contain no bypass or injection hook.

## Success Criteria

- [ ] No new per-call serialization/header/copy cost for native SAS C2C.
- [ ] Bad records are refused before effect through the real x86 admission path; valid compatibility records still work.
- [ ] Signature-invalid and format-invalid outcomes are distinguished in evidence.
- [ ] All affected callers/tests/docs are enumerated before any Law-1 change; otherwise zero ABI bytes changed.

## Assumptions

An authorized malformed-fixture signing route may need exact admission. Discover it first; no permission inferred from a development seed.

## Security and risk assessment

This is boundary validation, not permission issuance or LBI proof. Revert docs/private fixtures; retain refusal logs. No persistent migration.

## Deviation Log

SAS review: replaced universal ABI headers with boundary-specific contracts; removed the post-signature mutation workaround.
