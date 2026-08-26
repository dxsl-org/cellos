# Phase 06 — Veraison / COSE_Sign1 EAT interop adapter (DEFERRED)

## Context Links
- Plan: [plan.md](plan.md)
- Dossier Decision 2 (dossier:51-69): COSE_Sign1/CBOR/EAT (`coset` crate) is a **G2 interop adapter
  layered on top** when a third-party verifier (ARM Veraison, RFC 9711 EAT) actually consumes the
  token — NOT the native format. Keeps CBOR out of the kernel/early-cell TCB (YAGNI).
- Native token: `libs/attestation` `AttestToken` (P00).

## Overview
- **Priority**: P3
- **Status**: pending — **DEFERRED. Do not build until a third-party verifier is being wired.**
- **Testability**: deferred / interop-driven.
- **🔶 ROADMAP RULE — do not over-spec before the verifier exists** (dossier:99-106). This phase is a
  placeholder that fixes the *shape* of the future adapter so P00-P05 don't accidentally couple to CBOR.
- Encode the P00 attestation token as a COSE_Sign1 EAT (RFC 9711) so an external RATS verifier
  (Veraison) can consume it, without changing the native internal format.

## Key Insights
- **Adapter, not a rewrite** — the internal VPOL-shaped token (P00) stays the first-party format; this
  phase adds an *encoder* `AttestToken → COSE_Sign1(EAT claims)` and, if needed, a decoder for
  verifier-issued results. The kernel and early cells never touch CBOR.
- **`coset` crate** is the pick (dossier:26,62) — must build `no_std` in a Cell; verify like the
  clatter no_std spike discipline (`.agents/260623-0907-net-broker-robot-swarm/phase-00-derisk-spikes.md:49-59`)
  before committing (no `std`/`getrandom`/`cpufeatures` leakage).
- EAT claim mapping is the real work: measurement aggregate → `measurements`/`dbgstat`, alias pubkey →
  `cnf`/instance key, nonce → `eat_nonce`. This mapping is Veraison-endpoint-specific → cannot be
  finalized before the endpoint is chosen.

## Requirements (provisional — finalize at build time)
- Functional: `to_cose_sign1(token, signer) -> Vec<u8>` producing an EAT the target Veraison profile
  accepts; the same Silo/KMS signer (P02/P03) signs (P-256 → COSE alg ES256).
- Non-functional: adapter lives in a *userspace* cell only (never kernel/early-boot); `coset` behind a
  feature flag so no-interop builds don't pull CBOR.

## Architecture
`P05 AttestToken` → `to_cose_sign1` (map fields → EAT claims, sign ES256 via KMS/Silo) → CBOR bytes →
external Veraison verifier. Internal fleet enrollment (P05) continues to use the native token; COSE is
emitted only on the external-attestation path.

## Related Code Files
- **Create (deferred)**: `cells/services/attestation/src/cose_adapter.rs` (feature-gated `coset`).
- **Reference**: `libs/attestation` (P00), Veraison endpoint profile docs (external).
- **No Law 1** (userspace encoder only).

## Implementation Steps (deferred — execute only when Veraison is being wired)
1. Confirm the target Veraison profile + required EAT claims (blocks all mapping work).
2. `no_std` build-spike `coset` in a scratch cell (leakage check) before adding to a real cell.
3. Implement the claim mapping + ES256 signing via KMS/Silo.
4. Round-trip against a real Veraison instance (or its test vectors).

## Todo List (deferred)
- [ ] Target Veraison profile + claim set confirmed
- [ ] `coset` no_std build-spike passes leakage check
- [ ] `to_cose_sign1` claim mapping + ES256 sign
- [ ] Round-trip vs Veraison test vectors

## Success Criteria
- An external Veraison instance accepts the emitted COSE_Sign1 EAT — measured only when the endpoint
  exists. Until then this phase stays unstarted.

## Risk Assessment
- **Premature build (Med × Med → mitigated)**: explicitly deferred; the placeholder exists so earlier
  phases keep the native token decoupled from CBOR.
- **`coset` no_std viability (Med)**: mitigation — mandatory build-spike before adoption (clatter
  precedent).

## Security Considerations
- The external path exposes the same non-secret claims (aggregate, pubkey, nonce); no new secret
  crosses the boundary. Signing stays inside Silo/KMS (ES256), key never leaves the fence.

## Next Steps
- None until a third-party verifier is scheduled. Revisit when the roadmap wires ARM Veraison.
