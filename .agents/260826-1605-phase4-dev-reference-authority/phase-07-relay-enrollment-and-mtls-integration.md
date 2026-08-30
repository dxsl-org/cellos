---
phase: 7
title: "Relay Enrollment and Legacy-Signer Compatibility"
status: pending
priority: P1
effort: "not estimated"
dependencies: [6]
tier: thinking
---

# Phase 7: Relay Enrollment and Legacy-Signer Compatibility

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Stop on irreversible, public-ABI, trust-boundary, or fallback changes.

## Context Links
- [Parent plan](./plan.md) · [approved entry contract](../260825-1726-kms-silo-production-root/spec.md) · [scout report](./scout-report.md)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md) · [mTLS ADR](../../docs/decisions/0005-mutual-tls-relay-identity.md) · [TLS ownership ADR](../../docs/decisions/0008-protected-relay-tls-endpoint-ownership.md)
- `libs/authority-protocol/src/{wire,message,state}.rs` · `libs/types/src/kms/{model,csr}.rs` · `libs/types/src/kms/payload/{enroll,tls}.rs`
- `cells/services/kms/src/dispatch/{enrollment,relay}.rs`

## Overview
Execute one real managed-CA enrollment, authority profile validation/staging, and KMS opcode-13/11 receipt consumption as a standalone probe under `tools/dev-reference-authority/`. The legacy opcode-8 signer is exercised only against a deterministic TLS CertificateVerify fixture to prove frozen DEV_REFERENCE compatibility. It does not connect to a relay, authenticate a server, prove target binding, satisfy AC-012, or model the ADR-0008 relay client. All protected TLS endpoint, service-net, net-broker, OSTD, and `embedded-tls` work belongs to parent Phase 4 after Phase 8 GO.

## Key Insights
Opcode 14 exposes active state only and cannot validate a pending certificate. The authority must read the TPM pending SPKI, validate the managed-CA leaf and complete chain, persist a single-use receipt, and let opcode 13 consume only the matching receipt. Purpose-bound time and the legacy CertificateVerify fixture prove the selected DEV_REFERENCE provider and frozen ABI mechanics; they provide no relay-server authorization evidence. Production providers deny the standalone signer.

## Requirements
- Phase 6 must be complete on the admitted physical authority and live signed-time deployment; normal-runtime authority calls fail closed on reset, mismatch, expiry, or outage.
- Enrollment uses a named operator-approved managed CA and real issuance endpoint. The CSR originates from the pending TPM key; no fixture certificate, filesystem key, bare SPKI assertion, or self-signed substitute is acceptable.
- `ValidateAndStageRelayProfile` validates leaf-first order, at most 3 DER certificates, each at most 4096 bytes and total at most 12 KiB, pinned trust, CA constraints, clientAuth-only EKU, fixed relay SAN/NodeId policy, authenticated-time validity, floors/digests, and the authority-read pending SPKI.
- Opcode 13 consumes exactly one matching `StagedProfileReceipt`; opcode 11 commits only the same tuple. Receipt replay, changed pending slot, stale generation/policy, substituted leaf, truncated/misordered/oversized chain, or wrong validity/EKU/SAN/CA seals.
- The opcode-8 probe uses only a fixed canonical TLS 1.3 CertificateVerify test vector. It does not accept a network transcript, establish a TLS session, or count toward target-binding or AC-012 evidence.
- Purpose-bound signed-time probes prove issue, replay, freeze, rollback, expiry, reset, and outage behavior independently of TLS.
- Every change lives in `tools/dev-reference-authority/` probe assets plus `tools/relay-enroll/` probe callers. This phase cannot modify service-net, net-broker, OSTD, or `embedded-tls`.
- Public KMS opcodes/payloads 9–14 remain byte-identical. Production denial of the legacy signer remains mandatory.
- Operator checkpoint: authorize managed-CA issuance; do not create cloud resources or rotate keys outside the admitted account/region and recorded authorization.

### AC Traceability
| AC | Phase 7 observable evidence |
|---|---|
| AC-001/002 | Cold runtime opens only with the pinned physical authority and fresh boot challenge; disconnect, substitution, and challenge replay prevent enrollment and protected operations. |
| AC-004/005 | Typed signed time is independent of RTC/build time; expiry, replay, rollback, freeze, and endpoint outage seal protected operations. |
| AC-006 | Real managed-CA leaf plus required intermediate binds the TPM pending SPKI; every chain/SPKI/policy negative rejects before commit. |
| AC-007 | Stage/consume/commit faults leave the prior exact tuple or sealed state; no pending or split-brain tuple signs. |
| AC-008 | Byte fixtures prove opcodes/payloads 9–14 unchanged and opcode 14 active-only. |
| AC-009 | Interface review finds no generic signer/profile/time/client-identity surface. Opcode 8 is typed, fixture-only here, and production-denied. |
| AC-010 | Every Phase 7 feature, anchor, certificate, manifest, and binary remains `DEV_REFERENCE` for Phase 8 rejection tests. |

## Architecture
`probe enrollment → KMS opcodes 9/10 → managed CA → bounded chain → authority ValidateAndStageRelayProfile → KMS opcode 13 receipt consume → opcode 11 commit`. A separate deterministic test-vector call checks legacy opcode-8 wire/provider compatibility. There is no relay socket or probe-hosted TLS client. Parent Phase 4 implements the ADR-0008 protected endpoint and AC-012 after entry GO.

## Related Code Files
| Action | Exact likely files |
|---|---|
| Modify | `tools/dev-reference-authority/kms-integration-probe.py` plus probe modules for managed-CA submission, signed-time faults, and deterministic legacy-signer compatibility |
| Modify | `tools/relay-enroll/{relay_enroll.py,relay_enroll_test.py}` as probe-side callers only |
| Consume/verify | `libs/authority-protocol/src/{wire,message,state}.rs`; `libs/ostd/src/clients/kms/relay.rs`; `cells/services/kms/src/dispatch/{enrollment,relay}.rs`; `cells/services/kms/src/storage/authority.rs`; `cells/services/kms/src/storage/provider/stm32.rs` |
| Verify unchanged | `libs/types/src/kms/{model,csr}.rs`; `libs/types/src/kms/payload/{enroll,tls}.rs`; `cells/services/net*/**`; `cells/services/net-broker/**`; `libs/ostd/**`; `third_party/embedded-tls/**` |
| Evidence | `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-07/<run-id>/` |

## Implementation Steps
1. Freeze the real CA profile, issuance endpoint, chain bounds, authorization record, and expected pending generation; refuse issuance when any bound is exceeded.
2. Retrieve the opcode-9/10 CSR, submit that exact CSR to the managed CA, capture the complete DER chain, and send it only to `ValidateAndStageRelayProfile`; never accept a caller-computed trust digest.
3. Exercise opcode-13 receipt consumption and opcode-11 commit end to end; prove one-shot tuple equality, slot re-read, abort, restart, and fault behavior without changing bytes 9–14.
4. Exercise purpose-bound signed time and the opcode-8 DEV_REFERENCE signer against fixed test vectors only. Prove production-provider denial and label all output compatibility evidence, not relay security evidence.
5. Run all Phase 7 negatives and store hashed raw CA/authority observations under the Phase 7 evidence directory for Phase 8.

## Todo List
- [ ] Complete real managed-CA pending-SPKI enrollment and atomic activation through the standalone probe.
- [ ] Prove purpose-bound time faults independently of TLS.
- [ ] Prove opcode-8 deterministic fixture compatibility and production denial without a network transcript.
- [ ] Preserve frozen ABI and leave all parent Phase 4 TLS/client/carrier files untouched.

## Stop Conditions
Stop and mark NO-GO if Phase 6 evidence is incomplete; the real CA/profile is unnamed; the chain exceeds bounds; any caller can assert SPKI/time/profile or invoke generic signing; opcode 8 accepts arbitrary schemes or production use; any evidence labels the legacy probe as relay authentication, target binding, or AC-012; any parent Phase 4 TLS/client/carrier file changes before GO; or an operator checkpoint is missing.

## Success Criteria
- [ ] On admitted hardware, a managed-CA leaf requiring an intermediate binds the authority-read pending TPM SPKI and commits once.
- [ ] All substitution, ordering, size, policy, replay, expiry, outage, and transaction-edge cases fail closed with no previous/pending identity misuse.
- [ ] RTC/build-time changes cannot authorize signed-time operations.
- [ ] Byte fixtures for opcodes/payloads 9–14 are identical, opcode 14 remains active-only, and the legacy signer is explicitly fixture-only and production-denied.
- [ ] Evidence makes no relay handshake, target-binding, data-plane, or AC-012 claim.

## Risk Assessment
The primary risk is mistaking a successful legacy signature or valid client certificate for proof that the protected authority authenticated a relay server. Such evidence is invalid by construction and forces NO-GO.

## Security Considerations
The authority, not the probe, establishes pending-key trust. Certificate chains are public but integrity-critical; private keys and generic TPM/sign operations never cross the authority boundary. ADR-0008 deliberately defers complete TLS server authentication, transcript ownership, and record protection to parent Phase 4 after GO.

## Next Steps
Hand the hashed enrollment, signed-time, compatibility, and negative traces to Phase 8. Do not change parent Phase 4 from `blocked`; Phase 7 cannot satisfy AC-012.

## Deviation Log
- 2026-08-26 — Purpose-bound time received explicit replay/freeze/rollback/outage evidence requirements.
- 2026-08-29 — ADR-0008 invalidated the planned probe-hosted relay TLS exchange as target-binding evidence. The phase now limits opcode 8 to deterministic DEV_REFERENCE compatibility, removes all relay-session claims, and leaves protected TLS endpoint implementation plus AC-012 to parent Phase 4 after entry GO.
