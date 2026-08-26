---
phase: 3
title: "Certificate Activation and Provisioning"
status: completed
priority: P1
effort: "not estimated"
dependencies: [1]
tier: thinking
---

# Phase 3: Certificate Activation and Provisioning

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links
- `docs/decisions/0005-mutual-tls-relay-identity.md`
- `tools/relay-enroll/mtls-mount-manifest.template.toml`
- `reports/security-judge.json` finding KMS-ARCH-004

## Overview
Implement the public certificate/key activation contract between managed-CA tooling, mounted service-net configuration, and the protected KMS relay key without turning KMS into a certificate store.

## Key Insights
A certificate chain exceeds the fixed KMS response and contains no secret. KMS
should expose fixed relay public metadata; service-net owns mounted chain/trust
validation. A non-exportable key still needs proof of possession: KMS must create
and sign a constrained relay CSR rather than expecting a CA to trust bare SPKI.
Key activation and certificate replacement must be atomic from the consumer’s
perspective.

## Requirements
- Keep public chain/trust read-only in service-net; no private-key path or chain
  crosses a KMS frame.
- Expose bounded relay metadata and append-only opcodes 9–14 with exact
  supervisor/service-net authorization and live identity checks.
- KMS and provider reconstruct one canonical RFC 2986 CRI. The provider returns
  raw `r||s`; KMS normalizes and self-verifies it, then assembles bounded DER.
- Bind one-shot ordered CSR handles to supervisor identity, pending generation,
  policy/request/begin/restart epochs; poison them on transfer or bad order.
- Maintain `active` plus one `pending` state through
  prepare/CSR/stage/commit/abort and cleanup tombstones; never expose a partial
  activation.
- Keep the development enrollment key inside Silo; derive it with fresh
  entropy nonce plus generation, and promote/destroy it through purpose-bound
  commands only.
- Strictly validate bounded manifests, paths, TLS 1.3, certificate DER,
  clientAuth-only EKU, NodeId, SPKI, chain order, and duplicates.
- Authenticate lifecycle journal state and restart/time floors. If protected
  persistence or authenticated time is unavailable, seal relay mTLS.
- Frozen opcode 14 exposes only the active public key. It cannot bind a pending
  certificate before commit; precommit activation therefore stays unavailable.

## Architecture
Provider creates the key and reconstructs exact CSR/TLS messages. KMS
self-verifies raw CSR proof, owns canonical RFC 2986 ASN.1/DER assembly, and
exposes a bounded reader. Service-net activates an immutable profile matching
protected state; KMS stores no certificate chain.

## Related Code Files
| Area | Action | Test impact |
|---|---|---|
| `libs/types/src/kms`; `libs/ostd/src/clients/kms*` | Modified | enrollment ABI/client |
| `cells/services/kms/src/{dispatch,lifecycle,storage}` | Modified | state/recovery/CSR |
| `cells/{services,guests}/silo*`; `libs/types/src/silo*` | Modified | nonce key custody |
| `cells/services/net/src/tls*` | Modified | profile/certificate/time validation |
| `tools/{relay-enroll,relay-server}`; manifest template | Modified | strict provisioning |

## Implementation Steps
1. Froze CSR, chain, hostname, chunk, profile, generation, and manifest bounds.
2. Added purpose-bound enrollment/staging/public-key operations with exact
   principal and live-generation authorization.
3. Implemented canonical CRI/CSR construction, provider proof,
   self-verification, and bounded ordered reads.
4. Implemented active/pending lifecycle transitions, cleanup tombstones,
   authenticated journal state, restart recovery, and fail-closed runtime seams.
5. Extended development Silo with nonce-bound create/sign/destroy/promote
   commands and no generic signing surface.
6. Added strict profile, manifest, DER, EKU, NodeId, SPKI, and chain validation.
7. Kept raw/default/rolled-back time and unbound pending-key activation
   unavailable; production selection and qualification remain deferred.

## Todo List
- [x] Freeze manifest/CSR/profile bounds and canonical metadata.
- [x] Implement constrained CSR, certificate binding, and atomic lifecycle.
- [x] Prove cleanup, recovery, precedence, and fail-closed unavailable paths.

## Test Scenario Matrix
| Priority | Scenario | Expected |
|---|---|---|
| Critical | filesystem private key field or mismatched SPKI/extension | reject profile |
| Critical | arbitrary CSR body/profile or runtime caller enrolls | deny before provider |
| Critical | stale/replayed CSR handle or mixed chunks | reject and invalidate |
| Critical | CSR handle transferred to another supervisor generation | reject |
| Critical | active current plus pending next during renewal | current serves until atomic commit |
| High | power loss at every prepare/stage/commit point | one complete generation or unavailable |
| Critical | stale/revoked key generation after restart | unavailable |
| High | torn manifest/cert update | old valid profile or fail closed, never mixed |
| High | active/next CA overlap | only explicit overlap accepted |
| Critical | old profile/CA/denylist/time replay below protected floor | reject |
| Critical | missing/default/rolled-back authenticated time | mTLS unavailable |
| Medium | oversized/duplicate/malformed DER | bounded parse failure |

## Success Criteria
- [x] KMS frames never carry certificate chains or private material.
- [x] OpenSSL independently parses and verifies the KMS-assembled constrained
  CSR self-signature against its non-exportable public key.
- [x] CSR signing is purpose-bound and unavailable to service-net/broker.
- [x] CSR handles belong only to the exact live supervisor generation and
  invalidate on transfer, wrong order, exhaustion, or restart mismatch.
- [x] Lifecycle tests preserve current active state while preparing a pending
  generation and permit commit only from a complete staged state.
- [x] Service-net accepts only a complete active chain matching canonical KMS
  SPKI/NodeId metadata and strict profile/manifest policy.
- [x] Cleanup, recovery, stale-generation, and rollback tests never expose a
  mixed or orphaned usable generation.
- [x] Missing protected persistence, authenticated time, provisioning, or
  pending-key binding keeps relay mTLS unavailable without fallback.

## Verification Evidence
- Focused host suites passed 140/140: types 41, KMS 58, Silo 17, and net 24.
  The exact out-of-order CSR test and the full KMS suite passed.
- KMS checks were clean on RV64 and AArch64. The exact current-tree AArch64
  development-Silo check also passed clean with `LLVM_OBJCOPY`.
- Relay-enroll passed 10/10; relay-manifest passed 11/11. OpenSSL reported
  `Certificate request self-signature verify OK` for the assembled CSR.
- The production posture checker passed 2/2. Direct invocation with unqualified
  inputs exited 1 fail-closed; builder/checker paths produced no image and remain
  `BLOCKED_PENDING_PHASE_6_7_8`.
- Final code and security re-reviews returned GO with no residual findings.
  Exact commands and results are in `reports/harness/{verification,
  execution-evidence,review-decision}.json`.

## Risk Assessment
External issuance and one-shot CSR state are load-bearing; wrong-certificate
binding remains an identity failure even when signing is correct.

## Security Considerations
Reject CN-only hostname fallback, ambiguous DER, duplicate extensions, unknown key purpose, stale generations, and permissive defaults. Never log certificate private material or signer requests.

## Next Steps
Phase 4 may consume only an immutable active profile, and remains deferred until
protected persistence, authenticated time, and authenticated pending-key
binding exist. Phases 6–8 still gate product selection, hardware
implementation, physical qualification, and production readiness.
