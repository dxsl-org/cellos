# Spike-00 Findings — `embedded_tls::pki::CertVerifier` (G14)

Executed 2026-06-21. Throwaway spike on `cells/services/net`; fully reverted (baseline 436,312 B restored).
Target: `riscv64gc-unknown-none-elf` release (`lto=true, opt-level=z`).

## Verdict: GO with `pki.rs` for the embedded build. API locked. Size budget re-derived.

## 1. Compile — PASS
`embedded_tls::pki::CertVerifier` compiles inside the net cell (no_std + alloc, `#![forbid(unsafe_code)]`).
Only warnings were unused imports (expected during the swap). No unsafe required in the cell.

## 2. API locked (copy-paste ready for P02)
Public paths (via `pub use crate::config::*` re-exported through `asynch`):
```rust
use embedded_tls::pki::CertVerifier;
use embedded_tls::{Aes128GcmSha256, Certificate, CryptoProvider, TlsClock, TlsError, TlsVerifier, CryptoRngCore};
```
- `CertVerifier<'a, CipherSuite, Clock, const CERT_SIZE: usize>` where `Clock: TlsClock`.
- `CertVerifier::new(ca: Certificate<&'a [u8]>)` — **single anchor**. `Certificate::X509(&[u8])`.
- `TlsClock` is a **static** trait: `fn now() -> Option<u64>` (Unix epoch **seconds**). Our impl:
  `Some(ostd::syscall::sys_get_wall_secs().max(VICELL_MIN_UNIX))`.
- `CryptoProvider` requires `type CipherSuite`, **`type Signature: AsRef<[u8]>`** (use `[u8; 64]` — unused
  in server-auth), `fn rng(&mut self) -> impl CryptoRngCore` (`&mut ViRng`), and an **overridden**
  `fn verifier(&mut self) -> Result<&mut impl TlsVerifier<…>, TlsError>` that returns
  **`Ok(&mut self.verifier)` unconditionally** (B3 guard — `connection.rs:455` skips verify on `Err`).
  `signer()`/`client_cert()` keep their defaults (no client-cert/mTLS).
- `CERT_SIZE = 4096` worked (heapless buffer for the leaf cert held during handshake).
- `sys_get_wall_secs()` already exists at `libs/ostd/src/syscall.rs:949` (GetTime op=3). No ostd change.

## 3. Size budget — measured (replaces the guessed ≤20 KB)
| embedded-tls features | algorithms | binary | Δ vs 436,312 B |
|---|---|---|---|
| `["alloc"]` (baseline, UnsecureProvider) | none | 436,312 | — |
| `["alloc","rustpki"]` | **ECDSA P-256** | 457,160 | **+20.8 KB** ✅ |
| `["alloc","p384"]` | ECDSA P-256 + P-384 | 536,128 | +99.8 KB |
| `["alloc","rsa"]` | ECDSA P-256 + RSA | 571,464 | +135.2 KB |
| `["alloc","ed25519","p384","rsa"]` | all | 661,096 | +224.8 KB |

**The "small embedded" promise holds ONLY for ECDSA P-256 CAs** (+21 KB — p256 is already linked for the
TLS key exchange, so verification reuses it). RSA (~+135 KB) and P-384 (~+100 KB) are expensive.

## 4. Decisions this locks for P01-P03
- **Default `tls-ca-private` should be ECDSA P-256** (+21 KB) — keeps the embedded image lean.
- For public-CA selectors, **prefer the ECDSA root over the RSA one** to stay lean:
  - `tls-ca-amazon` → **Amazon Root CA 3** (ECDSA P-256, `rustpki` only, +21 KB) rather than Root CA 1 (RSA, +135 KB).
  - `tls-ca-letsencrypt` → **ISRG Root X2** (ECDSA P-384, `rustpki`+`p384`, +100 KB) rather than X1 (RSA-4096, +135 KB).
  - Offer the RSA roots only as explicit heavyweight opt-ins (documented cost).
- Feature mapping (P01 Cargo.toml): `tls-ca-private`/`tls-ca-amazon` → `embedded-tls/rustpki`;
  `tls-ca-letsencrypt` → `embedded-tls/p384`; RSA opt-ins → `embedded-tls/rsa`.

## 5. Deferred / not provable in a spike
- **Functional Ok/Err proof** (real chain accepted, bad cert rejected): `pki.rs` parses no_std handshake
  types, not trivially unit-testable on the host → proven in **P03 e2e** (QEMU, real + negative certs).
- **SAN/host caps** (`MAX_SAN_DNS_NAMES=3`, `HOSTNAME_MAXLEN=64`, source-confirmed): P03 must test against
  the **actual** broker cert; if it has >3 SAN dNSNames or a >64-char name, that endpoint needs the P04 full build.
- **No dep conflict:** `rustpki` pulls `der`/`digest`/`heapless` (already in tree via embedded-tls); does
  NOT pull `webpki`(0.101). Confirmed by clean build.

## 6. Residual risk unchanged
B3 silent-bypass (`connection.rs:455`) remains the #1 thing to guard — the infallible `verifier()` +
P03 negative gate are the mitigations.
