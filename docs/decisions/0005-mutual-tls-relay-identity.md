# ADR-0005: Use mutual TLS for external relay identity

**Date**: 2026-08-25  
**Status**: Accepted  
**Deciders**: Cellos maintainer

## Context

The external relay is an interop trust boundary. It routes opaque traffic by
NodeId, but the former raw-TCP `CLIENT_REGISTER` protocol let a client assert
any NodeId without cryptographically binding it to the connection. The existing
per-boot X25519 routing identity is also unsuitable as a durable, individually
revocable relay credential.

The relay needs a stable identity derived from a protected device key while
Noise remains the end-to-end protection for forwarded payloads. Mutual TLS is
limited to this external boundary; it does not replace native Cell-to-Cell
Noise.

A secure Cellos client path does not yet exist. It is blocked on a KMS signer
backed by separately selected, implemented, and qualified production hardware
through Phases 6–8, attested service-net authorization, certificate issuance
and provisioning, and an mTLS client-auth profile. The system must fail closed
until all prerequisites exist. The KMS-internal AArch64-QEMU Silo provider is
`DEV_REFERENCE` evidence only and cannot satisfy this production gate.

## Decision Drivers

- Bind routing identity to an authenticated, non-extractable per-device key.
- Support per-device issuance and revocation without a shared fleet admission
  secret.
- Preserve Noise as end-to-end payload protection.
- Prevent downgrade to unauthenticated relay registration.
- Bound relay resource use and reject duplicate live identities without route
  displacement or stale-cleanup races.
- Avoid treating completed server work as completion of the blocked client path.

## Considered Options

### Option A: Raw TCP with `CLIENT_REGISTER`

- **Pro**: Simple and requires no credential lifecycle.
- **Pro**: Compatible with the former broker client.
- **Con**: NodeId is attacker-asserted and unrelated to the connecting peer.
- **Con**: A fallback would bypass stronger admission policy.
- **Rejected because**: It does not authenticate relay identity.

### Option B: K1 challenge-response

- **Pro**: Adds cryptographic admission without a certificate authority.
- **Pro**: Reuses an existing cluster secret.
- **Con**: It couples external relay admission to one shared cluster secret;
  compromise expands across every peer trusting K1.
- **Con**: It lacks clean per-device issuance and revocation and does not itself
  bind a particular NodeId to the prover.
- **Rejected because**: A shared cluster secret is the wrong identity and
  revocation boundary for an external relay.

### Option C: mTLS with a filesystem private-key PEM

- **Pro**: Works with conventional TLS tooling at low integration cost.
- **Con**: Filesystem key bytes are extractable and copyable.
- **Con**: It bypasses KMS custody and attested signing authorization.
- **Rejected because**: Possession of an exportable file cannot serve as the
  protected device identity.

### Option D (chosen): TLS 1.3 mTLS with a managed CA and protected device key

- **Pro**: Provides standard mutual authentication, per-device revocation, and
  auditable trust anchors.
- **Pro**: A certificate-derived NodeId cannot be chosen independently by the
  client.
- **Pro**: The private key can remain non-extractable behind KMS and a separately qualified production hardware provider.
- **Con**: Requires CA operation, certificate profiles, secure provisioning,
  rotation, revocation, protected signing, and trust overlap.
- **Chosen because**: It alone combines authenticated routing identity,
  protected key custody, and per-device lifecycle control. It is selected
  despite its provisioning cost.

## Decision

The external relay accepts **TLS 1.3 mutual-authentication connections only**.
There is no raw-TCP or K1-proof fallback. Obsolete broker `RelayClient` and
`CLIENT_REGISTER` paths remain removed.

The authenticated leaf certificate must contain a P-256 public key, validate
under the configured client CA set, permit `clientAuth` extended key usage, and
carry private X.509 extension OID `1.3.6.1.4.1.55555.1.1`.

The canonical NodeId is:

`SHA-256(SPKI DER)`

where `SPKI DER` is the complete DER-encoded SubjectPublicKeyInfo from the leaf
certificate. The extension must contain exactly that 32-byte value. Any
mismatch fails before route registration.

The Protected Relay Authority owns the complete relay TLS client endpoint under
[ADR-0008](./0008-protected-relay-tls-endpoint-ownership.md): server
chain/hostname/time verification, handshake transcript and Finished validation,
client profile selection, CertificateVerify, traffic secrets, and TLS records.
`service-net` is only the bounded untrusted byte carrier. Private-key bytes and
TLS secrets must never be provisioned to the filesystem or returned across the
authority boundary.

The server starts only from a mandatory mounted manifest defining its TLS
material, client trust roots, NodeId denylist, and limits. Before registration
it validates the chain, `clientAuth` EKU, P-256 key, NodeId binding, and denylist.
Sessions, frames, I/O, and delivery errors are bounded. A second live session
with the same certificate-derived NodeId is rejected without displacing the
current route. Monotonic route generations prevent stale disconnect cleanup
from removing a later explicitly re-admitted route.
Destination absence detected before any destination write is a definite
unavailable outcome. Once a forwarding write is attempted, a write, TLS, or
drain failure is delivery-uncertain because bytes may already be queued. The
server reports distinct bounded errors and emits no success receipt. The
uncertain error is channel-level because the relay cannot inspect opaque C2C
request IDs; future pipelining requires conservative outstanding-work handling
or a separately approved correlated framing revision.

The relay forwards payloads as opaque Noise ciphertext. mTLS authenticates the
external relay hop; it does not terminate or replace Noise.

## Consequences

### Positive

- A stable certificate-derived NodeId replaces the per-boot X25519 routing
  identity for relay admission and routing.
- Route ownership is bound to a CA-issued protected device key.
- Individual devices can be revoked without rotating a shared cluster secret.
- Noise remains the end-to-end payload security boundary.
- Missing credentials or prerequisites fail closed rather than downgrading.

### Negative / Risks

- Certificate issuance, provisioning, renewal, revocation, and protected signer
  availability become operational dependencies.
- CA rotation requires planned trust overlap.
- The server-side denylist must be distributed and mounted accurately.
- Compromise of the CA or signing authorization can mint or exercise trusted
  relay identities.

### Blocked prerequisites and non-goals

- A durable production P-256 key and KMS signing-root lifecycle backed by the
  hardware provider selected, implemented, and qualified through Phases 6–8.
- Attested authorization limiting identity-signing requests to service-net.
- Managed-CA certificate issuance, provisioning, rotation, trust anchors, and
  the private NodeId extension profile.
- A protected-authority TLS client endpoint with fixed client chain,
  hostname/CA policy, bounded record transport, and live broker authorization.
- Until these exist, the Cellos relay path remains blocked and direct Noise
  address exhaustion returns `NotSupported`; raw fallback is forbidden.
- Server-only completion does **not** complete the Cellos client,
  enrollment/lease wiring, or the end-to-end relay path.

## Security

Validation precedes registration and fails on an untrusted chain, wrong or
missing `clientAuth` EKU, non-P-256 key, malformed or mismatched NodeId
extension, denylisted NodeId, invalid server hostname, or unavailable protected
signer. A certificate is insufficient if its key is exportable or arbitrary
callers can invoke its signer.

The mandatory manifest prevents permissive defaults. Resource bounds limit
exhaustion. Duplicate live NodeIds fail before route mutation, and exact
generation release prevents a stale disconnect from removing a later route.
The relay must not parse Noise ciphertext as authenticated application
plaintext.

## Verification

Acceptance evidence must show that:

- TLS versions below 1.3, missing client certificates, invalid chain/EKU/key,
  malformed or mismatched NodeId extension, and denylisted identities fail
  before registration;
- NodeId is exactly `SHA-256(SPKI DER)` and matches OID
  `1.3.6.1.4.1.55555.1.1`;
- startup fails without a valid mounted manifest, limits hold, duplicate live
  NodeIds cannot displace the current route, and stale cleanup cannot remove a
  later explicitly re-admitted session;
- destination absence and post-write/drain failure produce distinct definite
  unavailable and delivery-uncertain errors without a false success receipt;
- the protected authority validates the server CA/hostname and owns client
  CertificateVerify plus TLS record keys, while `service-net` transports only
  bounded bytes and no component exposes key material; and
- a two-node exercise preserves opaque end-to-end Noise traffic and fails closed
  without raw fallback.

Current evidence directly covers TLS 1.3 and required-client-certificate
configuration, missing/untrusted client certificates, NodeId extension binding,
denylist enforcement, bounded pre-TLS connections, routing, duplicate-live
rejection, definite/uncertain delivery-error separation, manifest validation,
missing/wrong `clientAuth` rejection, non-P-256 rejection, and route/session
limits. The server-only certificate-policy contract is covered at the host-test
ceiling. Client mTLS and the two-node TLS/Noise exercise remain blocked by the
prerequisites above.

## Links

- [ADR-0006](./0006-block-production-root-pending-exact-product-evidence.md) — no production root is selected; exact vendor product, firmware, boot, state, time, and board evidence gate the client path.
- [ADR-0007](./0007-development-first-hardware-constrained-execution.md) — protected relay identity remains a production milestone gate and does not block bounded local-runtime, QEMU, RPi3, or sensor development.
- [ADR-0008](./0008-protected-relay-tls-endpoint-ownership.md) — supersedes the relay client ownership boundary: the protected authority, not service-net, owns the complete TLS endpoint.
- `.agents/260825-sdk-delivery/phase-02-relay.md` — relay direction,
  prerequisites, acceptance conditions, and server-only/client-blocked status.
- `tools/relay-enroll/mtls-mount-manifest.template.toml` — provisioning
  inputs without private-key bytes.
- `docs/project-roadmap.md` — current relay status and client blockers.
- `docs/system-architecture.md` — Noise for native transport and mTLS at the
  external/interop boundary.
