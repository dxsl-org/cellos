---
phase: 2
title: "Private Protocol and DEV Separation"
status: blocked
dependencies: [1]
unblocks: [3, 4, 5]
priority: P1
tier: thinking
---

# Phase 2: Private Protocol and DEV Separation

## Context Links

- [Plan](./plan.md)
- [Phase 1 admission gate](./phase-01-admission-and-asset-baseline.md)
- [Approved entry contract](../260825-1726-kms-silo-production-root/spec.md)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md)
- [Codebase scout](./scout-report.md)

## Overview

Define the sole private AP↔STM32 authority contract as a `no_std`, no-allocation, fixed/bounded Rust crate, freeze public KMS opcode 9–14 bytes, and reject every DEV_REFERENCE marker from production artifacts. **Status: BLOCKED on completed Phase 1**; completion unblocks only parallel Phases 3, 4, and 5.

## Key Insights

- The existing public ABI in `libs/types/src/kms/` is immutable; authority trust is added behind it, not by adding fields or opcodes.
- The AP, VFS, supervisor, service-net, and transport are untrusted. They may relay typed bytes but cannot assert boot, state, time, profile, pending key, commit, or signing results.
- A closed operation set makes generic signing, digest, TPM, NV, raw time, profile assertion, and firmware update unrepresentable.
- `DEV_REFERENCE` is a terminal classification for this lane, not a promotion stage or feature switch.

## Requirements

- Create `authority_protocol` with `#![no_std]`, no `alloc`, no serde-dependent implicit layout, fixed-width integer types, explicit little-endian codecs, checked arithmetic, compile-time maxima, and caller-provided buffers.
- Use a fixed header containing magic, protocol version, `DEV_REFERENCE` lane tag, message kind, payload length, request ID, and reserved-zero bytes; every message has an exact or declared bounded length.
- Bind authenticated responses to device ID, authority ID, boot epoch, request ID, operation, payload digest, and authority signature; unknown version/kind, trailing bytes, nonzero reserved bytes, oversized lengths, replay, or response mismatch fails closed.
- Expose only: `OpenBoot`, `ReadCommittedRelayState`, `RequestSignedTime`, `AcceptSignedTime`, `BeginRelayEnrollment`, `ReadRelayCsrChunk`, `ValidateAndStageRelayProfile`, `ConsumeStagedRelayProfile`, `CommitRelayGeneration`, `AbortRelayEnrollment`, `GetRelayActivePublicKey`, and `SignTls13ClientCertificateVerify`.
- `ValidateAndStageRelayProfile` accepts one bounded typed profile record, independently binds the pending TPM public area, and creates a durable single-use `StagedProfileReceipt`; `ConsumeStagedRelayProfile` accepts only `{generation, policy_epoch, profile_digest}` from public opcode 13 and cannot create trust.
- Define closed `AuthorityFault` codes for malformed/version/length/state errors, identity or challenge mismatch, stale/replay/regression, time invalid/unavailable, profile rejection, receipt absent/consumed, provider split-brain, persistence failure, and sealed authority; faults carry no strings or secret material.
- Define explicit `AuthorityMode`, `BootState`, `TimeState`, and `RelayProfileState` transitions. Illegal transitions and every integrity/floor ambiguity enter or preserve `Sealed`; runtime protocol contains no provisioning/unseal/reset command.
- Preserve byte-for-byte request/response fixtures for public KMS opcodes 9–14, including opcode numbers, status bytes, payload order/length, errors, chunk bounds, and opcode 14 active-only semantics.
- Reject protocol lane tags, root-stream assets, STM32/SLB9672 DEV anchors, AWS DEV time anchors/certificates, feature names, and manifest markers from production candidates while retaining exact exit code `3` and `BLOCKED_BY_ADR_0006` text for otherwise-valid posture.
- This phase remains the sole owner of `scripts/check-production-relay-image.py`: Phases 3–7 hand off closed ASCII marker names only (root-stream, STM32/SLB9672 DEV anchors, AWS DEV signed-time, relay/CA DEV identifiers) through their reviewed manifests — never checker edits — and every handed-off name enters the frozen marker set here before the handing-off phase proceeds.

### Hard Stops

- Stop if Phase 1 is not signed `READY_FOR_PHASE_02` or if implementation requires changing any public opcode/payload byte in 9–14.
- Stop if any `Raw`, `Generic`, `Vendor`, `Execute`, generic sign/digest/TPM/NV/time, firmware-update, unseal, or caller-selected key operation appears in the request model.
- Stop if a variable length can allocate, truncate, wrap, accept trailing bytes, or exceed its compile-time maximum.
- Stop if production can accept, strip, relabel, or promote a DEV artifact; there is no fallback or promotion switch.

## Architecture

`public KMS 9–14 (frozen) → KMS adapter [later Phase 6] → authority_protocol typed frame → untrusted carrier → STM32 authority`

`FrameHeader + Closed Request/Response/Fault + authenticated binding` is transport-independent. `AuthorityState` owns transition legality; only typed operations can mutate it. Public byte fixtures are an independent compatibility oracle. Production scanning checks both selected features and binary/manifest markers before emitting the unchanged ADR block.

## Assumptions

- **Claim:** STM32 and AP Rust targets can consume the same core-only crate without target-specific dependencies. **Confidence:** medium. **How to verify:** compile the crate for the exact Phase 1-recorded STM32 target and the VF2/host fixture target before either adapter is written.
- **Claim:** One bounded profile record can hold the repository's maximum canonical relay chain without heap allocation. **Confidence:** medium. **How to verify:** derive the maximum from `libs/types/src/kms/payload/enroll.rs` and current chain policy, then compile-time assert encoded maximum ≤ frame maximum.
- **Claim:** Existing opcode 9–14 tests cover every request and response byte shape. **Confidence:** low. **How to verify:** enumerate every payload type referenced by `KmsOpcode::{BeginRelayEnrollment..GetRelayActivePublicKey}` and require one literal golden vector per direction and error shape.
- **Claim:** All new DEV artifacts can carry stable ASCII lane markers detectable by the production scanner. **Confidence:** medium. **How to verify:** inventory later-phase feature, manifest, certificate/anchor, and bundle names and scan synthetic binaries with markers split across read chunks.

## Related Code Files

- **Owner — modify:** `Cargo.toml` (add only workspace member `libs/authority-protocol`).
- **Owner — create:** `libs/authority-protocol/Cargo.toml`, `libs/authority-protocol/src/lib.rs`, `libs/authority-protocol/src/wire.rs`, `libs/authority-protocol/src/message.rs`, `libs/authority-protocol/src/fault.rs`, and `libs/authority-protocol/src/state.rs`.
- **Owner — create:** `libs/authority-protocol/tests/wire_fixtures.rs`, `libs/authority-protocol/tests/state_transitions.rs`, and `libs/authority-protocol/tests/rejection.rs`.
- **Owner — create/modify:** `libs/types/src/kms/tests/authority_compat.rs` and `libs/types/src/kms/tests/mod.rs` for literal public 9–14 golden vectors.
- **Owner — modify:** `scripts/check-production-relay-image.py` and `scripts/test_check_production_relay_image.py` for DEV feature/marker rejection and exact block preservation.
- **Read-only frozen boundary:** `libs/types/src/kms/model.rs`, `libs/types/src/kms/payload/enroll.rs`, and `libs/types/src/kms/payload/tls.rs`; do not alter their opcode or payload encodings.
- **Out of scope:** AP/STM32 transport adapters, firmware, hardware provisioning, AWS deployment, KMS dispatch cutover, and public ABI additions.

## Implementation Steps

1. Enumerate every field, byte order, exact/bounded encoded length, maximum, authentication binding, request/response pair, fault code, and legal state transition in the crate API; reject unknown and reserved values.
2. Implement manual encode/decode into caller-owned slices with length checked before indexing, exact consumption, reserved-zero enforcement, stable error precedence, and zeroization of temporary sensitive buffers where applicable.
3. Implement the closed request/response enums and bounded wrapper types; make prohibited generic operations absent rather than runtime-disabled.
4. Implement the state transition table: boot challenge opening, protected-state read, signed-time acceptance/floors, enrollment, root-validated staged receipt, consume, prepare/commit/abort, active-only read, typed TLS signing, and absorbing seal conditions.
5. Add literal golden vectors for every private message/fault and every existing public KMS 9–14 request/response; compare exact bytes, not round-trip alone, and add truncation/extension/overflow/reserved-byte mutation cases.
6. Extend the production checker with a closed DEV marker set (`DEV_REFERENCE`, root-stream, STM32H573 DEV authority, SLB9672 DEV anchor, AWS DEV signed-time, and their manifest/features) and split-chunk binary scans.
7. Run `cargo test -p authority-protocol`, `cargo test -p types`, and `python3 scripts/test_check_production_relay_image.py`; archive exact fixture hashes and command output as Phase 2 evidence.
8. Compare the finished operation set against PERSIST/TIME/BIND/LANE requirements; any missing typed operation changes this phase before downstream adapters begin, while any generic operation blocks the lane.

## Todo List

- [x] Private codec, message/fault/state models, bounds, and authentication bindings are frozen.
- [x] Literal public KMS 9–14 and private-protocol fixtures pass byte-for-byte.
- [x] Forbidden generic operation search and malformed-frame/state-transition scenarios pass.
- [x] Production candidates reject every DEV feature/marker and retain the exact ADR-0006 block.

## Success Criteria

- [x] The crate builds as `no_std` without `alloc`; maximum encoded sizes are compile-time asserted and all decoders consume exactly one bounded frame.
- [x] Public opcode/payload fixtures 9–14 are byte-identical to the pre-phase vectors, and opcode 14 represents active state only.
- [x] No AP-visible generic sign, digest, TPM, NV, time, profile assertion, update, provisioning, unseal, or arbitrary execution request exists.
- [x] Replay, mismatch, malformed length, unknown/reserved values, illegal transitions, missing/consumed receipts, and split-brain produce typed fail-closed faults; ambiguous protected state seals.
- [x] Every synthetic production artifact carrying any DEV marker exits `1`; an otherwise-valid marker-free candidate still exits `3` with exact `BLOCKED_BY_ADR_0006` text.
- [ ] Only Phases 3/4/5 become eligible; parent Phase 4 stays blocked until real AC-001..AC-011 hardware/cloud evidence and independent review.

## Risk Assessment

- **High:** duplicate codecs in later firmware/AP layers can drift; make this crate the exclusive wire/type owner and require adapters to import it.
- **High:** a “typed” opaque payload can recreate generic execution; every payload must have field-level bounds and operation-specific validation/state.
- **High:** marker-only separation can miss renamed assets; combine closed feature policy, manifest classification, and artifact scanning, and never provide a promotion path.
- **Medium:** large fixed profile buffers pressure MCU RAM; derive one justified maximum from frozen policy and reject oversize input rather than allocate or truncate.

## Security Considerations

Treat the carrier as hostile: authenticate bindings end to end, compare identities/digests in constant time where secrets or authenticators are involved, reject before state mutation, and return non-oracular fault codes. Protocol privacy does not establish trust. Provisioning and irreversible controls are offline operator checkpoints, never wire commands.

## Next Steps

After all criteria pass, Phases 3, 4, and 5 may consume `authority_protocol` in parallel. Phase 6 later owns the only KMS adapter; no downstream phase may fork the wire model or weaken DEV rejection.

## Deviation Log
- 2026-08-26 — Decision: the red-team simplicity review found Phases 3–5 planning parallel modifications of the production checker, risking marker-set drift. Resolution applied pre-execution: checker ownership is solely Phase 2; downstream phases deliver closed marker names through reviewed manifests only, merged into the frozen marker set here. No DEV-rejection behavior or exit-code contract changed.

- Append Decision/Deviation/Surprise entries during execution with reason, impact, and revert; escalate irreversible, generic-surface, frozen-ABI, or production-separation changes.
- 2026-08-26 — Decision: operator approved the software track, so this phase's host-verifiable deliverables (authority-protocol crate, fixtures, checker) may start before Phase 1 admission signs. All success criteria remain host-only; nothing here claims hardware or AC evidence.
- 2026-08-26 — Result: the authorized SOFTWARE_HARNESS slice is complete and host-verified. The phase remains blocked on Phase 1 and claims no hardware/cloud acceptance evidence.
