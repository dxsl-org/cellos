# Phase 00 — CDI derivation + internal attestation-token library

## Context Links
- Plan: [plan.md](plan.md)
- Dossier: `.agents/260712-1836-mythos-g123-analysis/dossier-4-dice-identity.md` (Decisions 1 + 2)
- Prior art template: `kernel/src/policy.rs:24-33` (VPOL blob shape), `kernel/src/sha256.rs:24` (local SHA-256)

## Overview
- **Priority**: P1 (foundation — every later phase links this)
- **Status**: pending
- **Testability**: G1 / CI, all arches (RISC-V, ARM64, x86) — pure software, no hardware, no Silo.
- Build a `no_std` library crate `libs/attestation` that: (a) derives a DICE CDI ladder
  `CDI_n = HKDF(CDI_{n-1}, H(layer_n))` over a caller-supplied measurement aggregate, and (b)
  serializes/parses **one internal attestation token** in the VPOL-shaped, verify-then-parse layout.
  This is the entire CI-testable slice of the feature.

## Key Insights
- The derivation *math* is arch-independent and needs no root hardware: tests feed synthetic
  aggregates. The real root swaps in at P02 (Decision 1) with **zero call-site change**.
- **Hand-roll HKDF-SHA256** (RFC 5869 extract+expand) over the existing `sha256` implementation —
  do NOT pull the `hkdf` crate. Rationale (YAGNI/KISS + TCB): SHA-256 is already local
  (`kernel/src/sha256.rs`), HKDF is ~30 lines, and the same code must run in kernel-adjacent and
  early-cell contexts where a new dependency is unwanted. `coset`/CBOR is deferred to P06 only.
- **Signature-algorithm nuance (load-bearing)**: the VPOL template carries a *64-byte* sig and the
  kernel verify is **Ed25519** (`kernel/src/ed25519.rs:12`). But the only in-tree signer is Silo,
  which produces **P-256 ECDSA DER** (`silo-guest/src/crypto.rs:83`). Resolution: the token carries a
  **64-byte raw P-256 r‖s** signature (DER→raw is a trivial fixed conversion), keeping the fixed-width
  VPOL shape, and is verified in **userspace** with the `p256` `VerifyingKey`, NOT via the kernel
  Ed25519 path. P00 defines the byte layout + a pure-Rust verify helper; it does not sign (P02/P03 do).

## Requirements
- Functional:
  - `derive_cdi(prev: &[u8;32], layer_hash: &[u8;32]) -> [u8;32]` = HKDF-SHA256(salt=prev, ikm=layer_hash).
  - `AttestToken` encode: `magic("ATT1") ‖ version(u8) ‖ body ‖ sig(64)`; body = `node_id(32) ‖
    measurement_aggregate(32) ‖ alias_pubkey(65) ‖ nonce(16)`.
  - `parse_and_verify(blob, verifying_key) -> Result<TokenView, AttestError>` — **verify-then-parse,
    fail-closed, panic-free** (mirror `policy.rs:103-128,137-180` bounds-checking discipline).
  - HKDF-SHA256 (extract + expand) as a standalone tested unit with RFC 5869 test vectors.
- Non-functional: `#![forbid(unsafe_code)]` (Law 4); `no_std`; host-buildable so `cargo test` on the
  dev host exercises it (matches the `self_test()` pattern in `policy.rs:186`).

## Architecture
Data flow (this phase): `aggregate:[u8;32]` + `root:[u8;32]` → `derive_cdi` chain → `CDI_final` →
(later phases turn `CDI_final` into an alias key) → `AttestToken::encode(body, sign_fn)` → blob.
Verify path: `blob` → length check → P-256 verify over `blob[..len-64]` → parse body fields. Parser
never runs on unverified bytes (VPOL invariant, `policy.rs:11-12`).

## Related Code Files
- **Create**: `libs/attestation/Cargo.toml`, `libs/attestation/src/lib.rs`,
  `libs/attestation/src/hkdf.rs`, `libs/attestation/src/token.rs`, `libs/attestation/src/cdi.rs`.
- **Reference (do not modify)**: `kernel/src/sha256.rs:24`, `kernel/src/policy.rs:137-180`,
  `libs/types/src/silo.rs` (SEC1 65-byte pubkey shape).
- **Modify**: workspace `Cargo.toml` members list (add `libs/attestation`).

## Implementation Steps
1. Scaffold `libs/attestation` `no_std` crate; vendor or re-export a `sha256` fn (share the kernel
   impl by extracting it to the crate, or duplicate the single file — decide in P00 to avoid a
   kernel→lib dependency cycle; duplication of one audited file is acceptable per KISS).
2. Implement `hkdf.rs` (extract + expand) with RFC 5869 vectors A.1–A.3 as `#[test]`.
3. Implement `cdi.rs::derive_cdi` + a `derive_chain(root, &[layer_hashes]) -> CDI_final` helper.
4. Implement `token.rs` encode/`parse_and_verify` with the byte layout above; add a `p256`
   raw-r‖s verify helper (userspace only).
5. Add `self_test()`-style positive + tamper-negative vectors (flip one body byte → verify fails),
   modeled on `policy.rs:186-243`.

## Todo List
- [ ] `libs/attestation` crate scaffolded + in workspace
- [ ] HKDF-SHA256 passes RFC 5869 vectors
- [ ] `derive_cdi` / `derive_chain` implemented + tested
- [ ] `AttestToken` encode/parse_and_verify (verify-then-parse, panic-free)
- [ ] P-256 raw-sig verify helper + tamper-negative test
- [ ] `cargo test -p attestation` green on host

## Success Criteria
- `cargo test -p attestation` passes on host AND the crate `cargo build`s for
  `riscv64gc-unknown-none-elf`, `aarch64-unknown-none`, and the x86 bare target.
- A tampered token blob is rejected; a truncated blob returns `AttestError`, never panics.
- HKDF matches published RFC 5869 outputs byte-for-byte.

## Risk Assessment
- **Sig-algorithm mismatch (High × High → mitigated)**: documented above; the fixed decision is raw
  r‖s P-256 + userspace verify. Do not attempt to route the token through kernel Ed25519 verify.
- **SHA-256 duplication drift (Low)**: if the file is copied, add a comment cross-linking
  `kernel/src/sha256.rs` so a future FIPS fix touches both; or extract to the shared crate.

## Security Considerations
- Token verify is fail-closed and panic-free (boot/enrollment path). No secret is present in the
  token (aggregate = hash of public ELF bytes; pubkey is public) → safe to log/transmit.
- CDI values ARE secret and must never be serialized into the token — only the *derived pubkey*
  appears. Assert this in review.

## Next Steps
- P01 exposes the real kernel aggregate so the token binds actual boot state.
- P02 supplies the real root and the Silo signer; P00's `sign_fn` seam accepts it unchanged.
