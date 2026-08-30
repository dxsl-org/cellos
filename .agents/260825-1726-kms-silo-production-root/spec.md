---
title: "Phase 4 Entry-Gate Contract"
status: approved
approved: 2026-08-26
decision: "Frozen KMS ABI plus root-owned Protected Relay Authority"
---

# Phase 4 Entry-Gate Contract

## Context

Phase 4 may begin only after runtime has real protected persistence, real authenticated time, and a reviewed pending-key binding under existing KMS opcodes 9–14. VFS, supervisor, service-net, network transport, and the application processor are outside the trust boundary. ADR-0006 still blocks Phases 7–8; this contract selects no production root.

Approval fixes the contract. It does not prove any entry gate. Phase 4 remains blocked until a concrete authority and signed-time source satisfy build-entry AC-001 through AC-011.

ADR-0008 amends the relay TLS ownership boundary without satisfying an entry
gate: the authority owns the complete fixed relay TLS endpoint, service-net is
only a bounded byte carrier, and the legacy public transcript-hash signer denies
in production.

## Functional Requirements

### Protected persistence

- **PERSIST-001 — Authority identity.** WHEN runtime opens protected relay state, THE authority SHALL prove a stable, non-exportable identity pinned by reviewed image or policy.
- **PERSIST-002 — Boot opening.** WHEN KMS calls `open_boot(challenge)`, THE authority SHALL return an authenticated fact binding at least `{device_id, authority_id, boot_epoch, state_epoch, challenge, approved_boot_measurement}` and SHALL atomically advance the relevant floor before state use.
- **PERSIST-003 — Durable record.** THE authority SHALL maintain a rollback-resistant, power-loss-atomic record binding `{schema, device, lane, authority_epoch, boot/restart/request floors, time source epoch/sequence/unix floor, approved boot measurement, firmware/policy floors, trust/verifier/denylist/qualification digests, active/pending key slot/SPKI/profile, canonical chain/profile, transaction intent, provider receipt}`.
- **PERSIST-004 — Untrusted transport.** WHERE VFS or network transport is used, THE system SHALL transport only ciphertext or opaque bytes; freshness, integrity, and key protection SHALL be established by the authority.
- **PERSIST-005 — Rollback failure.** IF state is missing, torn, replayed, restored from an old snapshot, reset under another authority identity, or regresses any floor, THEN enrollment, signing, and relay handshake SHALL seal.
- **PERSIST-006 — Transaction protocol.** WHEN profile activation occurs, THE system SHALL transition `PREPARED intent → provider CAS promotion receipt → COMMITTED authority record`; ONLY one COMMITTED tuple SHALL be served.
- **PERSIST-007 — Crash recovery.** IF a crash or power loss occurs at any prepare, promote, or finalize edge, THEN recovery SHALL restore one exact authenticated tuple or seal; it SHALL NOT infer active state from the provider or VFS alone.
- **PERSIST-008 — Provider split-brain.** IF provider active generation or SPKI differs from the authority COMMITTED record, THEN signing and handshake SHALL seal until the exact tuple is recovered through an authenticated receipt.

### Authenticated time

- **TIME-001 — Source.** BEFORE relay mTLS exists, THE authority SHALL obtain signed time from a pinned external signed-time authority over a pre-mTLS transport.
- **TIME-002 — Fresh challenge.** WHEN requesting time, THE authority SHALL generate a fresh 256-bit nonce and accept only a response binding `{device_id, authority_id, boot_epoch, request_id, purpose, nonce, source_epoch, source_sequence, unix_seconds, expires_at}`.
- **TIME-003 — Monotonic floor.** WHEN a valid time fact is received, THE authority SHALL atomically persist strict source epoch/sequence and Unix floors before issuing a fact or lease to KMS.
- **TIME-004 — Purpose binding.** THE authority SHALL issue time authorization only for a specific purpose; generic current-time assertions from the application processor, service-net, or supervisor SHALL NOT be trusted.
- **TIME-005 — Freshness.** IF a time fact or lease is expired, replayed, frozen, forked, nonce-mismatched, purpose-mismatched, source-sequence-regressed, or Unix-time-regressed, THEN enrollment, signing, and relay handshake SHALL seal.
- **TIME-006 — No cached availability.** IF the signed-time authority is offline or unreachable, THEN normal runtime SHALL seal; a cached lease SHALL NOT continue service after expiry or a boot transition.
- **TIME-007 — RTC exclusion.** WHEN raw RTC, build time, or application-processor clock changes, THEN authorization outcome SHALL remain unchanged unless the authenticated source fact changes.
- **TIME-008 — Remote rollback defense.** IF the remote time database returns old state under a response signed for a fresh nonce, THEN stored strict source epoch/sequence and Unix floors SHALL detect it and seal.

### Pending-key binding under frozen opcodes 9–14

- **BIND-001 — ABI freeze.** THE public KMS opcodes 9–14, payloads, and wire encodings SHALL remain byte-for-byte unchanged; opcode 14 SHALL expose active profile only.
- **BIND-002 — Direct SPKI proof.** WHEN staging a pending profile, THE authority SHALL directly authenticate the provider pending slot and SPKI; a caller-provided digest SHALL NOT create trust.
- **BIND-003 — Certificate validation.** THE authority SHALL validate that the leaf binds the exact pending SPKI and SHALL validate bounded ordered chain, pinned trust, leaf-first order, CA constraints, relay-client EKU, fixed relay SAN or identity policy, authenticated-time validity, firmware/policy/qualification floors, and denylist/verifier digests.
- **BIND-004 — Canonical profile.** WHEN validation succeeds, THE authority SHALL canonicalize and durably persist the exact profile and chain, then return a single-use staged receipt binding `{device, authority_epoch, boot_epoch, request, generation, policy_epoch, pending_slot, pending_spki, profile_digest}`.
- **BIND-005 — Opcode 13 semantics.** WHEN KMS opcode 13 receives `{generation, policy_epoch, profile_digest}`, KMS SHALL only match and consume an already root-validated staged receipt; it SHALL NOT trust the caller digest or create pending trust itself.
- **BIND-006 — Stale or substituted input.** IF the leaf SPKI is substituted, the chain is truncated, misordered, or oversized, EKU/SAN/CA/validity is wrong, generation or policy is stale, the receipt is replayed or consumed, or the pending slot changes after validation, THEN stage or commit SHALL fail closed.
- **BIND-007 — Commit binding.** WHEN commit begins, THE authority SHALL prepare the exact intent, verify the provider CAS-promotion authenticated receipt for the same tuple, and then finalize the COMMITTED record; the caller SHALL NOT select key material or active identity.
- **BIND-008 — TLS endpoint ownership.** WHEN relay TLS is attempted, THE authority SHALL own server chain/hostname/time validation, transcript and Finished verification, the active client chain, CertificateVerify, traffic secrets, and record seal/open for the exact COMMITTED relay tuple. The public transcript-hash signing request SHALL remain byte-compatible but deny in production.
- **BIND-009 — Caller limits.** service-net and supervisor MAY transport bounded opaque TLS bytes and authority requests/responses but SHALL NOT assert trusted TLS server identity, transcript, Finished, boot, time, profile, pending identity, qualification, or commit result.
- **BIND-010 — Typed outer framing and opaque Noise.** WHEN relay application data crosses the protected boundary, THE production broker API SHALL accept only bounded typed `{session_generation, correlation, destination_node_id, Noise_record}` sends and return typed source/Noise or packet-error events. THE authority SHALL construct and parse ADR-0009 outer relay frames and SHALL treat `Noise_record` contents as opaque. This prevents accidental plaintext routing but cannot prove ciphertext provenance against a compromised application processor; that threat is outside this authority's confidentiality guarantee.

### Lane and packaging constraints

- **LANE-001 — DEV_REFERENCE root.** A usable DEV_REFERENCE authority SHALL be a separate appliance or equivalent root-controlled lane outside the compromise domain of service-net, supervisor, and the application processor, with a stable non-exportable identity and durable rollback-tested NVRAM.
- **LANE-002 — Rejected substitutes.** A same-application-processor process or VM, QEMU RAM counter, filesystem-held key, fixture-only identity, and raw RTC SHALL NOT satisfy an entry gate.
- **LANE-003 — Boot authorization.** IF root-independent authorization of the application-processor boot measurement cannot be demonstrated, THEN normal runtime SHALL remain sealed. The current Raspberry Pi 3 header-SPI path does not independently demonstrate this authorization.
- **LANE-004 — Production separation.** Build, packaging, and runtime classification SHALL make it impossible to classify a DEV_REFERENCE authority, anchor, certificate, or feature as `ProductionQualified`; Phases 7–8 remain `BLOCKED_BY_ADR_0006`.
- **LANE-005 — Missing authority.** IF no qualifying appliance or equivalent is named and evidenced, THEN all three gates remain unsatisfied and Phase 4 Build SHALL NOT begin.

## Edge Cases and Threat Cases

Required analysis and verification covers replayed, torn, or missing state; old-snapshot restore; authority reset; application-processor, service-net, or supervisor compromise; time-token replay, freeze, fork, or rollback; a freshly signed response backed by a rolled-back remote database; provider/authority split-brain; crash at every prepare/promote/finalize edge; pending-certificate substitution; chain truncation, ordering, and size; invalid EKU, SAN, CA status, or validity; stale caller generation; generic signing attempts; authority partition or outage; and production packaging of the DEV lane.

## Build Entry Acceptance Criteria

- **AC-001:** Normal non-test runtime bypasses the current persistence `PermissionDenied` and zero-time stubs only while the real authority is challenge-verified and available.
- **AC-002:** Authority-identity pinning and a fresh boot challenge fail closed on substitution or replay.
- **AC-003:** Restart, injected power loss, and old-snapshot restore prove all protected floors; every regression seals.
- **AC-004:** RTC and build-time manipulation has no authorization effect.
- **AC-005:** Cold boot obtains signed time through the pre-mTLS path; replay, freeze, rollback, and offline cases seal.
- **AC-006:** A managed-CA leaf is accepted only when it binds the provider pending SPKI and a complete bounded policy-valid chain; all substitution negatives fail.
- **AC-007:** Fault injection at every transaction edge recovers the exact tuple or seals; no split-brain state serves traffic.
- **AC-008:** Opcodes 9–14 pass byte-level compatibility fixtures; opcode 14 never exposes pending state.
- **AC-009:** Generic signer, profile, and time assertions remain absent from service-net and supervisor interfaces.
- **AC-010:** Production build and packaging checks reject every DEV authority, provider, anchor, certificate, and feature and retain the ADR-0006 block.
- **AC-011:** Independent security review confirms the authority trust boundary, transaction recovery, and fail-closed paths before entry gates are marked passed.

## Phase 4 Completion and Route-Enable Criterion

- **AC-012:** After Build implements ADR-0008, attacker server, wrong CA/hostname, modified transcript, invalid Finished, stale TLS generation, and chunk replay/reorder/truncation fail inside the authority before client authentication or application release; service-net cannot request a standalone production signature. AC-012 cannot open Build and must pass before relay enablement or Phase 4 completion.

## Out of Scope

- Production root or vendor selection.
- Phase 7 hardware-provider implementation.
- Phase 8 production qualification.
- Any change to public KMS opcodes 9–14.
- Generic TLS client authentication.
- Availability fallback while the authority or signed-time source is unavailable.

## Constraints

- No KMS ABI expansion.
- No trusted assertion sourced from the application processor, supervisor, service-net, VFS, or network transport.
- No service from PREPARED-only state.
- No cached-time availability exception.
- No DEV-to-production promotion path.

## Open Prerequisite

The selected DEV_REFERENCE authority and signed-time candidate has not been implemented or evidenced. Therefore this approved contract leaves Phase 4 `blocked`; only evidence satisfying build-entry AC-001 through AC-011 may open Build. AC-012 is the mandatory post-Build completion and route-enable gate.

## Change Log

- 2026-08-26 — Approved Option A: integrated protected persistence, authenticated time, and pending-key binding while preserving KMS opcodes 9–14.
- 2026-08-29 — ADR-0008 amended relay TLS ownership: the protected authority owns the fixed endpoint and service-net carries bounded bytes; public KMS opcodes remain unchanged.