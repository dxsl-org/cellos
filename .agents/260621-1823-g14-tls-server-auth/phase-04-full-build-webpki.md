---
phase: 04
title: "Full build: rustls-webpki 0.103 multi-root (DEFERRED)"
status: Deferred
tier: thinking
depends_on: [03]
owns:
  - cells/services/net/Cargo.toml              # tls-roots-full deps
  - cells/services/net/src/tls/verify_full.rs  # new — custom TlsVerifier over webpki 0.103
  - cells/services/net/src/tls/roots_full.rs   # new — webpki-roots / curated multi-root
---

## Status: DEFERRED — build only when a server/PC OS image is actually required (YAGNI for G1)

## Context Links
- Plan: [plan.md](plan.md) · Why deferred: G1 robot/IoT targets a known broker → single-anchor `pki.rs`
  (P00-P03) is sufficient and far smaller. This phase exists so the option is designed, not built early.

## Overview
The `tls-roots-full` flavor for server/PC images that must reach **arbitrary** public TLS hosts with
**many SANs** and **multiple root CAs** — neither of which `pki.rs` supports (single-anchor; 3-SAN cap).
Implements a custom `TlsVerifier` backed by rustls-webpki 0.103 + ring, parameterized by a multi-root
anchor set, reusing the same `ViTlsProvider` wiring + `ViTlsClock` from P01/P02.

## Key Insights
- This is the **second engine** the red-team identified as genuinely necessary for the full build —
  not a DRY duplicate. Same `CryptoProvider`/handshake wiring, different `verify_certificate`/
  `verify_signature` body.
- Must reconstruct §4.4.3 signed data ourselves (rustls-webpki doesn't do the TLS-1.3 CertificateVerify
  step) — mirror `embedded-tls pki.rs:136-149` byte-for-byte; the forged-sig test from P03 must pass here too.
- `verify_for_usage(algs, anchors, intermediates, UnixTime, server_auth, None, None)` +
  `verify_is_valid_for_subject_name(&ServerName)`. **Confirm exact 0.103 signature via `cargo doc`** —
  it differs from the in-tree 0.101.
- Size: ~100 KB+ (ring + RSA). Acceptable for server/PC, not embedded.

## Requirements (when built)
- `Cargo.toml` under `tls-roots-full`: `rustls-webpki 0.103` (default-features=false, `alloc`),
  `rustls-pki-types 1`, a ring backend crate (path confirmed via `cargo doc`), optional `webpki-roots 0.26`.
- `roots_full.rs`: `fn trust_anchors() -> &'static [TrustAnchor<'static>]` = `webpki-roots::TLS_SERVER_ROOTS`
  (or a curated subset).
- `verify_full.rs`: custom `TlsVerifier` (chain via `verify_for_usage`, validity via `ViTlsClock`,
  hostname via `verify_is_valid_for_subject_name`, §4.4.3 sig reconstruction). `verifier()` still infallible (B3).
- `ViTlsProvider` selects engine by feature: `pki::CertVerifier` (embedded) vs `verify_full` (full).
- Reuse P03's full negative suite + add a multi-SAN positive (cert with > 3 SANs) that `pki.rs` would reject.

## Todo (when activated)
- [ ] Confirm webpki 0.103 API + ring algorithm static paths via `cargo doc`; check no dup-webpki conflict
- [ ] `roots_full.rs` multi-root anchors
- [ ] `verify_full.rs` custom `TlsVerifier` (+ §4.4.3) — infallible `verifier()`
- [ ] feature-select engine in `ViTlsProvider`
- [ ] full-flavor negative suite + multi-SAN positive
- [ ] size measured; reviewer pass

## Success Criteria
- Full build connects to arbitrary public hosts (multi-root, many SANs); all negatives still reject;
  forged-sig still rejects; `verifier()` infallible.

## Risks
- **Two security-critical engines** double the audit surface. Mitigate: shared wiring + identical negative
  suite; this engine only ships in server/PC images.
- **0.103 API drift / dup-webpki bloat.** Mitigate: `cargo doc` confirmation + keep embedded-tls `webpki` feature OFF.

## Security Considerations
- Same fail-closed + infallible-`verifier()` invariants as P02. Residual-threat doc (P03) covers both engines.

## Next Steps
Activate when a server/PC ViCell image is on the roadmap. Until then, G1 ships embedded (P00-P03).
