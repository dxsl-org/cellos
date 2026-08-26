# Scout Report — G14 TLS Server-Auth / Certificate Chain Verification

> Codebase analysis written at the Codebase Analysis stage. Read by `/hc-cook`, `/hc-review`,
> `/hc-debug` to skip re-scouting within this plan.

## The gap (one line)

[`cells/services/net/src/tls/socket.rs:56`](../../cells/services/net/src/tls/socket.rs#L56) uses
`UnsecureProvider::new::<Aes128GcmSha256>(rng)` → **the server certificate is accepted blindly.
Every TLS connection in ViCell is trivially MITM-able.** G14 replaces this with a verifying provider.

## Current TLS architecture (all in the net service cell)

| File | Role | Touch in G14? |
|------|------|---------------|
| `cells/services/net/src/tls.rs` | module root (`rng`/`transport`/`socket`/`block_on`) | add `verify`, `roots`, `clock` |
| `cells/services/net/src/tls/socket.rs` | `TlsSocketEntry::handshake()` — builds `TlsContext`, calls `conn.open(ctx)` | **YES — swap provider** |
| `cells/services/net/src/tls/transport.rs` | smoltcp↔embedded-io bridge (spin-poll) | no |
| `cells/services/net/src/tls/rng.rs` | `ViRng` (VirtIO-RNG ChaCha20) | no |
| `cells/services/net/src/handlers.rs:377-433` | `TLS_CONNECT_OP` (0x30) dispatch; handshake `Err` → reply `[0u8;8]` (cap 0) | verify failure already propagates here |
| `cells/services/net/Cargo.toml:10` | `embedded-tls 0.19 default-features=false features=["alloc"]` | **YES — features + deps** |
| `libs/ostd/src/tls.rs` | client helpers `tls_connect/write/read/close` (opcodes 0x30-0x32) | no (wire format unchanged) |

**Key fact:** verification failure plumbing already exists. `conn.open(ctx)?` returns `TlsError` →
`handshake()` returns `Err` → [handlers.rs:432](../../cells/services/net/src/handlers.rs#L432) replies cap 0.
We only change *which provider* `open()` runs.

## Capability/clock prerequisites — already shipped

- Entropy: `sys_get_random = 214` via `ViRng` ([rng.rs](../../cells/services/net/src/tls/rng.rs)). ✅
- Wall clock: `GetTime = 120` op=3 (epoch_secs) from RTC (Goldfish/PL031/CMOS) — see memory
  `project-rtc-wall-clock`. Needed for cert `notBefore`/`notAfter`. ✅
- `transport.rs:71` already calls `sys_get_time()` for smoltcp `Instant`.

## Law 1 (interface) — NOT triggered

TLS opcodes (0x30-0x32) live in the net cell (`handlers.rs`), not `libs/api`. No new syscall: 214 + 120
already exist. Build-time bundle selection is a cargo feature, not an ABI change. **No `libs/api` edit → no 2× confirm.**
(Confirmed against original TLS plan note `.agents/260607-1109-tls/plan.md:46`.)

## Build / packaging surface

- Net cell binary is built then placed on disk (gen_disk.ps1 / run scripts). The "two builds" the user wants
  (small embedded vs full server/PC) map to **mutually-exclusive cargo features** on the net cell, selected at
  net-cell build time when the OS image is assembled.
- `#![forbid(unsafe_code)]` holds for the cell: rustls-webpki / embedded-tls expose only safe APIs; their
  internal unsafe does not leak (Rust Reference — `forbid` is per-crate).

## Research verdicts feeding the design (full reports in `research/`)

1. **embedded-tls 0.19 supports custom verification.** `CryptoProvider::verifier()` (default `Err(Unimplemented)`)
   + `TlsVerifier` trait whose `verify_certificate(transcript, cert)` gets the **full DER chain** and
   `verify_signature(CertificateVerifyRef)` gets the sig — signed data reconstructed from the cached
   transcript (`[0x20;64] ++ "TLS 1.3, server CertificateVerify\0" ++ transcript.finalize()`, RFC 8446 §4.4.3).
   **CORRECTION (red-team-01, verified in source):** there are TWO built-in verifiers — `webpki.rs`
   (`webpki` feature, rustls-webpki 0.101) is **leaf-only** (`// TODO: Support intermediates`, `entries[0]`) →
   unusable; **`pki.rs`** (`rustpki` feature, pulled by `rsa`/`ed25519`/`p384`) is a **complete no_std+alloc**
   verifier that walks leaf+intermediates to a **single** CA (`pki.rs:119`), checks validity + RFC-6125
   hostname (capped 3 SANs / 64-char names, `der_certificate.rs:11`), and does §4.4.3 sig check (`pki.rs:136`).
   **`pki.rs` is the embedded-build engine** — researcher #1 missed it. ⚠️ `connection.rs:455` SKIPS
   verification if `verifier()` returns `Err` → provider's `verifier()` MUST be infallible.
2. **rustls-webpki 0.103 + rustls-ring** is the recommended chain engine: no_std+alloc, no unsafe leak, full
   RFC 5280 path build + sig + validity + SAN. `verify_for_usage(algs, anchors, intermediates, time, server_auth, None, None)`
   then `verify_is_valid_for_subject_name(&ServerName)`. `UnixTime::since_unix_epoch(Duration::from_secs(..))` (no std).
3. **Trust anchors:** curated ~6-8 DER roots (~10-12 KB) for embedded; `webpki-roots 0.26 TLS_SERVER_ROOTS`
   (~148, ~230 KB) for full. Hostname per RFC 6125 (SAN dNSName, wildcard = exactly one label, no CN fallback).
   Revocation (OCSP/CRL) **scoped out** — industry standard for this device class (AWS/Azure IoT do the same);
   mitigate via short-lived certs + pinning + OTA. Clock-unset → build-time min-date clamp.
