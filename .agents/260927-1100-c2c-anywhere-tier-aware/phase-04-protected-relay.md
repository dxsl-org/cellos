---
phase: 4
title: "Protected relay entry and typed handoff"
status: blocked
priority: P1
effort: "external entry/exit gates"
dependencies: [1]
tier: thinking
---

# Phase 04: Protected relay entry and typed handoff

## Overview

Consume, do not re-own, the KMS/Silo Phase-04 protected relay client. This phase gates broker-facing authority integration and prevents a fixture or raw relay client from becoming the production route. Contract and pure negative tests may progress while protected prerequisites remain absent; no TLS dial or remote ingress is enabled.

## Requirements and architecture

- ADR-0008: Protected Relay Authority alone owns TLS 1.3 server chain/hostname/authenticated time, Finished, client CertificateVerify, traffic secrets and record seal/open; `service-net` carries bounded chunks to one fixed endpoint, and net-broker supplies only typed opaque Noise records (`docs/decisions/0008-protected-relay-tls-endpoint-ownership.md:103-145`). No generic TLS, public transcript-hash signer or raw-key fallback.
- ADR-0009: authority builds `FT_SEND_PACKET_CORRELATED (0x0d)` and parses typed request-scoped `FT_PACKET_ERROR (0x0a)`; `0x08` is retired and `0x7f` is protocol-fatal. Correlation is `(authenticated TLS session generation, monotonic nonzero u64)`, not an app identity. Preserve exact `NotSubmitted`/`Submitted` ownership boundary (`docs/decisions/0009-correlate-relay-packet-failures.md:70-125`).
- Owner and **hard gate**: [KMS/Silo protected relay Phase 04](../260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md) must evidence real protected persistence, authenticated time and distinct reviewed pending-key binding (AC-001..AC-011), plus DEV_REFERENCE Phase-8 GO, **before authority Build**. Only its post-entry Build and AC-012 hostile-path acceptance open the relay for Phase 05/06. This is not a production root GO; ADR-0006 production remains separately blocked.
- Existing bounded server-only correlated relay codec and TLS admission tests are inputs, **not a client implementation** (`.agents/260819-1409-cell-to-cell-anywhere-core/phase-05-relay-first-remote-correctness-oracle.md:217-263`). Do not create a second broker-owned codec or embed private TLS in service-net.

## Related files

- External owner (do not duplicate): `libs/authority-protocol/src/{message,wire,state}.rs`, protected authority TLS engine, `cells/services/net/src/{relay_wire,relay_handler}.rs`, KMS/Silo Phase-04 plan and spec.
- Integration after external GO: `cells/services/net-broker/src/{connection_manager,relay_transport,local_runtime}.rs`, `libs/ostd/src/clients/relay_mtls.rs`, `tools/relay-server/` existing codec/tests.

## Implementation steps

1. Define broker↔authority typed contract for session generation, correlation allocation/retirement, opaque Noise record and typed receive/error events; review byte caps, ownership on rejection, certificate-derived NodeId and no plaintext logging. Record the external AC-001..AC-012 checklist with source-bound evidence IDs; while NO-GO, keep implementation receive/send disabled.
2. After the KMS/Silo owner actually passes its entry gate, consume its versioned closed private protocol and authenticated fixed-target carrier; reject stale generation, malformed chunks, wrong hostname/time/profile, legacy `0x08`, duplicate live NodeId, and saturation before mutating any live route.
3. Verify two outstanding relay sends produce independent `0x0a` failures and late/stale-generation events never resolve a newer request. Reconnect backoff resets only after authenticated session establishment; full pool returns `Busy` without evicting in-flight work.
4. Obtain KMS/Silo Phase-04 AC-012 hostile-path evidence. Until it passes, a broker binary with compiled relay modules still refuses dial/receive/remote exports; hand Phase 05 only a typed authenticated session handle.

## Success criteria

- [ ] External owner records entry GO on AC-001..AC-011, DEV_REFERENCE decision, post-entry Build and AC-012; these are cited, not reasserted by this plan.
- [ ] Authenticated server/client identity, typed `0x0d/0x0a` correlation, no-`0x08` and definite-versus-uncertain errors pass negative tests with two concurrent requests and stale connections.
- [ ] No ordinary app/net-broker code owns a relay TLS secret or constructs outer relay frames; missing gate leaves `RemoteEndpoint` disabled.

## Assumptions

- **Claim:** A qualified protected provider and authenticated time will become available. **Confidence:** low. **Verify:** KMS/Silo Phase-04 approved evidence and AC-012, not an elapsed date or software-only harness. If absent, this phase remains blocked; local phases still progress.

## Security considerations

Do not confuse relay certificate-derived route NodeId with E2E Noise remote caller identity; both checks are required in different trust layers. Authority cannot attest that a compromised broker supplied real Noise ciphertext (`ADR-0008:116-122`); peer broker still authenticates/decrypts end to end.

## Risk assessment and rollback

A mistaken gate could expose relay credentials or enable unauthenticated traffic. Rollback closes the exact protected TLS session and broker remote admission, retains local IPC, and never re-enables raw/legacy framing. Network submissions already accepted may execute and must complete as authenticated reply or `Indeterminate`; TLS/private-protocol ABI values already shipped cannot be reused without separate review.

## Deviation log

None.
