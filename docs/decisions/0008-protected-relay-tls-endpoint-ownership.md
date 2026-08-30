# ADR-0008: Keep the relay TLS client endpoint inside the protected authority

**Date**: 2026-08-29
**Status**: Accepted

## Context

ADR-0005 requires TLS 1.3 mutual authentication for the external relay and a
non-exportable device key. The first signing vertical slice exposed a fixed
public KMS request containing
`{transcript_hash, relay_generation, active_profile_digest, request_id}`. That
slice proves bounded signing mechanics, but it does not prove the relay target.
`service-net` is outside the protected trust boundary, performs server
CA/hostname validation, and supplies the opaque transcript hash. A compromised
`service-net` can therefore request a device CertificateVerify signature for a
TLS server that the protected authority never authenticated.

Pinning the relay hostname, CA, or server SPKI in protected state does not repair
that request: the authority still cannot derive the server identity, handshake,
or Finished verification from a caller-supplied hash. Mirroring only part of the
handshake also leaves two state machines whose transcript, key schedule,
cancellation, and retry transitions can diverge.

The public KMS opcodes 9–14 and their wire encodings are frozen. The existing
`service-net` TLS implementation is server-auth only. The protected authority
protocol is private, closed, typed, bounded, versioned, and carried over an
untrusted channel. On the correct production path, relay application bytes are
end-to-end Noise ciphertext. The authority treats those bytes as opaque and
cannot prove ciphertext provenance against a compromised application processor.

## Decision Drivers

- Bind device authentication to the exact configured relay server without
  trusting `service-net` assertions.
- Preserve standard TLS 1.3 mutual authentication and ADR-0005 certificate
  lifecycle semantics.
- Keep private-key bytes, TLS traffic secrets, and generic signing authority out
  of `service-net`, net-broker, VFS, and the public KMS API.
- Keep public KMS opcodes 9–14 byte-for-byte compatible.
- Make transcript, Finished, retry, cancellation, and teardown state have one
  authoritative owner.
- Bound authority memory, sessions, records, chains, timeouts, and transport
  chunks without allocation-dependent correctness.
- Preserve Noise as the end-to-end application security boundary.

## Considered Options

### Option A (chosen): Protected authority owns the complete relay TLS client endpoint

- **Pro**: The authority validates the exact server chain, hostname, validity,
  Server CertificateVerify, Finished, and active client profile in the same TLS
  state machine that creates Client CertificateVerify.
- **Pro**: TLS traffic secrets and the device private key never enter
  `service-net`.
- **Pro**: `service-net` becomes a bounded TCP/TLS-record byte carrier with no
  security decision to fake.
- **Pro**: Public KMS opcodes remain unchanged; the production relay path uses a
  separately versioned private authority protocol.
- **Pro**: On the correct production path, the authority sees TLS control data
  plus typed Noise-record application bytes; it treats their contents as opaque.
- **Con**: The protected TCB gains a bounded TLS 1.3 client engine, certificate
  verifier, record layer, and more state-machine tests.
- **Con**: The 1,200-byte authority frames require explicit chunking for
  certificate chains and TLS records.
- **Chosen because**: It gives one owner enough information to verify the relay
  and produce the client authentication proof without a split-state trust gap.

### Option B: Mirror the service-net handshake inside the authority

- **Pro**: Reuses the current service-net TLS connection and initially moves less
  record-processing code.
- **Pro**: The authority could independently verify the server certificate and
  Server CertificateVerify from mirrored handshake bytes.
- **Con**: Securely binding the final client transcript also requires protected
  transcript continuation, key-schedule continuity, and server Finished
  verification.
- **Con**: Fragmentation, HelloRetryRequest, alerts, cancellation, and retries
  must keep two state machines byte-identical.
- **Rejected because**: Once the missing Finished/key-schedule checks are added,
  it approaches a second TLS engine while retaining a fragile split-brain seam.

### Option C: Replace mTLS with server-auth TLS plus relay challenge-response

- **Pro**: A relay-signed fresh challenge could provide explicit target binding
  to a smaller protected signing protocol.
- **Pro**: The authority would not need to own the full TLS record layer.
- **Con**: It creates a custom admission protocol, replay/expiry state, and new
  server/client framing.
- **Con**: It abandons standard TLS client-certificate authentication and makes
  existing mTLS certificate-policy work partially obsolete.
- **Rejected because**: ADR-0005 deliberately chose managed-CA mTLS lifecycle and
  standard client authentication; no evidence justifies replacing it.

### Option D: Keep signing in service-net and add protected pins

- **Pro**: Smallest code change.
- **Con**: A hostname, CA, or SPKI pin does not prove which server produced an
  opaque caller-supplied transcript hash.
- **Con**: It treats an explicitly untrusted component as the authorization
  oracle and preserves the confused-deputy flaw.
- **Rejected because**: It does not meet the target-binding requirement.

## Decision

The Protected Relay Authority owns the complete TLS 1.3 client endpoint for the
relay.

1. **Single TLS state owner.** The authority owns client random and ECDHE state,
   transcript hashing, server chain/hostname/validity verification, Server
   CertificateVerify and Finished verification, client-chain selection, Client
   CertificateVerify, traffic secrets, and TLS record seal/open.
2. **Untrusted byte carrier.** `service-net` opens only the fixed configured relay
   socket for a live privileged broker generation and transports bounded TLS
   record chunks. It cannot select a hostname, CA, certificate, profile, key,
   signature scheme, or alternate destination and cannot observe TLS secrets.
3. **Typed opaque application data.** Net-broker's production API supplies and
   receives only bounded Noise-record buffers through the privileged relay
   tunnel. The authority treats their contents as opaque. This contract prevents
   accidental plaintext routing but does not let the authority distinguish
   malicious caller-supplied plaintext from Noise ciphertext.
4. **Private protocol revision.** A reviewed next version of the private
   authority protocol will provide closed typed relay-TLS session, chunk,
   application-record, close, and cancellation operations. Each operation binds
   device/authority identity, boot epoch, monotonic sequence, session generation,
   active profile, configured endpoint, and authenticated request context.
5. **Public ABI freeze.** Public KMS opcodes 9–14 and their encodings do not
   change. The old transcript-hash signing opcode remains fixture/compatibility
   surface and must deny in production; the relay client never uses it.
6. **Fail-closed bounds.** At most the approved relay-session count may exist.
   Certificate chains, handshake bytes, TLS records, Noise frames, chunks,
   retries, timeouts, sequence numbers, and cancellation are bounded. Missing or
   rolled-back authenticated time, stale broker/session generation, profile or
   endpoint mismatch, malformed chunk order, duplicate/future sequence, alert,
   EOF, or authority reset destroys the session and returns no plaintext or
   signature fallback.
7. **No generic protected TLS service.** The authority supports only the fixed
   relay profile and endpoint. Generic TLS, caller-selected destinations,
   arbitrary signing, private-key export, raw TCP identity proof, K1 proof, and
   insecure fallback remain unrepresentable.
8. **Evidence ceilings remain.** A software or QEMU authority implementation is
   `DEV_REFERENCE` only. Production still requires every protected persistence,
   authenticated time, pending-key binding, exact-product, physical, and release
   gate already recorded by ADR-0006 and the KMS/Silo plan.

## Consequences

### Positive

- A compromised `service-net` cannot redirect the protected device identity to
  an attacker-selected TLS server.
- One state machine owns server authentication and client authentication, so
  transcript and Finished claims are testable without cross-component trust.
- Public KMS compatibility and the generic server-auth TLS API remain intact.
- On the correct production path, relay TLS termination does not weaken
  end-to-end confidentiality because the application bytes are already Noise
  ciphertext.

### Negative / Risks

- The authority TCB and private protocol become larger.
- TLS record chunking and cancellation across an untrusted carrier add bounded
  state that needs hostile sequence, replay, truncation, and reset tests.
- Existing embedded-tls client code cannot simply call the old KMS signer; a
  protected adapter or dedicated bounded TLS engine is required.
- Authority outages close relay sessions; there is no availability downgrade.
- An application-processor compromise can submit arbitrary bytes to the fixed
  relay because the authority cannot authenticate Noise provenance. That
  compromise is outside this authority's confidentiality guarantee; it still
  cannot redirect the protected TLS identity or extract TLS keys.

## Verification

Acceptance evidence must prove:

- an attacker TLS server, wrong CA/hostname, stale server certificate, invalid
  Server CertificateVerify, bad Finished, or modified transcript fails before
  Client CertificateVerify is emitted;
- `service-net` cannot request a standalone production signature or choose any
  TLS identity/profile/endpoint input;
- stale generation, replayed/out-of-order chunks, cancellation, EOF, timeout,
  authority reset, and profile rotation destroy the exact session without
  affecting another generation;
- public KMS opcode 9–14 byte fixtures remain unchanged and the legacy signing
  request is denied by production providers;
- the production broker API accepts only typed bounded Noise-record buffers,
  instrumented honest-path runs carry no plaintext C2C envelopes or Cell
  payloads, and the evidence explicitly makes no malicious-caller provenance
  claim;
- retained logs contain no keys, TLS secrets, signatures, certificates, Noise
  payloads, or unrestricted buffers;
- bounded chain, record, session, and memory limits hold under hostile input; and
- a two-node relay oracle passes only after every upstream protected-identity
  gate opens, with its evidence ceiling stated explicitly.

## Links

- [ADR-0005](./0005-mutual-tls-relay-identity.md) — mTLS identity and certificate lifecycle remain authoritative; this ADR replaces only the client TLS ownership boundary.
- [ADR-0006](./0006-block-production-root-pending-exact-product-evidence.md) — production root implementation remains exact-product and evidence gated.
- [ADR-0007](./0007-development-first-hardware-constrained-execution.md) — DEV_REFERENCE implementation does not satisfy production admission.
- [Service-net mutual TLS plan](../../.agents/260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md) — implementation owner and blocked entry gates.
- [Relay-first C2C plan](../../.agents/260819-1409-cell-to-cell-anywhere-core/phase-05-relay-first-remote-correctness-oracle.md) — relay oracle and no-client status.
