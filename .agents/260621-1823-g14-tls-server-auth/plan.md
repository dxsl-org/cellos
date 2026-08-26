# G14 — TLS Server Authentication + Certificate Chain Verification

**Status**: ✅ Done (P00–P03 complete; P04 deferred)
**Priority**: P1 — security gate; current TLS is MITM-vulnerable (`UnsecureProvider`)
**Created**: 2026-06-21 · **Revised**: 2026-06-21 (post red-team-01)
**Branch base**: `feat/cell-security-p1-p3`
**Plan dir**: `.agents/260621-1823-g14-tls-server-auth/`

---

## Context

ViCell's net cell speaks TLS 1.3 (embedded-tls 0.19) but uses `UnsecureProvider` at
[`socket.rs:56`](../../cells/services/net/src/tls/socket.rs#L56) — **server certificates are never
verified**; any on-path attacker can impersonate the cloud broker/API. The original TLS plan
(`.agents/260607-1109-tls/`) deferred this ("no RTC → can't check cert expiry"). RTC now ships
wall-clock epoch (`GetTime` op=3), so the deferral is due.

## Approach (corrected after red-team — verified against embedded-tls 0.19 source)

embedded-tls 0.19 ships **`pki.rs`** — a complete `no_std+alloc` certificate verifier behind the
`rustpki` feature (pulled by `rsa`/`ed25519`/`p384`). It walks the server chain to a **single** CA,
checks validity dates + RFC-6125 hostname, and verifies ECDSA/Ed25519/RSA signatures using RustCrypto
crates **already linked for the handshake**. This — not rustls-webpki — is the right engine for the
small embedded build.

Two flavors with **two engines** (the "one DRY path" idea was wrong — the builds have genuinely
different needs; that is the whole point of two flavors):

| Flavor (cargo feature) | Engine | Trust model | Size | When |
|------------------------|--------|-------------|------|------|
| `tls-roots-embedded` (default) | embedded-tls `pki::CertVerifier` | **single pinned CA** (broker/private/ISRG) | small (reuses RustCrypto) | G1 robot/IoT — **this plan's core** |
| `tls-roots-full` | rustls-webpki 0.103 multi-root | arbitrary public chains, many SANs | ~100 KB+ | server/PC — **deferred (phase-04)** |
| `tls-insecure` | `UnsecureProvider` (legacy) | accept-all | 0 | dev/lab only, never shipped |

Shared across flavors: `ViTlsClock`, the `ViTlsProvider` wiring, and the **invariant that
`verifier()` is infallible** (see Decisions). No `libs/api` change → **no Law 1**.

## Phases

| # | Phase | Status | Tier | Depends |
|---|-------|--------|------|---------|
| 00 | [Spike: prove `pki::CertVerifier`, lock API + size budget](phase-00-spike-pki-verifier.md) | ✅ Done | thinking | — |
| 01 | [`ViTlsClock` + single trust anchor + build features](phase-01-clock-anchor-features.md) | ✅ Done | thinking | 00 |
| 02 | [Verifying provider + handshake wiring + transport deadline](phase-02-provider-wiring.md) | ✅ Done | thinking | 01 |
| 03 | [E2E + negative tests (the gate) + docs + threat model](phase-03-e2e-tests-docs.md) | ✅ Done — gate test passes (SKIP on no-internet, FAIL on bypass) | medium | 02 |
| 04 | [Full build: rustls-webpki 0.103 multi-root **(deferred)**](phase-04-full-build-webpki.md) | Deferred | thinking | 03 |

P00→01→02→03 sequential. P04 is optional/deferred until a server/PC image is actually needed (YAGNI for G1).

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| TLS library | Keep `embedded-tls 0.19` | already has `pki.rs` no_std verifier — no switch |
| Embedded engine | `embedded_tls::pki::CertVerifier` (`rustpki`: `alloc+ed25519+p384+rsa`) | reuses linked RustCrypto; single-anchor fits pinned-broker model; ≤ size budget |
| Full engine | `rustls-webpki 0.103` + ring (deferred P04) | only path with multi-root + arbitrary SANs; heavy → server/PC only |
| `verifier()` infallibility | **`ViTlsProvider::verifier()` MUST always return `Ok`** + test asserts a bad cert ⇒ cap 0 | `connection.rs:455` silently skips verification if `verifier()` is `Err` — one typo = silent MITM |
| Trust anchor (embedded) | **one** root, but **which** root is a build-time selector: `tls-ca-private` (default) / `tls-ca-letsencrypt` / `tls-ca-amazon` (mutually exclusive sub-features) | `pki.rs` is single-anchor; dev pins the CA matching the deployment at image build; pinning is the recommended IoT model |
| Insecure mode | `tls-insecure` cargo feature **+ runtime "INSECURE TLS" banner + CI assertion** | Cargo unifies features additively — a feature alone can't guarantee "off"; need a loud runtime + build-gate signal |
| Clock-unset | clamp `time = max(rtc_secs, VICELL_MIN_UNIX)` | build-time floor stops epoch-0 from misbehaving |
| Expiry-skip-on-pin | **separate explicit opt-in flag**, off by default | "pin ⇒ skip expiry" silently makes a leaked pinned key permanently trusted (no revocation) |
| Transport timeout | wall-clock deadline (via `sys_get_time`) + `sys_heartbeat` in `transport.rs` spin loops | software RSA-PSS verify on QEMU TCG can exceed the iteration-count `MAX_SPIN` → false rejection |
| Revocation (OCSP/CRL) | **out of scope** (`revocation:None`) | industry standard for embedded; documented w/ mitigations |
| SAN/hostname caps | embedded build accepts `pki.rs` limits (3 SANs / 64 chars) — test against the **actual** broker cert; full build (P04) removes the cap | known constraint, not a surprise |

## Law 1

**Not triggered.** TLS opcodes (0x30-0x32) are net-cell-local; `GetRandom=214` + `GetTime=120 op=3`
already exist; flavor is a cargo feature. No `libs/api`/`libs/types` edit. (Insecure mode stays
build-time — no per-connection opcode — to keep it clear.)

## Success Criteria

1. Default embedded build: TLS connect to the target broker (correct cert, in-trust CA, matching host,
   valid date) **succeeds**.
2. **Gate:** every negative — untrusted/self-signed CA, expired cert, hostname mismatch, tampered
   chain/sig — **returns cap 0**. A negative that *connects* = silent bypass = ship-blocker.
3. A test proves the `connection.rs:455` `Ok` branch is taken (bad cert rejected ⇒ verifier ran).
4. `tls-insecure` build accepts anything **and** prints the INSECURE banner; CI asserts release images
   are not built with it.
5. Net cell compiles under `#![forbid(unsafe_code)]`; embedded binary growth within the budget measured
   in P00.
6. Handshake does not time-out under the verifying provider on QEMU TCG RV64 (deadline, not iteration cap).
7. `docs/specs/07-networking.md` documents what is verified + residual threats (RTC manipulation,
   pinned-key-no-revocation, DER-parser surface, no CT/name-constraints) + revocation-out.

## Risks (top — full table per phase)

- **R1 (blocker-class):** verifier silently no-ops (`verifier()` returns `Err`, or NEG test passes).
  Mitigate: infallible `verifier()`, P03 negative gate, distinguish timeout vs reject in logs.
- **R2 (high):** §4.4.3 / signature path wrong → forged sigs accepted. Mitigate: `pki.rs` already does
  this correctly (`pki.rs:136`); we reuse it rather than hand-roll. P03 forged-sig test is the proof.
- **R3 (med):** slow software verify times out handshake (false rejection). Mitigate: transport deadline + heartbeat (P02).
- **R4 (med):** embedded `pki.rs` SAN/host caps reject the real broker cert. Mitigate: P00 tests the actual cert; full build (P04) if arbitrary certs needed.

## Research & Review

- Verdicts: [`scout-report.md`](scout-report.md) (3 researcher reports).
- Adversarial review: [`reports/red-team-01.md`](reports/red-team-01.md) — REVISE; drove this v2.
- **Correction logged:** Researcher #1 missed `pki.rs`; the verified engine is `embedded_tls::pki`, not rustls-webpki, for the embedded build.
