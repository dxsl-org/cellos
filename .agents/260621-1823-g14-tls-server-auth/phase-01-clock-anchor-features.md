---
phase: 01
title: ViTlsClock + single trust anchor + build features
status: Done
tier: thinking
depends_on: [00]
owns:
  - cells/services/net/Cargo.toml
  - cells/services/net/src/tls/clock.rs        # new — Clock impl over sys_get_time
  - cells/services/net/src/tls/roots.rs        # new — build-selected single anchor + min-date
  - cells/services/net/roots/{private,isrg-x1,amazon-root-1}.der  # new — candidate roots (dev picks 1)
  - cells/services/net/build.rs                # VICELL_MIN_UNIX (extend if present)
  - cells/services/net/src/tls.rs              # add `pub mod clock; pub mod roots;`
---

## Context Links
- Plan: [plan.md](plan.md) · Spike API: `reports/spike-00-findings.md` (from P00).
- Prereq facts: `GetTime` op=3 epoch_secs (memory `project-rtc-wall-clock`); `forbid(unsafe_code)`.

## Overview
Build the production, non-crypto foundation locked by P00: a `Clock` impl over the RTC syscall (with the
min-date clamp), the single-anchor trust store, the vendored root DER, and the build-flavor features.
Compiles and is inert — no verification wired yet (that's P02).

## Key Insights
- `pki::CertVerifier` needs a `Clock` (P00 locked the trait) returning a `Time`; build it from
  `sys_get_time` op=3, clamped: `secs = max(rtc_secs, VICELL_MIN_UNIX)`.
- `pki.rs` is **single-anchor** → the store holds exactly one `Certificate<&'static [u8]>`. **Which**
  root is a build-time selector: vendor several candidate DERs, `include_bytes!` the one chosen by a
  mutually-exclusive CA sub-feature (`tls-ca-private` default / `tls-ca-letsencrypt` / `tls-ca-amazon`).
  (Multi-root is the deferred P04 full build.)
- Top-level flavors mutually exclusive; default `tls-roots-embedded` + `tls-ca-private`.

## Requirements
**Functional**
- `Cargo.toml`: enable embedded-tls `["alloc","ed25519","p384"]`. Tie `rsa` to the RSA CA sub-features
  (`tls-ca-letsencrypt` ISRG-X1 RSA-4096, `tls-ca-amazon` RSA-2048 → pull embedded-tls `rsa`); a pure-ECDSA
  `tls-ca-private` can omit `rsa` (smaller). Add `[features]`: `default=["tls-roots-embedded","tls-ca-private"]`,
  `tls-roots-embedded=[]`, `tls-roots-full=[]` (deps in P04), `tls-insecure=[]`,
  `tls-ca-private=[]`, `tls-ca-letsencrypt=["embedded-tls/rsa"]`, `tls-ca-amazon=["embedded-tls/rsa"]`,
  optional `tls-pin-skip-expiry=[]` (separate opt-in per Decisions). `compile_error!` on conflicting combos
  (two flavors, two CA selectors, or insecure+verifying).
- `src/tls/clock.rs`: a `Clock` impl (name per P00) → current `Time` from op=3, clamped to
  `VICELL_MIN_UNIX`. ~30 lines, pure, no panics.
- `src/tls/roots.rs`: `pub fn ca_cert() -> Certificate<&'static [u8]>` — `#[cfg]`-selects the DER per the
  active CA sub-feature (`tls-ca-private`/`-letsencrypt`/`-amazon`); `compile_error!` if 0 or >1 selected.
  Drop the SPKI-pin stub (YAGNI — add when a real pin + test exist).
- `roots/{private,isrg-x1,amazon-root-1}.der` + `roots/README.md`: each CA, source URL, `notAfter`,
  intended deployment. `private.der` = placeholder for the fleet's own CA (dev replaces at build).
- `VICELL_MIN_UNIX`: `build.rs` emits from `SOURCE_DATE_EPOCH`/build time (− 7 days skew); fallback const
  `1_748_736_000` (2025-06-01). No `Date::now()` (banned in this env anyway).

**Non-functional**
- Each new file < 200 lines. No `unsafe`. Embedded DER ~1-2 KB (one root).

## Architecture
```
sys_get_time(op=3) ─ clamp(max, VICELL_MIN_UNIX) ─ clock::now() : Time ─┐
include_bytes!(roots/ca.der) ─ roots::ca_cert() : Certificate ──────────┤─ consumed by P02 provider
VICELL_MIN_UNIX (build.rs) ─────────────────────────────────────────────┘
```

## Implementation Steps
1. `Cargo.toml`: embedded-tls feature set (from P00) + `[features]` + `compile_error!` guards.
2. Vendor `roots/ca.der` (the target CA) + `roots/README.md`.
3. `build.rs`: emit `VICELL_MIN_UNIX`.
4. `clock.rs`: `Clock` impl + clamp.
5. `roots.rs`: `ca_cert()`.
6. Register modules; `cargo check` default + `tls-insecure` (full deferred to P04).

## Todo
- [ ] Cargo flavors + CA-selector sub-features (mutually exclusive + `compile_error!`) + `rsa` tied to RSA CAs
- [ ] Vendor 3 candidate roots (`private`/`isrg-x1`/`amazon-root-1`) + README (source, expiry, deployment)
- [ ] build.rs `VICELL_MIN_UNIX`
- [ ] clock.rs (`Clock` impl + clamp)
- [ ] roots.rs (`cfg`-selected `ca_cert`)
- [ ] `cargo check` each CA selector (private/letsencrypt/amazon) + insecure flavor green

## Success Criteria
- Default + insecure flavors `cargo check` clean; modules inert-but-present.
- `ca_cert()` parses (unit test asserts the DER decodes to a `Certificate`).
- Clock clamp unit-tested: `now()` ≥ `VICELL_MIN_UNIX` even when RTC returns 0.

## Risks
- **Wrong/PEM cert in `ca.der`** → P02 verify always fails. Mitigate: unit test that `ca.der` DER-decodes.
- **CA expiry** → silent future breakage. Mitigate: record `notAfter` in README; revisit before it lapses.

## Security Considerations
- Min-date floor is a *minimum* only — never overrides a later valid RTC time, never moves time backward.
- One anchor = minimal trust surface (the recommended IoT model).

## Next Steps
P02 wires `ca_cert()` + `clock` into `pki::CertVerifier` inside a `ViTlsProvider` and replaces `UnsecureProvider`.
