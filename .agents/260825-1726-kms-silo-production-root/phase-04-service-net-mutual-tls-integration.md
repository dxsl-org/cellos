---
phase: 4
title: "Service-Net Mutual TLS Integration"
status: blocked
priority: P1
effort: "not estimated"
dependencies: [1, 3]
tier: thinking
---

# Phase 4: Service-Net Mutual TLS Integration

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links
- `cells/services/net/src/tls/`
- `cells/services/net-broker/src/connection_manager.rs`
- `reports/security-judge.json` findings KMS-ARCH-001, 005, 006
- `docs/decisions/0006-block-production-root-pending-exact-product-evidence.md`
- [Approved Phase 4 entry-gate contract](./spec.md)
- [Concrete DEV_REFERENCE lane research](../reports/research-260826-1605-phase4-dev-reference-lane.md)
- [DEV_REFERENCE execution plan](../260826-1605-phase4-dev-reference-authority/plan.md)

## Overview
Add a privileged service-net relay mTLS profile backed by KMS and reconnect net-broker’s relay path without exposing the device identity through generic `TlsStream`. This software-only phase is independent of production-root product selection, but implementation remains blocked on its three entry gates.

## Key Insights
`embedded-tls` 0.19 supports client certificates and an external `SignerMut`, but its client CertificateVerify path currently calls infallible `sign()` and unwraps output. The generic TLS IPC is server-auth only and should remain so.
The selected research candidate is VisionFive 2 v1.3B UART-root-stream boot
with an STM32H573I-DK, an authority-private OPTIGA TPM SLB 9672, and a
project-operated AWS signed-time service. This is an unimplemented candidate,
not entry-gate evidence; all three gates remain NO-GO.

## Requirements
- Before implementation, evidence all requirements and acceptance criteria in
  the approved `spec.md`: real protected persistence, authenticated time, and
  root-validated pending-key binding under frozen KMS opcodes 9–14.
- Contract approval does not satisfy any gate. Product selection,
  hardware-provider plumbing, and Phase 7 evidence are not Phase 4 entry gates.
  No KMS ABI change is approved.
- Vendor/pin the minimal embedded-tls patch under `third_party/embedded-tls/`.
  Remote signer errors propagate as `TlsError`/alert; no panic or dummy signature.
- Extend its client-auth provider with a bounded ordered chain: at most three
  DER certificates, each at most 4096 bytes, total encoded certificate message
  at most 12 KiB. Send leaf first and every configured intermediate.
- Add a separate `RelayMtlsProvider`; retain normal server-auth
  `ViTlsProvider` unchanged.
- Only the live attested net-broker generation can request privileged relay connect.
- The current proposed request
  `{transcript_hash[32], relay_generation, active_profile_digest, request_id}` is
  insufficient because service-net is outside the trust boundary and the
  protected authority cannot derive the relay server identity from an opaque
  hash. Do not implement this signer path.
- Before Build, approve a design that binds CertificateVerify authorization to
  protected verification of the exact relay server chain, hostname/endpoint,
  handshake, live broker generation, and active client tuple without trusting
  service-net assertions. Public KMS opcodes 9–14 remain frozen; any necessary
  internal privileged protocol requires separate review.
- Convert KMS's canonical low-S `r||s` to DER only in the TLS adapter.
- Validate relay CA, validity, hostname/SAN, TLS 1.3, and active client profile
  inside the approved protected target-binding design, not by trusting
  service-net's result.
- Replace the build-time clock clamp with authenticated time checked against the
  protected floor. Missing or rolled-back time fails the handshake.
- Preserve direct Noise first; relay carries opaque Noise records and has no
  raw/K1/tls-insecure fallback.
- Bound handshake, I/O, frames, sessions, retries, cancellation, and errors.

## Architecture
Target boundary, with mechanism still unapproved:
`net-broker privileged connect → service-net transport → protected relay-server
identity/transcript validation + active-profile CertificateVerify → external
relay`. Broker never gets TLS key material, service-net cannot choose the
authenticated server, and generic consumers cannot spend the relay identity.

## Assumptions
- **Claim:** A maintained dependency patch can be pinned reproducibly without forking unrelated TLS code. **Confidence:** medium. **How to verify:** implement the smallest upstream-compatible `try_sign` change and run embedded-tls client-auth tests.
- **Claim:** Existing service-net ownership tables can bind privileged relay sockets to broker generation. **Confidence:** high. **How to verify:** reuse attested socket ownership patterns from `4c8acb2c`.

## Related Code Files
| File | Action | Test impact |
|---|---|---|
| `third_party/embedded-tls/` and root `Cargo.toml` patch | Create/modify | signer/chain tests |
| `cells/services/net/src/tls/{provider,socket}.rs` | Modify | mTLS handshake |
| `cells/services/net/src/tls/clock.rs` | Modify | missing/rollback time tests |
| `cells/services/net/src/{relay_mtls,relay_wire,relay_handler}.rs` | Create | auth/ownership tests |
| `cells/services/net/src/relay_profile.rs` | Modify | active chain bounds |
| `libs/ostd/src/clients/relay_mtls.rs` | Create | bounded I/O tests |
| `cells/services/net-broker/src/connection_manager.rs` | Modify | direct-first routing |
| `cells/services/net-broker/src/relay_transport.rs` | Create | Noise framing |

## Implementation Steps
1. Vendor the pinned dependency and patch CertificateVerify to call fallible
   signing without unwrap; add upstream-style failure regression tests.
2. Add a bounded multi-entry client-certificate API and prove leaf-first
   encoding, three-entry/12-KiB limits, intermediate delivery, and overflow
   rejection against a validating mTLS server.
3. Implement `RelayMtlsProvider` with active chain and KMS signer adapter;
   constrain the scheme to P-256/SHA-256 and DER-encode only canonical low-S
   `r||s`.
4. Add a dedicated relay-connect IPC operation with fixed configured
   endpoint/profile; reject caller-supplied certificate, key handle, or hostname
   override.
5. Authorize and own relay sockets by live net-broker identity/generation;
   invalidate on restart.
6. Use the distinct reviewed pending-key binding under frozen opcodes 9–14 and
   the frozen Phase 1 request fields; KMS/provider independently enforce protected
   profile, qualification, and time state before signing. Do not change the KMS ABI.
7. Add relay framing compatible with mandatory mTLS and its 8192-byte bound.
8. Restore fallback only to this privileged mTLS path after direct Noise fails.
9. Exercise invalid trust, truncated/misordered/oversized chain, missing
   intermediate/profile/time, denied signer, signer timeout/reset, cancellation,
   EOF, and reconnect behavior.

## Todo List
- [ ] Evidence the protected-persistence contract in `spec.md`.
- [ ] Evidence the authenticated-time contract in `spec.md`.
- [ ] Evidence the pending-key binding contract in `spec.md`.
- [ ] Make external signer failure error-safe.
- [ ] Approve protected relay-server identity binding; reject opaque
  service-net-only transcript assertions.
- [ ] Add isolated relay mTLS provider and privileged relay-connect IPC.
- [ ] Bind sockets to broker generation and restore direct-first authenticated relay routing.

## Test Scenario Matrix
| Priority | Scenario | Expected |
|---|---|---|
| Critical | generic TLS caller requests client identity | deny/unrepresentable |
| Critical | KMS denial/unavailable signer | handshake error, no panic/fallback |
| Critical | raw/K1/tls-insecure route | absent from production |
| High | invalid server CA/SAN/time/client profile | fail before registration |
| Critical | service-net requests signing for attacker TLS server | protected authority denies before client authentication |
| High | valid leaf plus intermediate chain | server receives ordered full chain |
| Critical | missing/default/rolled-back trusted time | fail before handshake |
| High | chain >3 entries or >12 KiB | reject before handshake |
| High | valid direct path | relay not contacted |
| High | direct exhausted plus valid profile | mTLS relay carries Noise bytes |

## Success Criteria
- [ ] All three software entry gates are evidenced before implementation begins.
- [ ] Product selection remains absent from the Phase 4 entry and completion gates.
- [ ] Generic `TlsStream` remains server-auth only.
- [ ] Only the privileged broker path can establish relay mTLS.
- [ ] Every signer and TLS failure is bounded and fail closed.
- [ ] Protected authorization proves the exact configured relay server identity
  and handshake without trusting service-net assertions.
- [ ] A managed-CA leaf requiring an intermediate completes client
  authentication; missing/misordered/oversized chains fail closed.
- [ ] No private key, generic sign handle, or plaintext Cell payload crosses service boundaries.

## Risk Assessment
A broad “mTLS flags” extension to generic TLS would expose the device identity. Keep one fixed relay profile and one privileged operation.

## Security Considerations
KMS creates the exact CertificateVerify signature; service-net must not prehash arbitrary caller data. Noise remains the end-to-end application security boundary.

## Next Steps
Do not begin Phase 4 Build. In addition to the three existing entry gates,
approve a protected relay-server identity-binding architecture; the current
opaque transcript-hash request is insufficient. Continue the selected
DEV_REFERENCE candidate through
[its execution plan](../260826-1605-phase4-dev-reference-authority/plan.md), but
its Phase 8 GO cannot by itself authorize the signer path. ADR-0006 independently
blocks Phases 7–8 pending vendor evidence and a superseding GO ADR.

## Deviation Log
2026-08-26 — ADR-0006 clarified that Phase 4 is product-independent, retained the protected-persistence and authenticated-time gates, added a distinct reviewed pending-key binding under frozen opcodes 9–14, and approved no KMS ABI change.
2026-08-26 — Approved `spec.md` selects a root-owned Protected Relay Authority while preserving public KMS opcodes 9–14. Approval fixes the three entry-gate contracts but leaves Phase 4 blocked until AC-001 through AC-011 are evidenced.
2026-08-26 — Deep research selected the VF2 UART-root-stream plus STM32H573/SLB9672/AWS signed-time composition as the only concrete DEV_REFERENCE candidate worth implementing. The research remains NO-GO evidence: no hardware, firmware, endpoint, fault matrix, or AC-001 through AC-011 result exists, so Phase 4 stays blocked.
2026-08-26 — Created the DEV_REFERENCE execution plan with red-team corrections applied; its Phase 8 GO is now the sole opener for Phase 4 Build, so Next Steps link to it directly.
2026-08-29 — Security review found that the frozen opaque transcript-hash signer request cannot bind the configured relay server identity inside the protected boundary because service-net performs CA/hostname validation while remaining untrusted. Phase 4 Build now also requires a separately approved target-bound signer architecture; no KMS ABI or implementation change is approved here.
