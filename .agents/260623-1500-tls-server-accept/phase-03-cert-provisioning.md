# Phase 03 — Server cert provisioning (rcgen + disk image)

## Overview
- **Priority:** P1 · **Tier:** medium · **Status:** Planned
- **Phụ thuộc:** Phase 01 PASS (chạy song song với P02)
- Sinh CA cert + leaf server cert bằng `rcgen` (host tool, std OK), embed DER vào net cell binary qua `include_bytes!()`.
- Thiết lập trust chain: CA cert thay thế `roots/private.der` hiện tại → client (embedded-tls) tin tưởng server cert.

## Context Links
- rcgen: `github.com/rustls/rcgen` · `docs.rs/rcgen/latest/rcgen`
- Existing CA pattern: `cells/services/net/roots/private.der` (được embed tại compile time)
- Client CA loading: `cells/services/net/src/tls/roots.rs:ca_cert()` — đọc `private.der` qua `include_bytes!()` hoặc feature flag

## ⚠️ CRITICAL: KHÔNG replace `roots/private.der`

`tools/hypha-mock-llm/gen-dev-ca.py` ghi CA cert vào `cells/services/net/roots/private.der`. External mock server (`mock_proxy.py`) dùng cert được ký bởi CA này. `http-smoke` tests HTTPS against external mock → verify qua CA trong `private.der`.

**Nếu replace `private.der` với CA mới → http-smoke external HTTPS test FAIL.**

→ **Giải pháp: ký leaf cert bằng CA đã có.** Load CA key + cert từ file hiện có, dùng rcgen để ký leaf mới.

## Cert Architecture

```
CA cert (đã tồn tại, KHÔNG thay đổi)
  └── cells/services/net/roots/private.der  ← GIỮ NGUYÊN, KHÔNG replace

Leaf server cert (ECDSA P-256, ký bởi CA đã có)
  ├── CN/SAN: "cellos-httpd.local", "localhost", "127.0.0.1", "10.0.2.15"
  ├── Lưu: cells/services/net/certs/server.der
  └── Dùng: rustls ServerConfig cert chain

Server private key (PKCS#8 DER, ECDSA P-256)
  ├── Lưu: cells/services/net/certs/server-key.der
  └── Dùng: CellosEcdsaSigningKey::from_pkcs8_der()
```

**Trust chain:** Existing CA signs new leaf → embedded-tls client (trusts existing CA) verifies new leaf → PASS. External mock cert vẫn valid (CA không đổi).

## Security Note
```
// SECURITY(dev): Server private key được embed plaintext trong net cell binary.
// Acceptable: G1 robot LAN, mạng đóng, không có user data nhạy cảm.
// G2 path: SiloSigningKey — key không bao giờ rời khỏi Silo EL2 enclave.
// KHÔNG deploy disk image có embedded key ra public network.
```

Cả 3 file (`private.der`, `server.der`, `server-key.der`) phải vào `.gitignore` (tương tự key files) — hoặc được regenerate mỗi build và chỉ tồn tại trong build artifacts.

**Thực tế**: vì `roots/private.der` hiện đang được track trong git (nếu là dev CA thì OK), ta theo cùng convention. Quan trọng là **không commit production key**.

## Script: `scripts/gen-server-cert.ps1`

```powershell
# gen-server-cert.ps1 — sinh server leaf cert cho Cellos TLS server
# Ký bằng CA ĐÃ CÓ (cells/services/net/roots/private.der + CA key)
# Chạy: pwsh scripts/gen-server-cert.ps1
# Output: cells/services/net/certs/server.der (leaf cert)
#         cells/services/net/certs/server-key.der (private key PKCS#8)
# Yêu cầu: cargo run scripts/gen-cert/
```

Script dùng một Rust binary nhỏ để gọi rcgen:

```rust
// scripts/gen-cert/src/main.rs (chạy trên host, std available)
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256, CertificateSigningRequestParams};

fn main() {
    // 1. Load CA cert + CA key (đã tồn tại — KHÔNG regenerate)
    let ca_cert_der = std::fs::read("cells/services/net/roots/private.der")
        .expect("CA cert exists at roots/private.der — run gen-dev-ca.py first");
    let ca_key_der = std::fs::read("cells/services/net/roots/private-key.der")
        .expect("CA key exists at roots/private-key.der");

    let ca_kp = KeyPair::from_der(&ca_key_der).expect("valid CA key DER");
    let ca_cert_params = CertificateParams::from_ca_cert_der(&ca_cert_der, ca_kp)
        .expect("valid CA cert DER");
    let ca = ca_cert_params.self_signed(&ca_cert_params.key_pair).unwrap();
    // Note: rcgen re-signs with same key, producing same cert bytes — CA unchanged

    // 2. Sinh server leaf keypair + cert (signed bởi CA)
    let server_kp = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut server_params = CertificateParams::new(vec![
        "cellos-httpd.local".into(),
        "localhost".into(),
    ]).unwrap();
    server_params.subject_alt_names.push(
        rcgen::SanType::IpAddress("10.0.2.15".parse().unwrap())
    );
    server_params.subject_alt_names.push(
        rcgen::SanType::IpAddress("127.0.0.1".parse().unwrap())
    );
    let server_cert = server_params.signed_by(&server_kp, &ca, &ca_kp).unwrap();

    // 3. Ghi ra file — KHÔNG ghi roots/private.der
    std::fs::create_dir_all("cells/services/net/certs").unwrap();
    std::fs::write("cells/services/net/certs/server.der", server_cert.der()).unwrap();
    std::fs::write("cells/services/net/certs/server-key.der", server_kp.serialize_der()).unwrap();

    println!("Server cert: cells/services/net/certs/server.der (signed by existing CA)");
    println!("Server key:  cells/services/net/certs/server-key.der");
    println!("SECURITY(dev): key is plaintext. G2: use SiloSigningKey.");
    // roots/private.der UNCHANGED
}
```

**Note:** Cần CA private key file (`roots/private-key.der`) để ký. Kiểm tra xem `gen-dev-ca.py` có lưu key file không, hoặc cần viết lại CA generation để lưu key. Nếu CA key không có, fallback: generate CA mới + ký leaf + update external mock cert cùng lúc (một bước atomic để không vỡ smoke test).

## Net Cell Integration (compile-time embed)

Trong `cells/services/net/src/tls/server.rs` (Phase 04 tạo):
```rust
const SERVER_CERT_DER: &[u8] = include_bytes!("../../certs/server.der");
const SERVER_KEY_DER: &[u8]  = include_bytes!("../../certs/server-key.der");
```

CA cert đã được embed tại `roots.rs:ca_cert()` → không cần thay đổi.

## Thiết lập gen-cert binary

```
scripts/gen-cert/
├── Cargo.toml    # [package] name = "gen-cert"; [dependencies] rcgen = "0.13"
└── src/main.rs   # code trên
```

**Không** add vào workspace — đây là host tool, không phải cell. Chạy standalone:
```powershell
cargo run --manifest-path scripts/gen-cert/Cargo.toml
```

## Implementation Steps

1. Tạo `scripts/gen-cert/` với Cargo.toml + src/main.rs
2. Chạy script, verify output files tồn tại và hợp lệ:
   - `openssl x509 -in cells/services/net/roots/private.der -inform DER -text -noout`
   - `openssl x509 -in cells/services/net/certs/server.der -inform DER -text -noout`
   - Verify SAN chứa "cellos-httpd.local", "localhost", IPs
   - Verify issuer của server cert = CA cert subject
3. Tạo `cells/services/net/certs/` directory (gitignore `*.der` trong đó nếu policy yêu cầu)
4. Update `.gitignore` nếu cần (key files)
5. Verify `cargo check` của net cell còn xanh sau khi certs exist (include_bytes! sẽ fail nếu file chưa có — Phase 04 add include_bytes!, nên chỉ cần file tồn tại trước P04)

## Todo
- [ ] Tạo scripts/gen-cert/ (Cargo.toml + main.rs)
- [ ] Chạy script, kiểm tra output 3 files
- [ ] Verify cert chain với openssl
- [ ] Verify SAN bao gồm QEMU IP (10.0.2.15) và localhost
- [ ] Ghi SECURITY(dev) comment vào script

## Success Criteria
- `cells/services/net/roots/private.der` là CA cert mới (ECDSA P-256, is_ca=true)
- `cells/services/net/certs/server.der` là leaf cert, signed by CA, SAN có localhost + 10.0.2.15
- `cells/services/net/certs/server-key.der` là PKCS#8 DER ECDSA P-256 key
- `openssl verify -CAfile <(openssl x509 -in roots/private.der -inform DER) -inform DER certs/server.der` PASS
- Script có thể chạy lại để regenerate (idempotent về mặt format, key mới mỗi lần)

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Script ghi đè `roots/private.der` cũ → client tests fail | M×M | Sau khi replace CA, phải rebuild net cell và rerun smoke test với CA mới |
| SAN thiếu IP QEMU (10.0.2.15) → TLS handshake fail | M×H | Script hardcode IP; verify cert sau khi sinh |
| rcgen API thay đổi (v0.12 vs v0.13) | L×M | Pin exact version trong scripts/gen-cert/Cargo.toml |
| Private key vô tình commit vào git | L×H | .gitignore `certs/server-key.der`; SECURITY comment trong script |
