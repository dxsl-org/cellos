---
phase: 00
title: "Spike: prove pki::CertVerifier, lock API + size budget"
status: Planned
tier: thinking
depends_on: []
owns:
  - scratch spike (throwaway) + reports/spike-00-findings.md
---

## Context Links
- Plan: [plan.md](plan.md) · Red-team: [reports/red-team-01.md](reports/red-team-01.md)
- Source of truth: `~/.cargo/registry/src/*/embedded-tls-0.19.0/src/{pki.rs,connection.rs,config.rs,der_certificate.rs}`

## Overview
De-risk the entire plan **before committing any deps**. The red-team showed the prior premise was wrong;
this spike replaces guesses with measured facts: does `embedded_tls::pki::CertVerifier` validate a real
chain, what is the exact API to wire it, and how much binary does `rustpki` add? Output is a findings
doc that locks P01–P03's design. **Throwaway code — nothing here ships.**

## Key Insights (verified facts going in)
- `pki.rs:67` `CertVerifier<'a, CipherSuite, Clock, const CERT_SIZE>`; `new(ca: Certificate<&[u8]>)` (single CA).
- Implements `TlsVerifier` (`verify_certificate` walks `CertificateChain::new(&ca, &cert)` `pki.rs:119`;
  `verify_signature` does §4.4.3 reconstruction `pki.rs:136`).
- `Clock` is a `pki`-local trait returning `Time`; must be impl'd over `sys_get_time` op=3.
- `connection.rs:455` skips verification if `verifier()` returns `Err` — the provider must return `Ok`.
- Caps: `MAX_SAN_DNS_NAMES=3`, `HOSTNAME_MAXLEN=64` (`der_certificate.rs:11-12`).

## Requirements (questions the spike MUST answer)
1. **API lock:** exact signatures of `pki::CertVerifier::new`, the `Clock`/`Time` trait, `CERT_SIZE`
   sizing, and how `CryptoProvider::verifier()` returns `&mut CertVerifier` (lifetime/ownership).
2. **Chain reality:** feed the **actual target broker cert** (leaf + intermediate) + its issuing CA →
   does `verify_certificate` return `Ok`? Confirm it walks ≥2 entries (not leaf-only like `webpki.rs`).
3. **Negative:** feed a self-signed / wrong-CA cert → confirm `Err` (not `Ok`).
4. **SAN/host caps:** does the real broker cert fit within 3 SANs / 64-char names? If not, the embedded
   build can't use `pki.rs` for that endpoint → escalate (pin a different name, or require P04 full build).
5. **Size:** `cargo bloat`/`size` on the net-cell `.elf` with `rustpki` (`alloc+ed25519+p384+rsa`) vs
   baseline → set the real budget number (replaces the guessed ≤20 KB).
6. **RSA need:** is the broker/CA RSA or ECDSA? If pure ECDSA, can we drop the `rsa` feature (smaller)?
7. **Dep conflict:** confirm enabling `rustpki` does NOT also pull embedded-tls's `webpki`(0.101) feature.

## Implementation Steps
1. In a scratch crate (or a temporary `[features]` toggle on service-net), enable embedded-tls
   `["alloc","ed25519","p384","rsa"]`; write a minimal `Clock` + `CryptoProvider` returning a
   `pki::CertVerifier`.
2. Embed test DER: real broker leaf+intermediate+CA, and one bad cert. Call `verify_certificate` directly
   (unit-level, no network) — assert Ok / Err respectively.
3. Measure `.elf` size delta vs current net cell.
4. Record all answers in `reports/spike-00-findings.md`; **revert the spike code**.

## Todo
- [ ] Scratch `Clock` + `CryptoProvider` + `pki::CertVerifier` compiles (no_std cell)
- [ ] Real broker chain → Ok; bad cert → Err (unit-level)
- [ ] SAN/host-cap check against real broker cert
- [ ] Binary size delta measured (real budget number)
- [ ] ECDSA-vs-RSA decision (can we drop `rsa`?)
- [ ] Confirm no webpki-0.101 pull-in
- [ ] `reports/spike-00-findings.md` written; spike code reverted

## Success Criteria
- A findings doc that (a) gives copy-pasteable API for P02, (b) confirms real-chain Ok + bad-cert Err,
  (c) states the measured size budget, (d) confirms the broker cert fits the SAN/host caps **or**
  routes that endpoint to the P04 full build.

## Risks
- **Broker cert exceeds caps** → embedded `pki.rs` unusable for it. Mitigate: decide now — pin a shorter
  SAN, switch to a private CA, or mark that endpoint full-build-only. Better to learn here than in P03.
- **Size blows budget** → reconsider feature set (drop `rsa`) or accept a larger embedded image.

## Security Considerations
- This is the cheapest place to discover a fundamental blocker. Do not skip to P01 until B1/B2/M1/M2/M5
  from the red-team are answered with measurements.

## Next Steps
P01 builds the production `ViTlsClock` + anchor store from the API locked here.
