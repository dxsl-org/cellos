# Phase 02 — `CellosRustlsProvider` (RustCrypto CryptoProvider)

## Overview
- **Priority:** P1 · **Tier:** thinking · **Status:** Planned
- **Phụ thuộc:** Phase 01 PASS
- Tạo crate `libs/cellos-rustls-provider/` implement `rustls::crypto::CryptoProvider` hoàn toàn bằng RustCrypto crates (đã có trong cây).
- Crate này **host-testable** (`x86_64-unknown-linux-gnu`) — tránh no_std lang-item trap của ostd.
- Provider cover TLS 1.3 cipher suites đủ cho Cellos: `TLS_AES_128_GCM_SHA256` + `TLS_CHACHA20_POLY1305_SHA256`, KX group `X25519`, signing key ECDSA P-256.

## Context Links
- rustls `CryptoProvider` trait: `docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html`
- rustls-rustcrypto (template reference, NOT dep): `github.com/RustCrypto/rustls-rustcrypto`
- Existing RustCrypto deps in cây: `p256`, `rand_chacha`, `sha2`, `hmac` (qua net cell)
- ostd không host-testable vì lang items: `project-ostd-http-json-plan.md` → đây là lý do crate tách riêng

## Why Hand-Roll (không dùng `rustls-rustcrypto`)
`rustls-rustcrypto` hiện gắn nhãn **"DO NOT USE IN PRODUCTION"** và `std` vẫn trong default features (no_std là "foundational only"). Hand-roll từ cùng RustCrypto crates cho phép:
1. Kiểm soát + audit hoàn toàn — security posture của Cellos
2. Không phụ thuộc crate ngoài đang churn
3. Chỉ impl đúng những gì cần (2 cipher suites, 1 KX group, 1 signing scheme)
4. Host-testable với unit tests thực sự

## Architecture

```
libs/cellos-rustls-provider/
├── Cargo.toml          # no_std + alloc; deps: p256, aes-gcm, chacha20poly1305, x25519-dalek, sha2, hmac, rustls
├── src/
│   ├── lib.rs          # pub use CellosServerCryptoProvider; provider()
│   ├── cipher.rs       # TLS_AES_128_GCM_SHA256 + TLS_CHACHA20_POLY1305_SHA256
│   ├── kx.rs           # X25519 key exchange group
│   ├── sign.rs         # ECDSA P-256 SigningKey + Signer; SiloSigningKey stub (G2)
│   ├── hash.rs         # SHA-256 MessageDigest
│   └── tests.rs        # host-testable unit tests (cfg(test))
```

## rustls CryptoProvider là STRUCT (không phải trait)

`rustls::crypto::CryptoProvider` là một **struct** với các field sau (không phải trait):
```rust
pub struct CryptoProvider {
    pub cipher_suites: Vec<SupportedCipherSuite>,
    pub kx_groups: Vec<&'static dyn SupportedKxGroup>,
    pub signature_verification_algorithms: WebPkiSupportedAlgorithms,
    pub secure_random: &'static dyn SecureRandom,
    pub key_provider: &'static dyn KeyProvider,
}
```

`SigningKey` + `Signer` cho server leaf cert **KHÔNG nằm trong CryptoProvider** — chúng được truyền qua `ServerConfig::builder_with_provider(...).with_single_cert(chain, key)` khi build config. Đây là integration point riêng.

## rustls Trait/Struct Map

| rustls surface | Impl trong crate | Dùng crate |
|---|---|---|
| `CryptoProvider` struct | `CellosServerCryptoProvider::provider()` → build struct với fields | (assembler) |
| `SecureRandom` trait | `CellosSecureRandom` wrapping `ViRng` | `ViRng` (VirtIO-RNG) |
| `SupportedCipherSuite` | `CELLOS_TLS13_AES128GCM_SHA256`, `CELLOS_TLS13_CHACHA20_POLY1305_SHA256` | `aes-gcm`, `chacha20poly1305` |
| `Tls13CipherSuite` (per-suite key schedule) | Impl HKDF-SHA256, HMAC-SHA256 bên trong mỗi suite | `hmac`, `sha2`, `hkdf` |
| `SupportedKxGroup` trait | `CellosX25519` | `x25519-dalek` |
| `KeyProvider` trait | `CellosKeyProvider::load_private_key()` | `p256`, `pkcs8` |
| `SigningKey` trait | `CellosEcdsaSigningKey` (load từ PKCS#8 DER) | `p256`, `ecdsa` |
| `Signer` trait | `CellosEcdsaSigner` | `p256::ecdsa::SigningKey` |
| `WebPkiSupportedAlgorithms` | wired ECDSA P-256 + (optional RSA) | rustls-webpki hoặc custom |

**Note về `ViRng`**: `ViRng` implements `rand_core::CryptoRng`, **KHÔNG** implements `rustls::crypto::SecureRandom`. Cần wrapper:
```rust
struct CellosSecureRandom;
impl rustls::crypto::SecureRandom for CellosSecureRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<(), GetRandomFailed> {
        // ViRng::new().fill_bytes(buf) — hoặc call sys_get_random trực tiếp
        sys_get_random(buf).map_err(|_| GetRandomFailed)
    }
}
```

**Note về key schedule (HKDF/HMAC trong cipher suites)**: Mỗi `Tls13CipherSuite` phải implement `KeySchedule` (HKDF + HMAC-SHA256 cho Finished message). Đây là phần việc TRONG cipher.rs, không phải ở CryptoProvider level. Budget thêm ~100 LOC cho phần này so với ước tính 400 LOC ban đầu.

## Cargo.toml (crate mới)

```toml
[package]
name = "cellos-rustls-provider"
version = "0.1.0"
edition = "2021"

[dependencies]
rustls = { version = "0.23", default-features = false, features = ["alloc", "hashbrown"] }
p256 = { version = "0.13", default-features = false, features = ["alloc", "ecdsa", "pkcs8"] }
x25519-dalek = { version = "2", default-features = false, features = ["alloc", "static_secrets"] }
aes-gcm = { version = "0.10", default-features = false, features = ["alloc", "aes"] }
chacha20poly1305 = { version = "0.10", default-features = false, features = ["alloc"] }
sha2 = { version = "0.10", default-features = false }
rand_core = { version = "0.6", default-features = false }

[features]
default = []
std = ["rustls/std"]

[dev-dependencies]
# host tests only — std OK here
```

## Key Implementation Notes

### sign.rs — `CellosEcdsaSigningKey` (wired qua `ServerConfig`, KHÔNG qua CryptoProvider)

```rust
// Dùng: PrivateKeyDer::Pkcs8(key_der) → ServerConfig.with_single_cert()
// Phải impl rustls::sign::SigningKey để KeyProvider::load_private_key() trả về

pub struct CellosEcdsaSigningKey {
    key: p256::ecdsa::SigningKey,
}

impl CellosEcdsaSigningKey {
    // Gọi từ Phase 04 khi build ServerConfig
    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self, ()> {
        p256::ecdsa::SigningKey::from_pkcs8_der(der)
            .map(|key| Self { key })
            .map_err(|_| ())
    }
}

impl rustls::sign::SigningKey for CellosEcdsaSigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered.contains(&SignatureScheme::ECDSA_NISTP256_SHA256)
            .then(|| Box::new(CellosEcdsaSigner { key: self.key.clone() }) as Box<dyn Signer>)
    }
    fn algorithm(&self) -> SignatureAlgorithm { SignatureAlgorithm::ECDSA }
}

// `CellosKeyProvider` (impl KeyProvider trait) trả về CellosEcdsaSigningKey khi
// rustls gọi load_private_key() với PrivateKeyDer::Pkcs8 DER bytes
```

### kx.rs — `CellosX25519`
```rust
// X25519 ECDHE: generate ephemeral keypair, return public key bytes
// Complete exchange: combine with peer's public key → shared secret → HKDF
// x25519-dalek: EphemeralSecret + PublicKey
```

### cipher.rs — cipher suite wiring
rustls `SupportedCipherSuite` wraps:
- `AeadAlgorithm` (aes-gcm / chacha20poly1305)
- `HashAlgorithm` (sha2)
- `TLS13` marker (TLS 1.2 support NOT needed — TLS 1.3 only matches embedded-tls client)

### G2 extension point: `SiloSigningKey`
```rust
// G2: thay CellosEcdsaSigningKey bằng SiloSigningKey
// rustls sign() trait là blocking — OK vì net cell synchronous
// SiloHandle::sign(&sha256_digest) → DER ECDSA sig (max 72B)
// Existing Silo IPC protocol đã compatible
// ĐÂY CHỈ LÀ STUB trong plan này — KHÔNG implement G2 path
pub struct SiloSigningKey; // placeholder, unimplemented!()
```

## Implementation Steps

1. Tạo `libs/cellos-rustls-provider/` crate (Cargo.toml + src/)
2. Add to workspace `Cargo.toml` members
3. `sign.rs`: `CellosEcdsaSigningKey::from_pkcs8_der()` → `SigningKey` + `Signer` impl
4. `kx.rs`: `CellosX25519` KX group với x25519-dalek ephemeral keypair
5. `cipher.rs`: 2 cipher suites (AES-128-GCM + ChaCha20-Poly1305)
6. `hash.rs`: SHA-256 MessageDigest
7. `lib.rs`: `CellosServerCryptoProvider` struct + `provider()` fn → `Arc<CryptoProvider>`
8. `tests.rs` (cfg(test)): verify `from_pkcs8_der` với known P-256 DER; round-trip sign+verify; `cargo test` chạy trên host
9. Verify `cargo check --target riscv64gc-unknown-none-elf` từ crate này

## Todo
- [ ] Tạo crate skeleton + workspace wiring
- [ ] sign.rs: ECDSA P-256 SigningKey + Signer
- [ ] kx.rs: X25519 group
- [ ] cipher.rs: 2 TLS 1.3 cipher suites
- [ ] hash.rs: SHA-256
- [ ] lib.rs: CellosServerCryptoProvider::provider()
- [ ] tests.rs: host unit tests (sign/verify round-trip)
- [ ] cargo check no_std target xanh

## Success Criteria
- `cargo test` (host) PASS — sign/verify round-trip, cipher suite construction
- `cargo check --target riscv64gc-unknown-none-elf` PASS — no std leakage
- `CellosServerCryptoProvider::provider()` trả về `Arc<CryptoProvider>` có thể truyền vào `ServerConfig::builder_with_details()`
- Không có `unsafe` trong crate này (pure safe Rust)

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| rustls cipher suite trait API thay đổi giữa 0.23.x | M×M | Pin exact version; đọc CHANGES.md trước khi bump |
| x25519-dalek ephemeral API misuse (reuse secret) | M×H | EphemeralSecret consumed by design; không clone key |
| p256::ecdsa::SigningKey không compile no_std | L×H | p256 native no_std+alloc; đã được test trong Cellos Silo path |
