---
phase: 6
title: "Frozen-ABI KMS Authority Integration"
status: pending
priority: P1
dependencies: [3, 4, 5]
tier: thinking
---

# Phase 6: Frozen-ABI KMS Authority Integration

> **Required — deviation log:** Record every decision, deviation, or surprise when it occurs. Escalate any irreversible, authorization-boundary, or locked-contract change rather than selecting a fallback.

## Context Links

- Parent plan: [plan.md](./plan.md)
- Dependencies: [Phase 3](./phase-03-vf2-uart-root-stream-boot.md), [Phase 4](./phase-04-stm32-tpm-protected-authority.md), [Phase 5](./phase-05-nonce-bound-signed-time-service.md)
- Contract: [PERSIST-001..008, BIND-001..009, AC-001..009](../260825-1726-kms-silo-production-root/spec.md)
- Frozen/private seams: [scout report](./scout-report.md)
- Candidate authority paths: `authority/vf2-root-stream/`, `authority/stm32h573i-dk/`, and `libs/authority-protocol/`

## Overview

Replace the runtime protected-state `PermissionDenied` stub and relay-provider placeholder with one opaque STM32H573 transport/provider while leaving public KMS opcodes and payload encodings 9–14 byte-for-byte unchanged. KMS becomes an adapter for authority-owned boot, state, signed-time floors, profile receipts, provider CAS receipts, and typed TLS CertificateVerify; it never becomes a second authority database.

## Key Insights

- `RelayLifecycle` may cache a validated COMMITTED view, but only the STM32/TPM record can authorize recovery or service.
- Public opcode 13's existing `{generation, policy_epoch, profile_digest}` is sufficient only to match and consume a previously authority-validated, single-use staged receipt; it cannot stage caller trust.
- Provider promotion is not commit. Service begins only after `PREPARED → authenticated provider CAS receipt → COMMITTED`, followed by exact provider/record comparison.

## Requirements

- Consume the Phase 2 `authority_protocol` crate and its bounded typed messages; do not define a second wire codec. The concrete runtime transports opaque frames only and exposes no raw UART, signing, digest, TPM, NV, clock, profile assertion, or update primitive to KMS callers.
- On construction, generate a fresh boot challenge, call `open_boot(challenge)`, verify the pinned device/authority/protocol/firmware/policy identity, and accept only the returned non-regressing boot/state epoch plus exact authenticated COMMITTED view. Missing, replayed, torn, wrong-device, PREPARED-only, provider-mismatched, or unavailable state constructs a sealed KMS service.
- Route protected-state reads/writes through the authority client. VFS remains untrusted opaque transport where separately needed and never regains authority freshness, integrity, key, counter, or recovery responsibility.
- Obtain time only through `RequestSignedTime`/`AcceptSignedTime`: STM32 creates the nonce/request tuple, the AP transports opaque CBOR to Phase 5, and STM32 verifies/persists source epoch/sequence/Unix floors before returning a purpose-bound fact. Raw RTC, build time, service-net, and supervisor values remain irrelevant.
- Profile validation occurs in `authority/stm32h573i-dk/`: the authority directly reads the pending TPM slot/SPKI, validates and canonicalizes the bounded chain/profile under authenticated time, then persists one staged receipt binding `{device,authority_epoch,boot_epoch,request,generation,policy_epoch,pending_slot,pending_spki,profile_digest}`.
- Public opcode 13 decodes exactly its existing bytes and calls only `consume_staged_receipt(generation,policy_epoch,profile_digest)`. Reject absent, stale, substituted, already-consumed, wrong-boot, or pending-slot-mismatched receipts; never create pending trust from caller bytes.
- Public opcode 11 prepares the exact intent in authority state, invokes provider compare-and-swap promotion, verifies its authenticated receipt for the same slot/SPKI/generation/profile tuple, and finalizes COMMITTED. On restart, accept only authority recovery of that exact receipt/tuple or seal; never infer from TPM/provider status, VFS, or KMS cache.
- Public opcode 14 returns an SPKI only when authority COMMITTED generation/profile, live provider active generation/SPKI, and lifecycle cache all match. It never reads or exposes a pending slot.
- TLS signing remains the frozen typed request. STM32 checks purpose, fresh authenticated time, request replay floor, active COMMITTED tuple, and reconstructs the exact TLS 1.3 client CertificateVerify input before TPM signing; generic P-256 signing is absent.
- Report `KmsProviderKind::HardwareRelay` plus `RelayProviderAssessment::DevelopmentReference`, never `ProductionQualified`. Keep the existing QEMU development Silo lane separate and unchanged.
- **Operator boundary:** no Build action may purchase hardware, provision OTP, change STM32 lifecycle/debug state, create/disable AWS resources, alter source/key pins, erase TPM/authority state, install a new physical image, inject power cuts, or mutate a live provider slot without an explicit operator checkpoint. Phase 6 consumes Phase 3–5 admitted artifacts; it does not repeat or bypass their irreversible approvals.

## Architecture

`KmsService → ProviderSlot::Stm32 → Stm32AuthorityProvider → AuthorityClient → bounded opaque runtime I/O → STM32 authority → authority-private TPM`.

`ProviderSlot::Stm32` owns the sole session/transport so storage and provider operations cannot race through independent clients. `storage/authority.rs` converts `authority_protocol` facts into internal views only after transcript/session/sequence validation. `RelayLifecycle` mirrors active generation/profile and time/restart floors for dispatch checks, but every mutation is acknowledged by a durable authority transition before the mirror changes. Runtime initialization and every error path default to `Unavailable`/sealed; there is no VFS, Silo, fixture, AP clock, or network fallback.

## Assumptions

- **Claim:** Phase 4 selects one concrete full-duplex runtime link after the VF2 UART boot stream and can expose it exclusively to KMS without an AP peer spoofing the STM32. **Confidence:** medium. **How to verify:** inspect the admitted wiring/pinmux/session-auth evidence and run substitution/replay on the physical link before enabling the feature.
- **Claim:** The Phase 4 firmware returns authenticated provider CAS and recovery receipts with every tuple field required above. **Confidence:** medium. **How to verify:** compare `authority/stm32h573i-dk/` messages against `libs/authority-protocol/src/message.rs`, `libs/authority-protocol/src/state.rs`, and golden vectors.
- **Claim:** The approved VF2 runtime can generate a fresh boot challenge before protected state is opened. **Confidence:** medium. **How to verify:** trace the real entropy call and demonstrate repeated cold boots produce distinct challenges while RNG failure seals.

## Related Code Files

- Create: `cells/services/kms/src/storage/authority.rs`
- Create: `cells/services/kms/src/storage/authority/runtime.rs`
- Create: `cells/services/kms/src/storage/authority/fixture.rs`
- Create: `cells/services/kms/src/storage/provider/stm32.rs`
- Create: `cells/services/kms/src/tests/authority.rs`
- Create: `cells/services/kms/src/tests/authority_faults.rs`
- Create: `tools/dev-reference-authority/kms-integration-probe.py`
- Modify: `cells/services/kms/Cargo.toml`
- Modify: `cells/services/kms/build.rs`
- Modify: `cells/services/kms/src/lib.rs`
- Modify: `cells/services/kms/src/dispatch.rs`
- Modify: `cells/services/kms/src/storage.rs`
- Modify: `cells/services/kms/src/tests.rs`
- Modify: `cells/services/kms/src/storage/provider.rs`
- Modify: `cells/services/kms/src/storage/provider/relay.rs`
- Modify: `cells/services/kms/src/dispatch/enrollment.rs`
- Modify: `cells/services/kms/src/dispatch/relay.rs`
- Modify: `cells/services/kms/src/lifecycle/mod.rs`
- Verify unchanged: `libs/types/src/kms/model.rs`
- Verify unchanged: `libs/types/src/kms/frame.rs`
- Verify unchanged: `libs/types/src/kms/payload/enroll.rs`
- Verify unchanged: `libs/types/src/kms/payload/tls.rs`
- Verify unchanged: `libs/types/src/kms/tests/frame.rs`
- Verify unchanged: `libs/types/src/kms/tests/payload.rs`
- Verify unchanged: `libs/types/src/kms/tests/enrollment.rs`
- Consume without redefinition: `libs/authority-protocol/`, `authority/vf2-root-stream/`, `authority/stm32h573i-dk/`, and Phase 5 vectors/manifest

## Implementation Steps

1. Capture baseline `cargo test -p types` byte fixtures, then add the `development-stm32-authority` KMS feature only where Phase 2 production rejection already recognizes it; fail compilation outside the admitted VF2 DEV_REFERENCE target and never add a promotion switch.
2. Implement fixed-capacity `AuthorityClient` request/response sequencing, transcript authentication, timeout, zeroization, and error mapping over the concrete runtime I/O. Reject extra bytes, wrong command/session/sequence, replay, truncation, and transport reset; allocate no unbounded buffers.
3. Add `ProviderSlot::Stm32(Stm32AuthorityProvider)` as the single runtime owner. Make boot open and state recovery happen before `RootAssessment`, bindings, enrollment, signing, or active-key reads can become ready.
4. Replace the no-test protected-state load/persist stubs with provider-backed authenticated views/transitions. Keep `ProtectedRelayState` only as a cache adapter and delete any path that treats its VFS encoding or process-local floor as authoritative.
5. Change staging so opcode 13 consumes the exact authority receipt. Add negatives for caller-only digest, receipt replay, wrong boot/request/slot/SPKI/generation/policy/profile, changed pending key, and authority restart.
6. Change commit to persist PREPARED, obtain and authenticate the provider CAS receipt, then persist COMMITTED before updating lifecycle. Inject failure before/after every prepare, TPM CAS, receipt return, finalize, response, and KMS restart edge; each recovery must return the exact authenticated tuple or seal.
7. Bind opcode 14 and typed TLS signing to the exact COMMITTED/provider tuple and purpose-bound signed-time fact. Test pending-only, PREPARED-only, split-brain, stale time, repeated request ID, substituted transcript, and provider signature failures.
8. Run `cargo test -p types`, `cargo test -p service-kms authority`, `cargo test -p service-kms enrollment`, `cargo test -p service-kms tls_signing`, and `cargo test -p service-kms storage`; fixture results are regression proof only, never AC hardware evidence.
9. With operator authorization, boot the exact admitted VF2/STM32/SLB9672 hardware and run `python3 tools/dev-reference-authority/kms-integration-probe.py --device <approved-device> --scenario normal --evidence-dir <run-dir>`, then scenarios `boot-replay`, `authority-substitution`, `receipt-replay`, `prepared-power-cut`, `post-cas-power-cut`, `pre-finalize-power-cut`, `provider-split-brain`, `signed-time-expiry`, and `signed-time-outage`.
10. Record raw UART/runtime frames as opaque ciphertext plus decoded authority audit facts, public KMS request/response bytes, TPM/provider generation/SPKI, power/reset timestamps, and reboot outcome under `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-06/<run-id>/`; never claim a fixture/QEMU trace as physical evidence.

## Todo List

- [ ] Land the single-owner authority client/provider without a second codec or fallback.
- [ ] Cut boot/state/time/stage/commit/sign paths over to authority facts.
- [ ] Prove opcode 13 receipt consumption, three-state recovery, and opcode 14 active-only behavior.
- [ ] Preserve all public byte fixtures and production rejection.
- [ ] Execute the authorized physical integration matrix and index raw evidence.

## Success Criteria

- [ ] Normal non-test DEV_REFERENCE runtime leaves the current sealed stubs only after a fresh, pinned, authenticated authority boot and exact COMMITTED/provider match.
- [ ] Opcodes/payloads 9–14 pass unchanged byte fixtures; opcode 13 cannot create trust from its digest, and opcode 14 never returns pending/PREPARED state.
- [ ] Every prepare/CAS/finalize interruption on real hardware recovers one exact authenticated tuple or seals; no split-brain state signs or serves.
- [ ] Signed-time replay, expiry, outage, source rollback, and RTC/build-time mutation cannot authorize enrollment, signing, or active-key service.
- [ ] No public/internal generic signer, digest, TPM, NV, raw time, profile assertion, or DEV-to-production path is introduced.

## Hard Stops

- Stop if any Phase 3/4/5 admission artifact, pin, physical identity, operator authorization, or real hardware/cloud dependency is missing.
- Stop on any public opcode/payload byte change, alternate provider/state/time fallback, PREPARED-only service, caller-created stage trust, unauthenticated promotion result, or `ProductionQualified` classification.
- Stop if the runtime link cannot exclude AP spoofing/replay, or if recovery needs inference from provider/VFS/cache rather than an authenticated authority receipt.
- Stop before implementing `AuthorityClient` until Phase 4 freezes and issues
  the AP-side request-authentication capability. The protocol currently exposes
  only verifier-side `RequestAuthenticator`; it has no session/key
  establishment, request-signing API, rotation/reset contract, or exact binding
  between the 32-byte authenticator, challenge, boot epoch, and AP identity.
  Never substitute a KMS-held generic signer.

## Risk Assessment

The main risks are two clients racing one authority session, partial provider promotion, stale lifecycle cache, receipt substitution, and mistaking unit fault injection for power-loss evidence. Single transport ownership, exact tuple receipts, authority-before-cache ordering, fail-closed recovery, frozen fixtures, and physical edge injection address them; unresolved behavior blocks Phase 7.

## Security Considerations

Keep TPM authorization and private keys entirely behind STM32; zeroize bounded transport buffers and avoid logging secrets or signed-time nonces. Authenticate every private frame before acting, rate-limit/replay-floor requests in authority state, compare all tuple fields in constant-time where secret-independent APIs permit, and seal on timeout or ambiguity. Production checks must reject the feature, firmware, protocol anchor, time anchor, certificate, manifest, and evidence markers while preserving exact `BLOCKED_BY_ADR_0006` output.

## Next Steps

After Phase 4 supplies both the concrete exclusive runtime transport and the
purpose-bounded AP request-authentication capability, this phase may implement
`AuthorityClient`. Only after the remaining criteria and raw evidence exist may
Phase 7 run managed-CA enrollment plus deterministic legacy-signer
compatibility. It may not open a relay TLS session or claim target binding. The
parent Phase 4 remains blocked until Phase 8 validates real AC-001..AC-011
evidence and an independent security review passes; after GO, parent Phase 4
implements ADR-0008 and must pass AC-012 before relay enablement.

## Deviation Log
- Decision: this phase is the single serialized owner for root `Cargo.toml` workspace registration of any Phase 3–5 artifact; parallel phases hand off marker names to the Phase 2 checker instead of editing it.
Why: red-team finding F — shared files were claimed by concurrent phases.
Impact: `plan.md` ownership; no contract change.
Revert: restore per-phase checker/workspace edits (rejected).
- Decision: software track authorized; fixture-simulator integration and unit/fault tests may proceed pre-admission. Step 9 (physical probe matrix) and all AC credit stay hardware-gated; simulator output is regression proof only.
- Evidence: Phase 6 software entry baseline records 54 passing `types` host tests
  and 8 passing production-image checker tests. Frozen KMS source/fixture SHA-256
  values are `07531027...` (`model.rs`), `c2294a6a...` (`frame.rs`),
  `9ab32ef0...` (`payload/enroll.rs`), `a3c0ab28...` (`payload/tls.rs`),
  `7d62eaa9...` (`tests/frame.rs`), `26a2b6ae...` (`tests/payload.rs`), and
  `fd514249...` (`tests/enrollment.rs`). This is a pre-change regression
  baseline only; it grants no hardware or acceptance-criterion credit.
- Evidence: Step 1 now registers `development-stm32-authority` only in KMS,
  requires the paired `vf2-dev-reference` selector plus the RISC-V bare-metal
  target, and makes it mutually exclusive with every existing relay provider.
  The production-image checker freezes and rejects both exact feature names in
  feature lists and artifact content. RV64 no_std paired-feature compilation
  and all 8 checker tests pass; missing-selector, host, and multi-provider
  negative compilations fail with their exact gate diagnostics.
- Blocker: `authority-protocol` requires a 32-byte authenticator on every
  request and exports verifier-only `RequestAuthenticator`; neither it nor the
  STM32 adapter defines AP session/key establishment or purpose-bounded request
  signing. KMS also lacks an exclusive concrete STM32 transport owner. Building
  `AuthorityClient` over a test seam now would invent both security boundaries,
  so Step 2 remains blocked on the Phase 4 contracts.
