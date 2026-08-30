---
phase: 1
title: "KMS TLS Signing Vertical Slice"
status: completed
priority: P1
effort: "not estimated"
dependencies: []
tier: thinking
---

# Phase 1: KMS TLS Signing Vertical Slice

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links
- `docs/decisions/0005-mutual-tls-relay-identity.md`
- `reports/security-judge.json` findings KMS-ARCH-001, 005, 006, 009
- `reports/simplicity-judge.json`

## Overview
Deliver one fixture-backed, independently testable path from an attested service-net caller through KMS policy to a purpose-specific P-256 TLS signer. No Silo or real hardware is required yet.

## Key Insights
The existing KMS fixed-frame ABI, caller trailer, broker binding, and `RootProvider` seam are reusable. Existing broker/X25519 identity must remain separate. Authorizing a generic 32-byte prehash would create a durable cross-protocol signing oracle.

## Requirements
- Append KMS v1 operations for service-net binding, relay signer metadata, and
  TLS 1.3 client CertificateVerify only.
- Preserve `BrokerBinding`, `AcquireNodeIdentity`, and `NoiseStaticDh` contracts.
- Bind service-net cell ID, generation, and live service TID; stale/restarted
  instances deny.
- Keep one provider boundary with independent typed capability leaves:
  `C2cX25519Status` and `RelayP256Status` each carry algorithm, generation,
  policy epoch, provider identity, assessment, and readiness. Neither health,
  rotation, error, or readiness can satisfy the other.
- Both KMS and the protected provider reconstruct and hash the exact
  `64*0x20 || "TLS 1.3, client CertificateVerify\0" || transcript_hash`
  from typed fields. The provider never accepts a caller-computed digest.
- Provider/KMS signatures are 64-byte big-endian `r || s`. KMS validates scalar
  ranges, normalizes low-S, and self-verifies. For TLS only, DER encoding occurs
  in service-net; Phase 3 KMS owns CSR ASN.1/signature encoding.
- Freeze one TLS request:
  `{transcript_hash[32], relay_generation, active_profile_digest, request_id}`.
  Qualification and authenticated time are protected provider state, never
  caller-supplied booleans/proofs.
- Security correction: this fixture-backed request proves typed signing
  mechanics only. Because untrusted service-net supplies the opaque transcript
  hash, it does not bind the protected signer to the configured relay server and
  must not be wired into a relay client. Phase 4 owns an unapproved replacement
  architecture while public KMS opcodes remain frozen.
- `RelayP256Status` includes protected profile/time floors and a per-device
  monotonic qualification epoch/record digest, plus
  `DevelopmentReference | QualificationTest | ProductionQualified`.
  Before every production sign, the provider matches its immutable device
  identity and current silicon/revision, lifecycle, board, RoT firmware,
  protocol version, AP measurement, KMS/provider/image, policy, key generation,
  and profile digest to its independently signed record. Record substitution,
  replay, or mismatch reverts protected state to `QualificationTest`.
- Create one top-level production image owner:
  `scripts/build-production-relay-image.sh` plus
  `scripts/check-production-relay-image.py`. The production tuple requires the
  hardware relay provider and verified TLS and excludes Silo, fixtures,
  test-hooks, dev keys/RNG, `tls-insecure`, raw relay, and K1 fallback.

## Architecture
`service-net typed request → KmsService authorization/profile match → RelayP256
provider leaf reconstructs exact TLS input → KMS low-S normalization/signature
self-check → fixed response`. Existing C2C requests reach only `C2cX25519`.
A relay-only provider may report RelayP256 ready while C2cX25519 is unavailable.
Production requests additionally require the protected profile digest and
`ProductionQualified` latch; qualification tests are endpoint/profile-bound.

## Assumptions
- **Claim:** Raw `r||s` fits the existing response and embedded-tls adapter can encode it as required. **Confidence:** high. **How to verify:** compile a provider conformance test against embedded-tls 0.19.
- **Claim:** The live NET service TID can be obtained through the existing service registry. **Confidence:** high. **How to verify:** inspect `api::syscall::service` and init registration before editing.

## Related Code Files
| File | Action | Test impact |
|---|---|---|
| `libs/types/src/kms/model.rs` and `libs/types/src/kms/payload/` | Modify | wire vectors |
| `libs/ostd/src/clients/kms.rs` | Modify | client round trips |
| `cells/services/kms/src/{auth,dispatch,main}.rs` | Modify | authorization/operations |
| `cells/services/kms/src/storage/{provider,root}.rs` | Modify | independent leaf invariants |
| `cells/services/kms/src/tests/` | Modify | negative/cross-readiness matrix |
| `scripts/build-production-relay-image.sh` | Create | named image build |
| `scripts/check-production-relay-image.py` | Create | artifact/config scan |
| `cells/services/{kms,net}/Cargo.toml`, `kernel/Cargo.toml` | Modify | feature exclusion |

## Implementation Steps
1. Add canonical fixed-size request/response payloads and typed errors; reject
   unknown flags, roles, schemes, lengths, generations, and key purposes.
2. Add independent `C2cX25519Status` and `RelayP256Status` types; do not
   reinterpret the existing node identity or share readiness/rotation state.
3. Freeze `r||s`, scalar validation, low-S normalization, self-verification,
   exact TLS request schema, and error behavior in shared vectors.
4. Extend the provider seam with
   `sign_tls13_client_certificate_verify(transcript_hash, relay_generation,
   active_profile_digest, request_id)`. Provider reconstructs the exact TLS input,
   checks its protected qualification/time state, and preserves C2C unchanged.
5. Add protected profile/time floors and qualification-record/tuple digests.
   Production requires an exact current tuple match, never readiness or host
   assertions.
6. Add service-net binding registration and live generation validation without
   changing broker binding.
7. Implement dispatcher/client methods and a fixture provider; test every
   combination of C2C/relay readiness, rotation, errors, and enablement.
8. Add the named production image build/check scripts and make both/neither
   provider selection, insecure TLS, and development features hard errors.
9. Compile/typecheck changed crates; run focused KMS/type tests and the
   production artifact checker.

## Completion Evidence

- 2026-08-25 focused verification passed 59 of 59 tests: 40 KMS tests and
  19 type/wire tests. KMS produced zero warnings; OSTD compiled with the same
  seven pre-existing baseline warnings
  (`reports/harness/verification.json`,
  `reports/harness/execution-evidence.json`).
- Live service-net generation authorization, restart/stale-generation denial,
  request replay handling, exact TLS message construction, qualification/profile
  denial, malformed-provider rejection, low-S normalization, and signature
  self-verification passed the focused KMS scenarios
  (`reports/harness/execution-evidence.json`).
- All 10 unsafe Cargo feature combinations and all 18 unsafe production artifact
  checker probes were rejected. Clean checker and builder candidates
  intentionally exit 3 with `BLOCKED_PENDING_PHASE_6_7_8`; the hardware relay
  provider remains compile-blocked pending Phases 6–7. Phase 1 therefore proves
  fail-closed production exclusion but does not claim hardware-backed production
  signing (`reports/harness/verification.json`,
  `reports/harness/execution-evidence.json`).
- Adversarial validation and the standard/security review gate passed with zero
  residual Critical/High findings
  (`reports/harness/adversarial-validation.json`,
  `reports/harness/review-decision.json`).
- 2026-08-29 security review supersedes the earlier relay-authorization verdict:
  the fixture path cannot prove the exact relay server identity inside the
  protected boundary. Production exclusion still holds, so no reachable relay
  client is exposed; Phase 4 remains blocked on a target-bound design.

## Todo List
- [x] Freeze canonical TLS signing frames and errors.
- [x] Add separate relay P-256 identity types.
- [x] Enforce live service-net binding.
- [x] Implement fixture-backed vertical slice.
- [x] Prove production provider exclusion.

## Test Scenario Matrix
| Priority | Scenario | Expected |
|---|---|---|
| Critical | arbitrary cell, broker, missing trailer, stale generation | deny before provider |
| Critical | generic digest/key ID/cross-role request | unrepresentable or reject |
| High | provider unavailable/wrong epoch/bad signature | fail closed |
| High | valid transcript hash and active generation | self-verified signature |
| Critical | C2C ready/relay unavailable or inverse | only matching leaf authorizes |
| Critical | provider ready but production latch absent | production sign denied |
| Critical | typed fields do not match protected profile digest | deny |
| High | high-S provider signature | normalized, self-verified low-S output |
| Medium | malformed/replayed request ID | canonical typed error |

## Success Criteria
- [x] Existing broker authorization and wire contracts remain unchanged, and
  focused regressions are green.
- [x] Only the live bound service-net generation can invoke the TLS signer.
- [x] No public API accepts a generic prehash or private-key material.
- [x] `scripts/build-production-relay-image.sh` and
  `scripts/check-production-relay-image.py` reject development and downgrade
  paths and keep production builds blocked until a hardware provider is selected,
  implemented, and qualified in Phases 6–8.

## Risk Assessment
The fixed ABI is public and easy to overload. Keep operations append-only and key purposes distinct. CallerIdentity is local provenance, not hardware boot attestation.

## Security Considerations
Authorization must precede provider access. Signature failures, malformed provider output, or self-check failure return an error and never panic or retry with another provider.

## Next Steps
Phase 2 is next and requires explicit approval before work begins. It may contain
the development Silo lane; Phase 3 can later bind mounted certificates to the
fixed relay metadata.

## Deviation Log
None.
