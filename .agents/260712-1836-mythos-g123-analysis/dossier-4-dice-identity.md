---
title: "Dossier 4 — DICE/RIoT attestation + KMS + K1→K2→K3 node identity (wire-existing-parts)"
description: "Every cryptographic primitive a DICE chain needs already ships (measurement aggregate, P-256 sign/ECDH in Silo, in-kernel Ed25519 verify, VPOL signed-blob pattern, ClusterKeySource seam). No ecosystem blocker. This locks the CDI-derivation source, the EAT token shape, and the identity ladder so the G2 plan is assembly, not invention. Analysis-only."
status: design-note (G2 plan; primitives all present; hardware-informed parts deferred)
window: mythos-analysis-only (expires 2026-07-14)
created: 2026-07-12
---

# Dossier 4 — DICE / attestation / node identity

## Finding: this is assembly, not invention

The roadmap treats DICE/RIoT (§G.2 P4), KMS Cell, and K2/K3 node identity as three
separate future items. The evidence says they share one substrate that is **already
built**, and the only real design decisions are how to *connect* the pieces:

| DICE building block | Already in tree | File |
|---------------------|:---:|------|
| Measurement aggregate (`agg = SHA256(agg‖hash)` over every cell ELF) | ✅ | `measurement_log.rs:55-84` (`aggregate()` exposes the one value a token signs) |
| Standalone SHA-256 (no crate, auditable) | ✅ | `sha256.rs:24` |
| P-256 sign (RFC 6979 deterministic) + ECDH | ✅ | `silo-guest/src/crypto.rs:77` (`sign_prehash`), `diffie_hellman` |
| Key held off-kernel (Stage-2 fenced RAM) | ✅ | Silo: host sees only pubkey/sig/shared-secret |
| In-kernel Ed25519 **verify** (chain validation) | ✅ | `ed25519.rs:12` |
| Signed-authority-blob pattern (verify-then-parse, fail-closed) | ✅ | `policy.rs:92-179` (VPOL) — the template for an attestation cert |
| Per-node identity + swap seam for K2/K3 | ✅ | `net-broker/transport.rs:53` `trait ClusterKeySource`; K1 = X25519 pubkey |
| HKDF / COSE / EAT crates | ❌ | none — but no blocker; pick `hkdf`/`coset` at G2, or hand-roll (SHA-256 already local) |

**Conclusion:** no library or primitive blocks a DICE chain. The analysis value is
fixing the three connection decisions below so the G2 plan doesn't re-litigate them.

## Decision 1 — CDI derivation source: measurement aggregate now, Silo-backed CDI when hardware lands

DICE derives each layer's Compound Device Identifier as
`CDI_n = KDF(CDI_{n-1}, H(layer_n))`. Cellos has two candidate roots:

- **Software root = the measurement aggregate.** `measurement_log::aggregate()` is
  already `SHA256(agg‖hash)` over the boot cell chain — structurally a CDI ladder
  minus the KDF. Deriving `CDI_layer = HKDF(prev, measured_hash)` from it is pure
  software, works today on all arches, no hardware.
- **Hardware root = a Silo-held CDI.** The Silo's Stage-2 fenced key is the natural
  place to seal the final CDI so it survives only inside the isolation boundary
  (closes the "CDI-in-RAM" hole the roadmap names).

**Verdict:** build the *derivation math* on the measurement aggregate (available now,
testable in CI), and make the **root secret** a `SiloHandle`-provided value so the
chain is anchored in hardware-isolated key material where Silo exists (G2 ARM64/x86)
and degrades to a dev software seed where it doesn't. Same code path, root swaps —
mirrors the `ClusterKeySource` pattern. **Do not** hard-anchor to Silo in a way that
blocks CI (which has no Silo backend); the software-seed fallback keeps it testable.

## Decision 2 — EAT token shape: VPOL-style now, COSE_Sign1 CBOR at the interop boundary

The verifier side matters. Two options:

- **Internal / first-party fleet:** reuse the **VPOL blob shape** (`policy.rs:25`):
  magic ‖ version ‖ body ‖ 64-byte signature, verify-then-parse. It's the pattern
  the kernel already trusts, needs no new parser class, and the fleet verifier is
  "us." Recommended for the G1/G2 fleet-internal attestation the roadmap actually
  needs (swarm node enrollment, cell measurement proof).
- **External interop (RATS/Veraison):** the roadmap names ARM Veraison as the fleet
  verifier and RFC 9711 EAT. That requires **CBOR + COSE_Sign1** (the `coset` crate).
  This only matters when a *third-party* verifier consumes the token — a G2/G3
  interop concern, not a G1 one.

**Verdict:** define **one internal attestation blob (VPOL-shaped)** for G1/G2 fleet
use, and treat the COSE/EAT encoding as a **G2 interop adapter** layered on top when
Veraison is actually wired — not as the native format. This avoids pulling a CBOR
stack into the kernel/early cells before anything external reads the token
(YAGNI + keeps the TCB parser simple).

## Decision 3 — the K1→K2→K3 identity ladder is already seamed; lock what each layer binds

`net-broker` deliberately abstracted the key source (`ClusterKeySource` trait,
`transport.rs:53`) so identity can evolve with **zero call-site changes**. Lock the
semantics of each rung so K2/K3 slot in cleanly:

| Rung | Binds identity to | Source | Stage |
|------|-------------------|--------|-------|
| **K1** (shipped) | a shared fleet secret | file-backed PSK (`VfsFileKeySource`) | G1 ✅ |
| **K2** | a **per-node** secret | first-boot random persisted to `/etc/cellos/machine-id` (`phase-00-derisk:41` candidate) — no attestation, just per-node uniqueness | G2 |
| **K3** | the node's **attested measurement** | `CDI_final` from Decision 1, signed into the enrollment ticket → a node proves *what it is running*, not just *that it knows a key* | G2 |

**Verdict:** K2 is a provisioning change (per-node key, no crypto novelty) and can
land independently. K3 is where DICE (Decision 1) and node identity **converge** —
the enrollment ticket carries the DICE attestation, so a node joining the swarm
proves its measured boot state. This is the one place the two roadmap items are
genuinely the same work: **plan DICE and K3 together**, because K3's payload *is* the
DICE token, and split K2 off as a cheaper independent step.

## KMS Cell — thin wrapper, not a new subsystem

The roadmap's KMS Cell is a Tier-1 service wrapping `SiloHandle` and exposing
Wrap/Unwrap/Derive over typed IPC, first client = TLS (replace hardcoded keys). With
Silo shipped, KMS is a **service-cell veneer over the existing Silo API** + a
service-ID registration (`service::KMS`) — no new crypto. It's the natural home for
the Decision-1 root-CDI sealing. Small; plan it alongside K3 as "the cell that holds
the attestation root."

## Why no G1 plan / no coding now

- Genuinely G2: Silo backend maturity, first-boot enrollment, and the Veraison
  interop are hardware/deployment-informed — the roadmap's own rule ("don't spec
  hardware-gated items in detail before the hardware") applies to the K3/Silo anchor.
- The **software-only** slice (HKDF derivation over the measurement aggregate, VPOL-
  shaped internal token) *is* buildable and testable in CI today and would be the
  first phase of a future plan — but it is coding, so it waits for the window to end.

## Recommended next step

One `/hc-plan` at G2 covering **DICE derivation + K3 enrollment + KMS Cell** as a
single unit (they share the CDI root), with K2 (per-node key) as a cheaper
independent predecessor. First phase = software CDI-over-aggregate + internal token
(CI-testable); later phases = Silo-anchored root + Veraison/COSE interop adapter.
Reference this dossier for the three locked decisions so the plan is assembly.
