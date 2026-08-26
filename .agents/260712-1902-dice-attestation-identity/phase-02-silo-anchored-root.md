# Phase 02 — Silo-anchored root CDI (with software-seed fallback)

## Context Links
- Plan: [plan.md](plan.md)
- Dossier Decision 1: root secret = `SiloHandle`-provided where Silo exists; dev software-seed else.
- Silo API: `libs/ostd/src/silo.rs:64` (`connect`), `:130` (`init_key(seed)->pubkey`), `:159` (`sign`)
- Silo protocol types: `libs/types/src/silo.rs:26-51,108`
- Seam precedent: `net-broker/transport.rs:53-74` (`ClusterKeySource` — root swaps, no call-site change)

## Overview
- **Priority**: P1
- **Status**: pending
- **Testability**: G2 / hardware-informed. **CI stays green via the software-seed fallback** (no Silo
  backend in CI). Do NOT hard-anchor to Silo in a way that blocks CI (Decision 1, dossier:48-49).
- **🔶 HARDWARE-INFORMED — do not over-spec the Silo-anchored path before Silo maturity** (roadmap rule,
  dossier:99-106). Specify the seam + fallback now; leave board-specific UDS provisioning to hardware.

## Key Insights
- **The clean closure**: `CDI_final` (32 bytes, from P00 over the P01 aggregate) is passed straight to
  `SiloHandle::init_key(&CDI_final)` → the returned P-256 pubkey **is** the attested Alias key. No new
  crypto; the existing Silo `Init` path (`silo.rs:130`, guest `crypto.rs:55-69`) does the work.
- **Root selection mirrors `ClusterKeySource`**: define `trait RootSecretSource { fn root() ->
    [u8;32] }` with two impls — `SiloRootSource` (G2 ARM64/x86, seals/derives inside the fence) and
    `DevSeedRootSource` (fixed dev seed, CI + arches without Silo). Same code path; root swaps.
- Silo currently exposes Init/Sign/Ecdh/GetPub only (`silo.rs:26-35`). Deriving CDI *inside* the fence
  would need a new Silo command — **out of scope here**; P02 derives CDI in the producer cell and uses
  Silo only as the seeded signer. Sealing the root inside Silo is P03 (KMS) territory.

## Requirements
- Functional:
  - `RootSecretSource` trait + `SiloRootSource` / `DevSeedRootSource` impls.
  - Producer flow: `root = source.root()` → `CDI_final = attestation::derive_chain(root, layers)` →
    `alias_pub = silo.init_key(&CDI_final)?` → sign token body with `silo.sign(digest)`.
  - DER→raw r‖s conversion for the 64-byte token sig (P00 layout).
- Non-functional: fallback path fully exercised in CI; Silo path behind `#[cfg(feature="silo")]` or a
  runtime `SiloHandle::connect()` probe that degrades to `DevSeedRootSource` on `ServiceNotFound`.

## Architecture
`P01 aggregate` + `RootSecretSource::root()` → P00 `derive_chain` → `CDI_final` →
`SiloHandle::init_key(CDI_final)` → `alias_pub` → token body → `SiloHandle::sign(SHA256(body))` →
DER→raw → 64-byte sig → P00 `AttestToken` blob. On no-Silo arch: identical flow, `DevSeedRootSource`
root + a software P-256 signer (the `p256` crate, userspace) instead of Silo.

## Related Code Files
- **Create**: `cells/services/attestation/` producer module OR a `libs/ostd` helper
  `ostd::attest` that wires RootSecretSource + Silo. (Home decided with P03 — likely the KMS cell.)
- **Reference**: `libs/ostd/src/silo.rs`, `cells/guests/silo-guest/src/crypto.rs:77-100`,
  `libs/attestation` (P00).
- **Modify**: none in `libs/api`/`libs/types` (no Law 1 here).

## Implementation Steps
1. Define `RootSecretSource` + the two impls (fallback fixed seed clearly marked dev-only, like
   `policy.rs:38-43` DEV key discipline).
2. Wire the producer flow end-to-end using the software P-256 signer first (CI-testable).
3. Add the Silo path behind a connect-probe; on `SiloError::ServiceNotFound` fall back.
4. Implement DER→raw r‖s and assert round-trip against a `p256` verify.
5. Golden-vector test: fixed dev root + fixed synthetic aggregate → fixed `CDI_final` → fixed
   alias pubkey → token verifies.

## Todo List
- [ ] `RootSecretSource` trait + Silo/DevSeed impls
- [ ] End-to-end producer via software signer (CI green, no Silo)
- [ ] Silo path behind connect-probe with graceful fallback
- [ ] DER→raw r‖s conversion + verify round-trip
- [ ] Golden-vector determinism test

## Success Criteria
- CI (no Silo): deterministic token produced from dev root + synthetic aggregate; verifies.
- On QEMU ARM64 with Silo up: token produced with Silo-held Alias key; `alias_pub` matches
  `SiloHandle::get_public_key()`; token verifies.
- Fallback triggers cleanly when `connect()` returns `ServiceNotFound` (no panic, logged).

## Risk Assessment
- **CI-blocking hard-anchor (High × High → mitigated)**: the fallback is mandatory and tested first;
  the Silo path is additive. Explicitly forbidden to make CI depend on a Silo backend.
- **Dev seed shipped to prod (Med × High)**: mitigation — mark dev seed with the same `#[cfg]`/comment
  discipline as `DEV_FLEET_PUBKEY` (`policy.rs:38-43,51-54`); production provisioning replaces it.
- **Silo single-key reuse (Med)**: `init_key` is once-only (`silo.rs:130` contract); if TLS/KMS also
  seed Silo there is a collision. Mitigation — P03 owns Silo key lifecycle; attestation consumes via
  KMS, not a second `init_key`. Flag for P03.

## Security Considerations
- `CDI_final` is a secret seed; it exists only transiently in the producer before `init_key` consumes
  it — zero it after (mirror guest `crypto.rs:59` seed-zeroing). Never log it, never put it in a token.
- The Silo private key never leaves the Stage-2 fence (`silo.rs:6-8`) — the Alias key inherits that.

## Next Steps
- P03 wraps this in the KMS Cell and owns the Silo key lifecycle (resolves the single-key-reuse risk).
- P05 consumes the produced token as the K3 enrollment payload.
