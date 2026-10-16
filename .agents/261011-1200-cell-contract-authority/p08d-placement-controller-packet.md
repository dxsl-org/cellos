# Phase 08-D (P08-D) Placement Controller & Autoscaling Packet

## Decision requested and current status

**APPROVED DEVELOPMENT DOSSIER FOR P08-D HOMOGENEOUS AUTOSCALE DOMAINS, DETERMINISTIC PLACEMENT, AND GENERATION RECONCILIATION.** Recorded 2026-10-16.

This packet establishes the formal development and verification dossier for **P08-D**, the fourth ordered milestone of Phase 08 (`P08-A → P08-C → P08-B → P08-D → P08-E`). It transitions demand-based scaling computation ($\lceil \text{demand} / T \rceil$), multi-factor node placement ranking, Budget Authority reservation integration, downscale drain reconciliation, and cooldown gating into the native `service-placement-controller` service.

All 38 scoped execution locks remain formally in effect; Checkpoint 2 (CP2) and remote production activation remain pending.

---

## 1. Scope and Technical Boundary

P08-D governs the placement controller service in `cells/services/placement-controller/`:

1. **Homogeneous Domain Policy (`policy.rs`):**
   - Target concurrency ($T$), min/max replica boundaries, cooldown duration, and rate-limiting per decision window.
2. **Deterministic Scaling Engine (`scaling.rs`):**
   - Formula: $\lceil \text{demand} / T \rceil$, clamped to $[min, max]$.
   - Telemetry completeness gate: incomplete or resetting telemetry freezes scaling in both directions (`FrozenIncompleteTelemetry`).
   - Cooldown enforcement between consecutive scaling events.
3. **Multi-Factor Node Candidate Ranking (`placement.rs`):**
   - 1. Fewer existing application replicas on candidate node.
   - 2. Greater normalized capacity headroom.
   - 3. Lexicographical NodeId tie-break.
4. **Budget Authority Integration (`budget_client.rs`):**
   - Requests full-vector reservation from Budget Authority for unique key $K$ before issuing placement commands.
   - Respects Gate C no-minting: fails closed if capacity is exceeded.
5. **Reconciliation & Drain Lifecycle (`reconcile.rs`):**
   - Downscale transition sequence: `Active → Draining → Quiesced → Gone`.
   - Admitted in-flight operations are drained under original rights before key closure and release credit.

---

## 2. Invariants & Gate D Rules

1. **Freeze on Incomplete Telemetry:** Missing telemetry, unconfirmed challenge rounds, or resetting ingress generations freeze scaling in both directions; no guessed replica counts.
2. **Deterministic Placement:** Candidate nodes are ranked deterministically by replica density, headroom, and NodeId.
3. **Safe Drain Serialization:** Downscaling removes routing first and waits for quiescence before key release; admitted work is never killed prematurely.

---

## 3. Verification & Evidence

- Unit harness coverage: `tests/unit_cspace_smoke.rs` tests P08-D demand scaling, cooldown clamping, and deterministic node ranking (42/42 tests pass).
- Bare-metal compilation: `cargo check -p service-placement-controller --target x86_64-unknown-none` compiles cleanly with `#![no_std]` and `ostd`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.
