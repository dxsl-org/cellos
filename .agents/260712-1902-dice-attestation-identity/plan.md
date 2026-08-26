---
title: "DICE/RIoT attestation + KMS Cell + K2/K3 node identity"
description: "Assemble a DICE measured-boot attestation chain, a thin KMS service Cell, and the K2/K3 identity rungs from primitives that already ship — software slice first (CI-testable), Silo-anchored + hardware-informed parts deferred."
status: queued (P00 complete; P01-P05 pending; P06 deferred)
priority: P2
effort: 21
branch: main
tags: [security, attestation, dice, kms, identity, g2, silo, planning-only]
created: 2026-07-12
---

# DICE/RIoT attestation + KMS Cell + K2/K3 node identity

> **D35 portfolio ruling (2026-08-01):** child of Trust & Identity, not a merged
> manifest/revocation plan. P00 landed in `aebc092a`; retain phase-local Law-1 and
> hardware/consumer gates for P01-P06.

PLANNING ONLY — Mythos analysis window (expires 2026-07-14). No code lands until the window ends.
This is **assembly, not invention**: every primitive exists (dossier-4). Order puts the CI-testable
software slice first, then the Silo-anchored root, then KMS, K2, K3, and the deferred Veraison adapter.

## Design authority (locked — do not re-litigate)
`.agents/260712-1836-mythos-g123-analysis/dossier-4-dice-identity.md` — Decisions 1 (CDI source), 2
(token shape), 3 (K1→K2→K3 ladder), KMS scope.

## Architecture spine (one-paragraph)
`root secret` (Silo-held UDS-equivalent where Silo exists; dev software-seed in CI) → `CDI_final =
HKDF(root, measurement_aggregate)` (aggregate from `measurement_log::aggregate()` kernel/src/measurement_log.rs:83)
→ `CDI_final` is fed to `SiloHandle::init_key(seed)` libs/ostd/src/silo.rs:130 → the resulting **P-256
pubkey is the attested identity (Alias key)** → the identity + aggregate are packed into a VPOL-shaped
internal token (P-256-signed by Silo, verified in userspace with the `p256` crate) → K3 embeds that
token in the swarm enrollment ticket. Same code path on all arches; only the root swaps (Decision 1).

## Phases
| # | Phase | Effort | Testability | Depends on | Law 1 |
|---|-------|:------:|-------------|-----------|:-----:|
| P00 | [CDI derivation + internal attestation-token library](phase-00-cdi-token-library.md) | 3 | **G1 / CI, all arches** | — | no |
| P01 | [Kernel measurement-aggregate read syscall](phase-01-aggregate-read-syscall.md) | 2 | G1 / CI, all arches | P00 | **YES (new syscall)** |
| P02 | [Silo-anchored root CDI (software-seed fallback)](phase-02-silo-anchored-root.md) | 4 | G2 / hardware-informed | P00,P01 | no |
| P03 | [KMS Cell (thin Silo veneer, first client = TLS)](phase-03-kms-cell.md) | 3 | partial CI (fallback) | P02 | **service-ID + wire types (additive ABI)** |
| P04 | [K2 per-node identity (first-boot machine-id)](phase-04-k2-per-node-identity.md) | 2 | G1 / CI (VFS+RNG) | — (parallel) | no |
| P05 | [K3 attested enrollment (DICE token in ticket)](phase-05-k3-attested-enrollment.md) | 4 | G2 / hardware-informed | P00,P02,P04 | no |
| P06 | [Veraison / COSE_Sign1 EAT interop adapter](phase-06-veraison-cose-adapter.md) | 3 | **deferred** (do not spec before verifier) | P05 | no |

## Critical dependencies / ordering rationale
- P00 has zero deps and is the entire CI-testable surface — it is the natural first phase.
- P01 unblocks binding the token to *real* boot state; it is the only new-syscall phase (2x confirm).
- P02/P03 are the Silo-anchored + service layers; P03's root-CDI-sealing is why KMS is planned here.
- P04 (K2) is crypto-free provisioning and **can be built in parallel** with P02/P03.
- P05 is the convergence point (DICE token = K3 payload); it needs P00+P02+P04.
- P06 is explicitly deferred — YAGNI until a third-party (Veraison) actually consumes the token.

## Roadmap rule honored
Hardware-gated parts (Silo maturity, first-boot enrollment provisioning, Veraison interop) are NOT
over-specified before the hardware. Each such phase carries a "hardware-informed / do-not-over-spec"
banner and a software-seed/dev-fallback so CI stays green with no Silo backend.

## Coding-law flags
- **Law 1**: P01 adds one syscall (`ReadMeasurement`, libs/api). P03 adds `service::KMS` + KMS wire
  types (additive, but ABI-visible). Both need explicit 2x user confirmation before code.
- **Law 4**: all new code lives in Cells / libs → `#![forbid(unsafe_code)]`; the P01 syscall handler
  is the only kernel touch and adds no `unsafe`.
- **Law 2/6/8** respected throughout (owned buffers, `Vi`/service naming, Drop where handles held).

## Open questions
See each phase's Risk section; the load-bearing one is P00/P05 **signature-algorithm mismatch**
(VPOL template is Ed25519/64-byte; Silo signs P-256 DER) — resolved by carrying a 64-byte raw r‖s
P-256 signature and verifying in userspace, NOT via the kernel Ed25519 path.
