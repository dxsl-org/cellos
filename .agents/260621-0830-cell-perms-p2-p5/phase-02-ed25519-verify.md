---
phase: 02
title: In-kernel Ed25519 signature verification (P5a)
tier: thinking
depends: []
status: done
---

# Phase 02 — In-kernel Ed25519 verify (P5a)

> **✅ SPIKE RESOLVED (2026-06-21): signed policy is VIABLE.** `ed25519-compact`
> (no_std, verify-only, no RNG) builds clean under PIC on riscv64 + aarch64; the
> verify path codegens/links/runs correctly (RFC 8032 TEST 1 + tamper-negative,
> confirmed at boot on both arches via a power-on self-test). Chosen over
> ed25519-dalek to minimise PIC risk. → The unsigned-G1 fork is NOT taken; Phase
> 03/04 proceed with signed policy. Shipped: `kernel/src/ed25519.rs` + boot POST.

## Context Links
- Design: [research-cell-security-permissions.md](../../docs/research/research-cell-security-permissions.md) §3 (P5 needs signature verify)
- Roadmap: §G.2 "Operator-policy consent (G1)" (crypto sub-task)
- Related: `kernel/src/sha256.rs` (SHA-256 already in-kernel, shipped P3)

## Overview
**Priority:** P1 (gates Phase 03/04 — no policy verification without it).
**Status:** planned.
The kernel must verify an Ed25519 signature over the operator policy blob (Phase 03) against a fleet
root public key. The kernel currently has **no asymmetric crypto** in-tree (Silo does P-256, but in a
separate cell — unreachable from the boot path). This phase is a **spike + integration**: choose the
verify implementation, get it building in the PIC kernel, and prove it against RFC 8032 test vectors.

## Key Insights
- **Verify-only**, not sign — the kernel never signs. This is the cheaper half of Ed25519 (one
  scalar-basepoint mult + one double-scalar mult, SHA-512 of `R‖A‖M`).
- The PIC kernel build is finicky (per `project-release-build-broken-at-head`): some crates broke it.
  So the crate choice is a **real spike**, not a given. Order of preference:
  1. **`ed25519-dalek` / `curve25519-dalek` no_std** (`default-features=false`, `alloc`). Most
     trusted. Risk: pulls a large dep graph; may conflict with `relocation-model=pic` or need a
     `getrandom`/`rand_core` shim (verify needs no RNG — good). **Spike this first.**
  2. **A minimal audited verify-only port** (e.g. `ed25519-compact` no_std, or a vetted single-file
     impl) if (1) breaks the build.
  3. Hand-roll ONLY as last resort (Ed25519 field arithmetic is error-prone — would itself need a
     thinking-tier sub-effort + extensive vectors). Avoid.
- SHA-512 is required by Ed25519 (not SHA-256). The chosen crate must bring its own SHA-512, or add a
  `sha2` no_std SHA-512 (same family as our SHA-256; could extend `sha256.rs` → `sha512` if hand-roll).

## Requirements
- F1: `kernel::ed25519::verify(pubkey: &[u8;32], msg: &[u8], sig: &[u8;64]) -> bool` using
  **`verify_strict`** semantics (reject low-order/identity A, non-canonical R, S ≥ L). Never the
  cofactored/permissive `verify`. (Red-team M-malleability.)
- F2: Builds clean on riscv64 + aarch64 under `relocation-model=pic`.
- F3: Passes ≥2 RFC 8032 §7.1 positive vectors **AND** negative vectors: tampered msg/sig, **low-order
  public key**, **S = L** and **S > L** (must all return `false`).
- F4: **Round-trip test** — sign a blob with the dev private key (host side), verify in-kernel; proves
  the in-kernel path matches the signer, not just static vectors (red-team M-false-negative).
- N1: No RNG dependency. **Panic-free** on any malformed input → returns `false` (never `unwrap`).

## Architecture
New module `kernel/src/crypto/ed25519.rs` (file parallel to a `crypto.rs` facade, or a flat
`kernel/src/ed25519.rs` to mirror `sha256.rs` — **no `mod.rs`**, Law 5). Thin wrapper over the chosen
backend so Phase 03 depends only on `ed25519_verify`, isolating the backend decision.
```
// kernel/src/ed25519.rs
pub fn verify(pubkey: &[u8;32], msg: &[u8], sig: &[u8;64]) -> bool { … backend … }
#[cfg(test)] mod tests { /* RFC 8032 vectors */ }
```
Register `pub mod ed25519;` in `main.rs` (alongside `sha256`).

## Related Code Files
**Create**
- `kernel/src/ed25519.rs` — verify wrapper + RFC 8032 vector tests.
**Modify**
- `kernel/Cargo.toml` — add the chosen crypto crate (no_std, default-features off) IF crate route.
- `kernel/src/main.rs` — `pub mod ed25519;`.

## ⚠️ Decision fork (red-team M-fallback — MUST be resolved in this phase)
This phase **gates all of P5**. If the spike fails, do NOT silently slide into hand-rolling Ed25519
(a thinking-tier project of its own). Explicit fork:
- **Spike succeeds** (dalek or compact builds under PIC) → proceed to Phase 03 with signed policy.
- **Both crate routes break PIC** → **STOP P5 signing.** Ship policy **unsigned** in G1, its integrity
  resting on the kernel-embedded VIFS1 image (the image itself is the trust unit; measured-boot is the
  G-future story). Defer Ed25519 to G2. Phase 03/04 then load+parse the blob WITHOUT a sig check (the
  `policy_verify_bypass` path becomes the G1 default, loudly logged). Document this as a G1 limitation.
This keeps P5's *policy model* shippable even if in-kernel crypto is blocked.

## Implementation Steps
1. **Spike (timeboxed, on its own branch — serialize before Phase 01's final gate):** add
   `ed25519-dalek` (no_std, default-features off) to `kernel/Cargo.toml`;
   `RUSTFLAGS=-C relocation-model=pic cargo build --release -p vicell-kernel` + aarch64.
2. If clean → wrap as `ed25519::verify` (use `verify_strict`). If PIC breaks → try `ed25519-compact`.
   If both break → take the **Decision fork** above (unsigned-policy-G1) and record it.
3. Add RFC 8032 §7.1 positive vectors + negatives (tamper, low-order A, S=L, S>L) `#[cfg(test)]`.
4. Host-verify the vectors (scratch host bin mirroring the wrapper, as SHA-256 was checked) +
   the round-trip (dev-key sign → in-kernel verify).
5. **Dev key gating:** the dev keypair's PUBLIC key lives behind `#[cfg(feature = "dev-policy-key")]`
   — NEVER `debug_assertions` (the standard build is `--release`). Add a CI assertion that the release
   artifact's embedded `FLEET_ROOT_PUBKEY != dev_pubkey` (byte compare).
6. Build both arches clean; boot smoke (`ViCell >`) to confirm the dep didn't break the PIC build.

## Todo
- [ ] crate spike: ed25519-dalek no_std under PIC (riscv64 + aarch64)
- [ ] fallback decision recorded if spike fails
- [ ] `ed25519::verify` wrapper + module registration
- [ ] RFC 8032 vectors pass (host-verified) + tamper-negative
- [ ] build both arches clean
- [ ] boot smoke green (dep didn't break PIC build)

## Success Criteria
- `ed25519::verify` correct on RFC 8032 vectors, rejects tampering.
- Both arches build clean under PIC; boot still reaches `ViCell >`.

## Risk Assessment
| Risk | Mitigation |
|------|-----------|
| dalek breaks PIC kernel build (dep graph / asm) | Spike FIRST, timeboxed; documented fallback chain (compact → audited port) |
| Pulls `getrandom` and fails to link | Verify needs no RNG; disable features; provide no-op shim only if a transitive dep insists |
| SHA-512 missing | Crate provides it; if hand-roll path, extend sha256.rs family with SHA-512 + vectors |
| Code-size blowup on the 4.4MB kernel | Measure kernel size delta; acceptable if < ~150KB; else minimal port |

## Security Considerations
- Verify-only; no private key in kernel. The fleet root **public** key (Phase 03) is the trust anchor.
- Constant-time is not strictly required for *public-key signature verify* (no secret involved), so a
  non-CT verify is acceptable; do not reuse this module for any secret-dependent op.

## Next Steps
Phase 03 consumes `ed25519::verify` to validate the policy blob at boot.
