# Phase 02 — Mutual-TLS Relay

## Context Links
`cells/services/net/src/tls/{provider,socket,transport}.rs`; `cells/services/net-broker/src/{relay,identity,transport}.rs`; `cells/services/{kms,silo}/`; `tools/relay-server/relay.py`.

## Overview
Replace raw-TCP relay identity with mutual TLS only after the protected signing and certificate-lifecycle prerequisites exist.

## Key Insights
`embedded-tls` 0.19 already implements TLS 1.3 client certificates and an
external `SignerMut`, so the TLS engine is not the blocker. Its CertificateVerify
path still calls infallible `sign()` and unwraps the encoded signature; a remote
signer failure needs an error-propagating dependency patch before use. Cellos
exposes no client-auth profile. KMS has no P-256 signing operation and deliberately
uses `UnavailableRootProvider`; Silo accepts unauthenticated callers and ships an
empty guest image. No durable identity key, issued matching certificate, or
service-net signing policy therefore exists.

## Requirements
No raw-TCP or K1-proof relay fallback. Mutual TLS must bind routing identity to a protected P-256 key, validate certificate hostname and CA, require client certificates at the server, bound every frame/error, and preserve end-to-end Noise payload encryption.

## Architecture
Derive NodeId from the mTLS certificate SPKI, with a certificate extension carrying the same value. Service-net performs TLS over its existing bounded transport; an attested Silo/KMS capability signs CertificateVerify without exposing private bytes; the server accepts only TLS 1.3 client-auth connections and routes by the authenticated NodeId.

## Related Code Files
`cells/services/net/src/tls/*`, `cells/services/silo/src/ipc.rs`, `cells/services/kms/src/dispatch.rs`, `libs/ostd/src/silo.rs`, `tools/relay-server/relay.py`.

## Implementation Steps
1. Add a production root backend with a durable opaque P-256 key, versioned
   handle, anti-rollback lifecycle, and certificate issuance/provisioning.
2. Add an attested service-net-only KMS signing policy. Silo must sit behind KMS
   or independently reject every caller except the authorized KMS boundary.
3. Patch `embedded-tls` CertificateVerify signing to propagate signer failure,
   then add a service-net relay profile with mounted CA/client chain, exact
   SPKI/NodeId/extension validation, and an opaque KMS signer.
4. Add a privileged service-net relay-connect operation authorized only for the
   attested net-broker generation. Keep the generic `TlsStream` server-auth only.
5. Rebind relay routing identity to the certificate-derived NodeId and run the
   guest/two-node TLS plus opaque-Noise relay exercise.
6. The external mandatory-mTLS relay server is complete and verified.

## Todo List
- [x] Choose mTLS relay direction.
- [ ] Provision protected signing and certificate lifecycle.
- [x] Implement and verify mandatory-mTLS relay server.
- [ ] Implement Cellos service-net/broker mTLS client.
- [ ] Run TLS/Noise relay oracle.

## Success Criteria
Only CA-issued, unrevoked client certificates with a matching SPKI-derived NodeId register. No stale disconnect deletes a replacement connection. Unavailable credentials, bad CA/SAN, bad certificate binding, malformed frames, and unavailable destinations fail closed.

## Risk Assessment
Using a VFS private key, transient NodeId, unauthenticated Silo request, or raw-TCP fallback defeats the requested mutual-authentication boundary.

## Security Considerations
Private keys never leave the Silo/KMS boundary. Server-side certificate validation occurs before routing registration. The broker still treats relay payloads as Noise ciphertext, not authenticated application plaintext.

## Next Steps
Production client work is blocked before service-net integration: implement the
durable KMS/Silo P-256 root and CA provisioning authority first. Adding a client
certificate adapter against the current unauthenticated, empty-image Silo would
create an exportable admission bypass rather than a usable intermediate.

## Provisioning Input
Fill and mount `tools/relay-enroll/mtls-mount-manifest.template.toml`
outside the repository. The template contains only paths, key handles, hashes,
and policy references; it deliberately accepts no private-key bytes.

## Server Evidence
`python3 -m unittest discover -s tools/relay-server -p '*_test.py' -v`
passes 23 focused certificate, TLS, identity, revocation, routing, duplicate,
cancellation, manifest, and bounded-error tests. Security re-review found no
remaining High/Medium server issue. This is server-only evidence; the obsolete
raw Cellos relay client has been removed and direct exhaustion fails closed with
`ViError::NotSupported`.