---
phase: 7
title: "Relay Enrollment and mTLS Integration"
status: pending
priority: P1
effort: "not estimated"
dependencies: [6]
tier: thinking
---

# Phase 7: Relay Enrollment and mTLS Integration

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Stop on irreversible, public-ABI, trust-boundary, or fallback changes.

## Context Links
- [Parent plan](./plan.md) · [approved entry contract](../260825-1726-kms-silo-production-root/spec.md) · [scout report](./scout-report.md)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md) · [mTLS ADR](../../docs/decisions/0005-mutual-tls-relay-identity.md)
- `libs/authority-protocol/src/{wire,message,state}.rs` · `libs/types/src/kms/{model,csr}.rs` · `libs/types/src/kms/payload/{enroll,tls}.rs`
- `cells/services/kms/src/dispatch/{enrollment,relay}.rs` · `cells/services/net/src/tls/`

## Overview
Execute one real managed-CA enrollment, authority profile validation/staging, and KMS opcode-13/11 receipt consumption as a standalone probe under `tools/dev-reference-authority/`, including one probe-hosted TLS 1.3 client-auth exchange against the named relay endpoint. This phase touches only probe assets plus read-only verification of frozen libraries; all service-net, net-broker, ostd, and `embedded-tls` changes belong to parent Phase 4 after Phase 8 GO. Frozen KMS opcodes/payloads 9–14 stay unchanged; no generic TLS identity, signing, profile, or time surface is created.

## Key Insights
Opcode 14 exposes active state only and cannot validate a pending certificate. The authority must read the TPM pending SPKI itself, validate the managed-CA leaf and complete chain, persist a single-use receipt, and let opcode 13 only consume the matching receipt. Purpose-bound typed time, CertificateVerify signing, and relay-session lifetime binding are narrow authority-mediated operations proven here by the probe; production service-net integration repeats them only in parent Phase 4 after GO.

## Requirements
- Phase 6 must be complete on the admitted physical authority and live signed-time deployment; all normal-runtime authority calls fail closed on reset, mismatch, expiry, or outage.
- Enrollment uses a named, operator-approved managed CA and real issuance endpoint. The CSR originates from the pending TPM key; no fixture certificate, filesystem key, bare SPKI assertion, or self-signed substitute is acceptable.
- `ValidateAndStageRelayProfile` validates leaf-first order, at most 3 DER certificates, each at most 4096 bytes and total at most 12 KiB, pinned trust, CA constraints, clientAuth-only EKU, fixed relay SAN/NodeId policy, authenticated-time validity, floors/digests, and the exact authority-read pending SPKI.
- Opcode 13 consumes exactly one matching `StagedProfileReceipt`; opcode 11 commits only the same tuple. Replayed receipt, changed pending slot, stale generation/policy, substituted leaf, truncated/misordered/oversized chain, or wrong validity/EKU/SAN/CA seals.
- The probe's TLS client validates certificates only against a purpose-bound authority time fact and signs CertificateVerify only via the typed opcode-8 request, converting canonical low-S `r||s` to DER locally; no generic signer, clock, or client-identity assertion exists in this phase.
- Scope restriction: every change lives in `tools/dev-reference-authority/` probe assets (plus `tools/relay-enroll/` probe callers). Modifying `cells/services/net*`, `cells/services/net-broker`, `libs/ostd`, or `third_party/embedded-tls` in this phase is prohibited — those belong to parent Phase 4 after Phase 8 GO.
- Every relay connection the probe opens is bound to its accepted time fact with a hard teardown deadline ≤ that fact's `expires_at`; the binding governs the established data plane, not just the handshake.
- Renewal requires a fresh purpose-bound reauthorization time fact before the deadline; the original fact cannot be reused or extended to keep a session alive.
- The probe tears down the connection on expiry, reboot/reset, or failed renewal even mid-traffic; any application byte flowing after the deadline is a NO-GO.
- Operator checkpoint: authorize each managed-CA issuance and live relay connection; do not create/deploy cloud resources or rotate keys outside the admitted account/region and recorded authorization.

### AC Traceability
| AC | Phase 7 observable evidence |
|---|---|
| AC-001/002 | Cold runtime opens only with the pinned physical authority and fresh boot challenge; disconnect, substitution, and challenge replay prevent enrollment/signing/handshake. |
| AC-004/005 | Cold boot gets typed signed time before any mTLS; RTC/build-time changes do nothing; expiry, replay, rollback, freeze, and endpoint outage stop the handshake; and a live scenario proves an established data-plane connection stops by the time-fact deadline. |
| AC-006 | Real managed-CA leaf plus required intermediate binds the TPM pending SPKI and succeeds; every chain/SPKI/policy negative is rejected before commit. |
| AC-007 | Stage/consume/commit faults leave the prior exact tuple or sealed state; no pending or split-brain tuple signs. |
| AC-008 | Existing byte fixtures prove opcodes/payloads 9–14 unchanged and opcode 14 active-only. |
| AC-009 | Live negative probes and interface review find no generic signer/profile/time/client-identity operation in the probe surface; production service-net/supervisor/generic-TLS IPC stay untouched by this phase and are re-checked in parent Phase 4 after GO. |
| AC-010 | Every Phase 7 feature, anchor, certificate, manifest, and binary remains marked `DEV_REFERENCE` for Phase 8 rejection tests. |

## Architecture
`probe enrollment → KMS opcodes 9/10 → managed CA → bounded chain → authority ValidateAndStageRelayProfile → KMS opcode 13 receipt consume → opcode 11 commit`, then one probe-hosted exchange: `probe TLS 1.3 client (committed chain + purpose-bound time) → named relay mTLS endpoint → opcode 8 CertificateVerify → opaque Noise records`. All runtime service-net/net-broker wiring is parent Phase 4 work after Phase 8 GO.

## Assumptions
- **Claim:** A host-side probe TLS client can complete TLS 1.3 client authentication with an external signer (typed opcode-8 CertificateVerify), a bounded leaf-first client chain, and purpose-bound time without patching any shipped TLS stack. **Confidence:** medium. **How to verify:** exercise the probe client against the named validating relay and record full handshake traces.
- **Claim:** The managed CA can issue the fixed P-256 relay-client profile and return a leaf-first chain within 3/4096/12288 bounds. **Confidence:** medium. **How to verify:** record the CA profile, issued DER sizes, extensions, and full chain before staging.
- **Claim:** Phase 6 exposes purpose-bound time and TLS signing through the closed authority protocol without a new public KMS opcode. **Confidence:** high. **How to verify:** inspect `libs/authority-protocol` and opcode fixture diffs before Build.

## Related Code Files
| Action | Exact likely files |
|---|---|
| Modify | `tools/dev-reference-authority/kms-integration-probe.py` plus new probe modules for managed-CA submission, the TLS 1.3 client-auth exchange, and session-deadline enforcement |
| Modify | `tools/relay-enroll/{relay_enroll.py,relay_enroll_test.py}` as probe-side callers only; `mtls-mount-manifest.template.toml` stays descriptive documentation |
| Consume/verify | `libs/authority-protocol/src/{wire,message,state}.rs`; `libs/ostd/src/clients/kms/relay.rs`; `cells/services/kms/src/dispatch/{enrollment,relay}.rs`; `cells/services/kms/src/storage/authority.rs`; `cells/services/kms/src/storage/authority/{runtime,fixture}.rs`; `cells/services/kms/src/storage/provider/stm32.rs` |
| Verify unchanged | `libs/types/src/kms/{model,csr}.rs`; `libs/types/src/kms/payload/{enroll,tls}.rs`; `libs/types/src/kms/tests/{frame,payload,enrollment}.rs`; `cells/services/net*/**`; `cells/services/net-broker/**`; `libs/ostd/**`; `third_party/embedded-tls/**` |
| Evidence | `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-07/<run-id>/` |

## Implementation Steps
1. Freeze the real CA profile, endpoint, chain bounds, relay endpoint, authorization record, and expected pending generation; refuse issuance if any bound is already exceeded.
2. Make enrollment retrieve the opcode-9/10 CSR, submit that exact CSR to the managed CA, capture the complete DER chain, and send it only to `ValidateAndStageRelayProfile`; never accept a caller-computed trust digest.
3. Exercise Phase 6's opcode-13 receipt consumption and opcode-11 commit end to end from the probe; prove one-shot, tuple equality, slot re-read, abort, restart, and fault behavior without reimplementing the adapter or changing bytes 9–14.
4. Implement the probe TLS 1.3 client with the committed chain, pinned server trust/hostname/TLS 1.3, purpose-bound time validation, and opcode-8 CertificateVerify signing; propagate signer/time/transport failures as handshake errors, never panic or synthesize output.
5. Bind each probe relay connection to its accepted time fact with a teardown deadline ≤ `expires_at`; require fresh purpose-bound reauthorization for renewal and tear down unconditionally on expiry, boot, reset, or failed renewal even mid-traffic.
6. Run one real cold-boot enrollment/commit/mTLS exchange and all Phase 7 AC negatives, including the live established-data-plane-stops-by-deadline scenario; store hashed raw CA, authority, network, and relay-server observations under `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-07/<run-id>/` for Phase 8.

## Todo List
- [ ] Complete real managed-CA pending-SPKI enrollment and atomic activation via the standalone probe.
- [ ] Prove purpose-bound time and typed CertificateVerify inside the probe TLS exchange.
- [ ] Prove session binding: teardown by time-fact deadline, fresh purpose-bound reauthorization for renewal, mid-traffic expiry teardown.
- [ ] Preserve frozen ABI; leave service-net/net-broker/ostd/embedded-tls untouched pending parent Phase 4 post-GO.

## Stop Conditions
Stop and mark NO-GO if Phase 6 evidence is incomplete; the real CA/profile/relay endpoint is unnamed; the chain exceeds bounds; the probe TLS client cannot fail safely or send the complete chain; any AP caller can assert SPKI/time/profile or invoke generic signing; any established session survives past its accepted time-fact deadline, renews without fresh purpose-bound reauthorization, or continues after a failed renewal; any edit outside `tools/dev-reference-authority/` touches service-net, net-broker, ostd, or `embedded-tls` before parent Phase 4 GO; or any operator checkpoint is missing.

## Success Criteria
- [ ] On actual admitted hardware, a managed-CA leaf requiring an intermediate binds the authority-read pending TPM SPKI, commits once, and completes TLS 1.3 client authentication at the named relay.
- [ ] All substitution, ordering, size, policy, replay, expiry, outage, transaction-edge, and unauthorized-caller cases fail closed with no previous/pending identity misuse.
- [ ] Live AC-005 scenario: an established data-plane connection stops by its accepted time-fact deadline; renewal happens only on fresh purpose-bound reauthorization; expiry, reset, or failed renewal tears down mid-traffic.
- [ ] Cold boot succeeds only with fresh typed authority time and CertificateVerify; RTC/build-time changes cannot enable it.
- [ ] Byte fixtures for opcodes/payloads 9–14 are identical and opcode 14 remains active-only; no service-net, net-broker, ostd, or `embedded-tls` file changed, so generic interfaces still expose no identity, signer, profile, or time assertion.

## Risk Assessment
The main risks are CA profile drift, a probe TLS client that treats signer/clock failure as optional, and session state surviving its time-fact deadline. Any such result is a stop, not a fallback trigger; production-integration risk transfers to parent Phase 4 after GO.

## Security Considerations
The authority, not service-net, establishes pending-key trust. Certificate chains are public but integrity-critical; private keys and generic TPM/sign operations never cross the authority boundary. Noise remains the end-to-end application-security layer inside relay mTLS, and the time-fact teardown deadline is a security boundary, not a hint.

## Next Steps
Hand the hashed real-system traces to Phase 8. Do not change the parent Phase 4 `blocked` state from Phase 7, even if every Phase 7 scenario passes.

## Deviation Log
None at planning time beyond: **2026-08-26 Decision** — security red-team review returned NO-GO on PLAN-TIME-004 and the simplicity review returned NO-GO on phase scope; both resolved without weakening any stop. PLAN-TIME-004 resolution: relay connections bind to their accepted time fact with a hard teardown deadline ≤ `expires_at`, renewal requires fresh purpose-bound reauthorization, teardown on expiry/boot/reset/failed renewal applies even mid-traffic, and a live AC-005 established-data-plane-stop-by-deadline scenario was added; production enforcement lands in parent Phase 4 after Phase 8 GO. Simplicity resolution: this phase is restricted to the standalone managed-CA/authority/KMS probe under `tools/dev-reference-authority/`; all service-net, net-broker, ostd, and `embedded-tls` modifications moved to parent Phase 4 post-GO.
