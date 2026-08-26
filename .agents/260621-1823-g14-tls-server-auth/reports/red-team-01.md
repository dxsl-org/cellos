# Red-Team 01 — G14 TLS Server-Auth Plan (verdict: REVISE)

Adversarial review by haily-brainstormer (opus), 2026-06-21. All claims below were
**verified by the planner against the installed embedded-tls 0.19 source**
(`~/.cargo/.../embedded-tls-0.19.0/src/`).

## Verified findings

| # | Sev | Finding | Source proof |
|---|-----|---------|--------------|
| B1 | blocker | embedded-tls 0.19 ships `pki.rs` — a complete **no_std+alloc** verifier (`rustpki` feature, pulled by `rsa`/`ed25519`/`p384`). Original premise "must add rustls-webpki 0.103" is wrong for the embedded build. | `src/pki.rs`; `Cargo.toml` features 62-76 |
| B2 | blocker | `pki.rs CertVerifier::new(ca)` is **single-anchor** (walks leaf+intermediates to ONE CA). `webpki.rs` is **leaf-only** (`// TODO: Support intermediates`, `entries[0]`). Neither does multi-root path building → full public-web build genuinely needs rustls-webpki 0.103. "One DRY code path" claim is false. | `pki.rs:85,119`; `webpki.rs:197,243` |
| B3 | blocker | **Silent-bypass landmine**: `if let Ok(verifier)=provider.verifier() {…?} else {debug!("skipped")}` — if `verifier()` returns `Err`, cert check is skipped and handshake succeeds. Default returns `Err(Unimplemented)`. | `connection.rs:455-467`; `config.rs:147` |
| M1 | major | rustls-webpki 0.103 + ring + RSA adds **>100 KB**, not the planned ≤20 KB. Dual-webpki (0.101 vendored + 0.103) doubles parsers. | `Cargo.toml:239` (webpki=0.101) |
| M2 | major | `pki.rs` caps `MAX_SAN_DNS_NAMES=3`, `HOSTNAME_MAXLEN=64` → silently rejects valid multi-SAN / long-name public certs. OK for a pinned broker; not for arbitrary public web. | `der_certificate.rs:11-12` |
| M3 | major | min-date clamp only defends the floor; a manipulated-but-plausible RTC defeats expiry. "Pin bypasses expiry" + `revocation:None` ⇒ a leaked pinned key is permanently MITM-capable. Under-documented. | threat analysis |
| M4 | major | Transport spin-loop (`MAX_SPIN` iteration count, no heartbeat) can time out a slow software RSA-PSS verify on QEMU TCG → `Err` → cap 0, **indistinguishable from a real rejection**. | `transport.rs:86,107`; `handlers.rs:432` |
| M5 | major | Plan's webpki API is 0.103's; only 0.101 is in-tree. "Decision gate" defers the highest-risk unknown into mid-P02. Must be a Phase-0 spike before deps. | `webpki.rs:257`; phase-02 |
| m1 | minor | Empty-SNI already fails closed in `pki.rs` (`host=None` matches only no-SAN certs); keep explicit reject + test. | `pki.rs:494` |
| m2 | minor | `tls-insecure` as a cargo feature is unsafe — Cargo unifies features additively across the graph. Add runtime "INSECURE TLS" banner + CI assertion, not feature-only. | Cargo feature unification |
| m3 | minor | SPKI-pin stub threaded through 3 phases for a `None` that's never tested = YAGNI. Embedded needs 1 root, not 8. | phase-01/02 |
| T1 | threat | Residual risks undocumented: RTC manipulation, pinned-key+no-revocation, DER-parser DoS surface on hostile certs, no CT/name-constraints. (TLS-1.3-only = no downgrade — state as positive.) | — |

## Single most important thing
Run a Phase-0 spike compiling `embedded_tls::pki::CertVerifier` (features `alloc+ed25519+p384+rsa`)
against the real broker cert + a bad cert and **measure binary growth** before writing any other phase.
That resolves B1, B2, M1, M2, M5. And **prove the `connection.rs:455` `Ok` branch is taken** via a
negative test (bad cert → cap 0) — that one assertion separates "verifying" from "silently MITM-able".

## Disposition
Plan revised 2026-06-21: added phase-00 spike; embedded build now uses `pki.rs` (single pinned CA) as the
G1 core; full multi-root build (rustls-webpki 0.103) deferred to phase-04; silent-bypass invariant +
transport deadline + residual-threat doc folded into phases 02/03.
