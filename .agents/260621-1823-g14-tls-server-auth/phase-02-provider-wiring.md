---
phase: 02
title: Verifying provider + handshake wiring + transport deadline
status: Done
tier: thinking
depends_on: [01]
owns:
  - cells/services/net/src/tls/provider.rs     # new — CryptoProvider (infallible verifier())
  - cells/services/net/src/tls/socket.rs       # swap UnsecureProvider → ViTlsProvider
  - cells/services/net/src/tls/transport.rs    # iteration-count → wall-clock deadline + heartbeat
  - cells/services/net/src/tls.rs              # add `pub mod provider;`
---

## Context Links
- Plan: [plan.md](plan.md) · Red-team B3/M4: [reports/red-team-01.md](reports/red-team-01.md).
- Touch points: [socket.rs:49-57](../../cells/services/net/src/tls/socket.rs#L49-L57),
  [transport.rs:86,107](../../cells/services/net/src/tls/transport.rs#L86),
  [handlers.rs:427-433](../../cells/services/net/src/handlers.rs#L427-L433).

## Overview
Wrap P00/P01's `pki::CertVerifier` (single anchor + `ViTlsClock`) in a `ViTlsProvider` whose
`verifier()` is **infallible**, swap it for `UnsecureProvider` in `handshake()` (gated so only
`tls-insecure` keeps the old path), and fix the transport spin-loop so a slow software verify can't be
misread as a verification failure. Verification failure already propagates to cap 0 via
`handlers.rs:432` — no dispatch change.

## Key Insights
- **B3 — the silent-bypass:** `connection.rs:455` skips verification if `verifier()` returns `Err`.
  `ViTlsProvider::verifier()` MUST `Ok(&mut self.verifier)` unconditionally — never a fallible path.
- The verifier is **per-handshake** (owns transcript/host state) — construct inside `handshake()`.
- **M4 — transport timeout:** `transport.rs` busy-spins on an *iteration count* (`MAX_SPIN`) with no
  heartbeat. Software RSA-PSS verify on QEMU TCG is slow and happens between `read()` calls; a multi-
  segment cert flight can exhaust the count → `TimedOut` → `Err` → cap 0, **indistinguishable from a
  real rejection**. Switch to a wall-clock deadline (`sys_get_time`, already imported `transport.rs:18`)
  and add `sys_heartbeat` so the RT watchdog doesn't trip during a long verify.
- Empty hostname: `pki.rs` fails closed (host=None matches only no-SAN certs), but reject explicitly
  pre-`open()` in verifying builds for a clear error.

## Requirements
**Functional**
- `provider.rs`: `ViTlsProvider { rng: ViRng, verifier: pki::CertVerifier<…> }` impl `CryptoProvider`
  (`type CipherSuite = Aes128GcmSha256`; `rng()`→`&mut self.rng`; **`verifier()`→`Ok(&mut self.verifier)`
  always**). `new(hostname)` builds the `CertVerifier` from `roots::ca_cert()` + `ViTlsClock` + sets
  hostname.
- `socket.rs handshake()`:
  ```
  #[cfg(not(feature = "tls-insecure"))]
  let ctx = TlsContext::new(&config, ViTlsProvider::new(ViRng::new(), hostname));
  #[cfg(feature = "tls-insecure")]
  let ctx = TlsContext::new(&config, UnsecureProvider::new::<Aes128GcmSha256>(ViRng::new()));
  ```
  Verifying build + empty hostname → return `Err` before `open()`. Gate `UnsecureProvider` import behind
  `tls-insecure`. On `tls-insecure`, print a one-time `[net/tls] !!! INSECURE TLS BUILD — certs NOT verified !!!` banner.
- `transport.rs`: replace `MAX_SPIN` iteration caps in `Read`/`Write` with a wall-clock deadline
  (`now + TLS_IO_TIMEOUT`); call `sys_heartbeat` periodically inside the spin so a long verify doesn't
  trip the watchdog. Keep behavior identical on the fast path.
- Expiry-skip-on-pin: only when `tls-pin-skip-expiry` feature is set (separate opt-in). Default: always
  enforce expiry against clamped time. (No pin implemented yet → this is just the clock plumbing honoring the flag.)

**Non-functional**
- `#![forbid(unsafe_code)]` holds. No `libs/api` change. `provider.rs` < 200 lines.

## Architecture
```
handshake(handle, hostname)
  ├─ tls-insecure  → UnsecureProvider (banner) ── accept-all
  └─ default       → ViTlsProvider{ ViRng, pki::CertVerifier(ca_cert, ViTlsClock, host) }
                       verifier() ≡ Ok(&mut self.verifier)   ← never Err (B3)
                       conn.open(ctx)? ─ verify fail → Err → handlers.rs:432 → cap 0
transport Read/Write: deadline = now()+TLS_IO_TIMEOUT ; heartbeat in loop (M4)
```

## Implementation Steps
1. `provider.rs`: `ViTlsProvider` impl `CryptoProvider` with infallible `verifier()`.
2. Unit test: `provider.verifier().is_ok()` holds unconditionally (guards B3).
3. `socket.rs`: cfg-gated provider swap + empty-host reject + insecure banner + gated import.
4. `transport.rs`: deadline + heartbeat refactor; keep fast-path semantics.
5. `cargo build` default + insecure; boot net cell (default) on QEMU w/ virtio-rng + RTC; confirm no
   regression to TCP/DHCP/plaintext.

## Todo
- [ ] `ViTlsProvider` with **infallible** `verifier()`
- [ ] unit test: `verifier()` always `Ok`
- [ ] socket.rs cfg-gated swap + empty-host reject + INSECURE banner + gated import
- [ ] transport.rs wall-clock deadline + heartbeat (M4)
- [ ] default + insecure build green; net cell boots, no plaintext regression

## Success Criteria
- Default build references `ViTlsProvider`; `UnsecureProvider` absent unless `tls-insecure`.
- `verifier()`-always-`Ok` test passes (B3 guard).
- Net cell boots; long handshake does not trip the RT watchdog or false-timeout (verified in P03 with a real verify).

## Risks
- **R1/B3:** any fallible `verifier()` path = silent MITM. Mitigate: infallible by construction + the unit test; P03 negative gate.
- **R3/M4:** deadline too short still false-rejects on slow TCG. Mitigate: P00 size/speed data informs `TLS_IO_TIMEOUT`; P03 logs distinguish timeout vs reject.
- **Per-handshake state leak:** verifier constructed inside `handshake()` — never a global.

## Security Considerations
- The ONLY accept-unverified path is an explicit `tls-insecure` image, which now also screams at boot.
- Heartbeat additions must not mask a genuinely hung handshake — the deadline still fires.

## Next Steps
P03 proves the wired path: positive connect + every negative rejects, with timeout-vs-reject logging, plus docs.
