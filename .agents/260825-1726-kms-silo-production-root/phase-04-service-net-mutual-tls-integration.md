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
- `docs/decisions/0008-protected-relay-tls-endpoint-ownership.md`
- [Approved Phase 4 entry-gate contract](./spec.md)
- [Concrete DEV_REFERENCE lane research](../reports/research-260826-1605-phase4-dev-reference-lane.md)
- [DEV_REFERENCE execution plan](../260826-1605-phase4-dev-reference-authority/plan.md)

## Overview
Add a protected-authority relay TLS 1.3 client endpoint and reconnect
net-broker's relay path without exposing device identity, TLS secrets, or server
authorization to generic `TlsStream` or untrusted service-net. ADR-0008 fixes the
ownership boundary; Build remains blocked on the three original entry gates.

## Key Insights
`embedded-tls` 0.19 already exposes server certificate/transcript verification
seams, but the protected authority must own the complete TLS state machine,
including key schedule, Finished verification, client CertificateVerify, and
record seal/open. The current public KMS transcript-hash request remains
fixture-only and deny-only for production. The 1,200-byte private authority
frames require bounded ordered chunking for chains and TLS records.

The selected research candidate remains VisionFive 2 v1.3B UART-root-stream boot
with an STM32H573I-DK, an authority-private OPTIGA TPM SLB 9672, and a
project-operated AWS signed-time service. This is an unimplemented candidate,
not entry-gate evidence; all three gates remain NO-GO.

## Requirements
- Before implementation, evidence build-entry AC-001 through AC-011 in the
  approved `spec.md`: real protected persistence, authenticated time, and
  root-validated pending-key binding under frozen public KMS opcodes 9–14.
- Contract approval does not satisfy any gate. Product selection,
  hardware-provider plumbing, and Phase 7 evidence are not Phase 4 entry gates.
- Version the closed private authority protocol for bounded relay TLS sessions,
  ordered record chunks, typed opaque application records, cancellation,
  close, and reset. No generic TLS or arbitrary signing operation may exist.
- Keep public KMS opcodes 9–14 and their wire encodings byte-for-byte unchanged.
  The old transcript-hash signing opcode remains fixture-compatible but denies
  in production and is never called by the relay client.
- The protected authority owns client random/ECDHE, server CA/hostname/time
  validation, Server CertificateVerify and Finished, client chain and
  CertificateVerify, traffic secrets, and record seal/open.
- Support a bounded leaf-first client chain: at most three DER certificates,
  each at most 4096 bytes and at most 12 KiB total. Constrain signatures to
  P-256/SHA-256 and normalize canonical low-S `r||s` internally before DER.
- `service-net` opens only the configured relay endpoint for the live attested
  broker generation and transports bounded TLS bytes. It cannot select or
  observe hostname, CA, profile, key, scheme, signature, or traffic secret.
- Net-broker's production API supplies and receives bounded typed Noise-record
  buffers. The authority treats their contents as opaque: this prevents honest
  path plaintext routing but cannot prove provenance against a compromised
  application processor, which already controls the supplied bytes.
- Missing/rolled-back authenticated time, stale broker or TLS generation,
  endpoint/profile mismatch, malformed or reordered chunks, replay, alert, EOF,
  timeout, cancellation, or authority reset destroys the exact TLS session.
- Preserve direct Noise first; relay has no raw, K1, shared-secret, or
  `tls-insecure` identity fallback.
- Bound authority sessions and memory plus handshake, I/O, chain, record, frame,
  chunk, retry, cancellation, and error behavior.

## Architecture
`net-broker Noise ciphertext → service-net fixed-endpoint byte carrier →
Protected Relay Authority TLS client endpoint → authenticated external relay`.
The authority is the sole owner of relay server validation, TLS key schedule,
client authentication, and record crypto. Broker never gets TLS key material;
service-net makes no TLS security decision; generic consumers cannot spend the
relay identity.

## Assumptions
- **Claim:** `embedded-tls` can be adapted into the protected target with fixed
  memory and fallible internal signing without importing generic service-net TLS
  APIs. **Confidence:** medium. **How to verify:** prototype chain/record bounds,
  signer failure, and code-size/memory ceilings before product wiring.
- **Claim:** The 1,200-byte authority frame can carry ordered TLS chunks without
  transcript ambiguity or cross-session confusion. **Confidence:** medium.
  **How to verify:** model session generation, chunk offset/final markers,
  replay/reorder/truncation, cancellation, and reset before accepting a record.
- **Claim:** Existing service-net ownership tables can bind the fixed relay
  socket carrier to live broker generation. **Confidence:** high. **How to
  verify:** reuse attested socket ownership patterns from `4c8acb2c`.

## Related Code Files
| File | Action | Test impact |
|---|---|---|
| `docs/decisions/0008-protected-relay-tls-endpoint-ownership.md` | New decision | architecture gate |
| `libs/authority-protocol/src/{message,wire,state}.rs` | Version/extend | byte fixtures, state rejection |
| protected authority TLS adapter/engine | Create | handshake, chain, record, reset |
| `cells/services/kms/src/dispatch/relay.rs` | Modify | production legacy-sign denial |
| `cells/services/net/src/{relay_wire,relay_handler}.rs` | Create | carrier ownership/chunk tests |
| `libs/ostd/src/clients/relay_mtls.rs` | Create | privileged bounded carrier IPC |
| `cells/services/net-broker/src/connection_manager.rs` | Modify | direct-first routing |
| `cells/services/net-broker/src/relay_transport.rs` | Create | Noise ciphertext framing |

## Implementation Steps
1. Specify the private authority-protocol revision: fixed session generation,
   ordered chunk offsets/final markers, endpoint/profile binding, request
   authentication, cancellation, close, and deterministic errors.
2. Preserve public KMS opcode 9–14 byte fixtures and make the legacy
   transcript-hash signer deny for production providers.
3. Build the bounded protected TLS client engine with internal authenticated
   time, fixed relay trust/hostname, server CertificateVerify/Finished checks,
   bounded client chain, fallible P-256 signing, and record seal/open.
4. Prove hostile chain, record, chunk, replay, reorder, truncation, timeout,
   alert, EOF, cancellation, profile rotation, and authority-reset behavior.
5. Add service-net's fixed-endpoint carrier. Bind socket and authority session
   generation to the live attested net-broker; expose no generic TLS flags.
6. Add privileged OSTD carrier IPC and net-broker relay transport whose honest
   production path exchanges only bounded Noise ciphertext.
7. Restore fallback only to this protected mTLS path after direct Noise fails;
   preserve request ID, deadline, retry class, dedup state, and no-evict rules.
8. Exercise attacker server, invalid CA/SAN/time/profile, missing/intermediate
   chain, signer/provider denial, stale generation, disconnect/reconnect, and
   the isolated relay oracle before any remote-complete claim.

## Todo List
- [ ] Evidence the protected-persistence contract in `spec.md`.
- [ ] Evidence the authenticated-time contract in `spec.md`.
- [ ] Evidence the pending-key binding contract in `spec.md`.
- [ ] Make external signer failure error-safe.
- [x] Approve ADR-0008 protected relay TLS endpoint ownership; reject opaque
  service-net-only transcript assertions.
- [ ] Implement the ADR-0008 protected TLS endpoint and fixed-endpoint carrier; satisfy AC-012 before route enablement.
- [ ] Bind sockets to broker generation and restore direct-first authenticated relay routing.

## Test Scenario Matrix
| Priority | Scenario | Expected |
|---|---|---|
| Critical | generic TLS caller requests client identity | deny/unrepresentable |
| Critical | KMS denial/unavailable signer | handshake error, no panic/fallback |
| Critical | raw/K1/tls-insecure route | absent from production |
| High | invalid server CA/SAN/time/client profile | fail before registration |
| Critical | service-net requests standalone production signature | unrepresentable or production deny |
| High | valid leaf plus intermediate chain | server receives ordered full chain |
| Critical | missing/default/rolled-back trusted time | fail before handshake |
| High | chain >3 entries or >12 KiB | reject before handshake |
| Critical | chunk replay/reorder/truncation or stale TLS generation | exact protected session destroyed |
| High | valid direct path | relay not contacted |
| High | direct exhausted plus valid profile | mTLS relay carries Noise bytes |

## Success Criteria
- [ ] Build starts only after build-entry AC-001 through AC-011 are evidenced.
- [ ] AC-012 hostile-path evidence passes before relay enablement or Phase 4 completion.
- [ ] Product selection remains absent from the Phase 4 entry and completion gates.
- [ ] Generic `TlsStream` remains server-auth only.
- [ ] Only the privileged broker path can establish relay mTLS.
- [ ] Every signer and TLS failure is bounded and fail closed.
- [ ] Protected authorization proves the exact configured relay server identity
  and handshake without trusting service-net assertions.
- [ ] A managed-CA leaf requiring an intermediate completes client
  authentication; missing/misordered/oversized chains fail closed.
- [ ] No private key or generic sign handle crosses service boundaries; the
  honest production broker path sends only Noise ciphertext, while the
  authority makes no cryptographic claim about caller-supplied byte provenance.

## Risk Assessment
A broad mTLS extension to generic TLS or a standalone production signer would
expose the device identity. Keep one fixed protected relay endpoint with closed,
typed, bounded authority operations and no caller-selected TLS inputs.

## Security Considerations
The protected authority validates the exact relay server and owns
CertificateVerify plus TLS record keys. service-net is an untrusted byte
carrier. The typed production broker path supplies only Noise-record buffers,
but the authority treats them as opaque and cannot distinguish malicious
plaintext from ciphertext after application-processor compromise. Generic
signing, caller-selected TLS inputs, private-key export, and downgrade paths
remain absent.

## Next Steps
Do not begin Phase 4 Build. ADR-0008 resolves the signer ownership decision but
does not satisfy protected persistence, authenticated time, or pending-key
binding. Continue the selected DEV_REFERENCE candidate through
[its execution plan](../260826-1605-phase4-dev-reference-authority/plan.md);
only its Phase 8 GO over AC-001 through AC-011 can open software Build. Build
must then implement ADR-0008 and pass AC-012 before relay enablement or Phase 4
completion. ADR-0006 independently blocks Phases 7–8 pending exact-product
evidence and a superseding GO ADR.

## Deviation Log
2026-08-26 — ADR-0006 clarified that Phase 4 is product-independent, retained the protected-persistence and authenticated-time gates, added a distinct reviewed pending-key binding under frozen opcodes 9–14, and approved no KMS ABI change.
2026-08-26 — Approved `spec.md` selects a root-owned Protected Relay Authority while preserving public KMS opcodes 9–14. Approval fixes the three entry-gate contracts but leaves Phase 4 blocked until AC-001 through AC-011 are evidenced.
2026-08-26 — Deep research selected the VF2 UART-root-stream plus STM32H573/SLB9672/AWS signed-time composition as the only concrete DEV_REFERENCE candidate worth implementing. The research remains NO-GO evidence: no hardware, firmware, endpoint, fault matrix, or AC-001 through AC-011 result exists, so Phase 4 stays blocked.
2026-08-26 — Created the DEV_REFERENCE execution plan with red-team corrections applied; its Phase 8 GO is now the sole opener for Phase 4 Build, so Next Steps link to it directly.
2026-08-29 — Security review found that the frozen opaque transcript-hash signer request cannot bind the configured relay server identity inside the protected boundary because service-net performs CA/hostname validation while remaining untrusted. Phase 4 Build now also requires a separately approved target-bound signer architecture; no KMS ABI or implementation change is approved here.
2026-08-29 — The user approved ADR-0008 Option A: the Protected Relay Authority owns the complete relay TLS endpoint, service-net becomes a bounded untrusted byte carrier, the legacy public transcript-hash signer denies in production, and public KMS opcodes 9–14 remain byte-compatible. This resolves the architecture decision only; the three entry gates remain NO-GO.
