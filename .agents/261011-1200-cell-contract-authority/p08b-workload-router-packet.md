# Phase 08-B (P08-B) Workload Router Implementation Packet

## Decision requested and current status

**APPROVED DEVELOPMENT DOSSIER FOR P08-B WORKLOAD ENDPOINTS, ROUTING, AND BOUNDED ADMISSION.** Recorded 2026-10-16.

This packet establishes the formal development and verification dossier for **P08-B**, the third ordered milestone of Phase 08 (`P08-A → P08-C → P08-B → P08-D → P08-E`). It transitions workload endpoint management, destination Ready evidence validation, bounded weighted selection, and receiver-initiated challenge rounds into the native `service-workload-router` service.

All 38 scoped execution locks remain formally in effect; Checkpoint 2 (CP2) and remote production activation remain pending.

---

## 1. Scope and Technical Boundary

P08-B governs the workload router service in `cells/services/workload-router/`:

1. **Workload Endpoints & Ready Verification (`endpoint.rs`):**
   - Logical endpoints opt into replication-compatible typed methods; concrete endpoints remain generation-pinned.
   - Router verifies assigned destination/issuer Ready evidence against each member's artifact hash, tier, reservation key, and generation.
2. **Bounded Admission & Origin Proof (`admission.rs`):**
   - Validates immutable origin proof without altering payload, deadline, or minting unauthorized requests.
   - Bounded payload checking against maximum allowed bytes.
3. **GCD-Normalized Weighted Selection & Target Freezing (`routing.rs`):**
   - Deterministic weighted round-robin cycle over active ready members.
   - Skips draining, suspect, or stale members.
   - Freezes the selected target for the submission attempt: exactly one selected attempt (no retry, hedging, or replay).
4. **Bounded Queue & Deadline Eviction (`queue.rs`):**
   - Enforces limits on queue item count and total payload bytes.
   - Evicts expired requests; expired requests never execute on late Ready.
5. **Receiver-Initiated Bounded Challenge Rounds (`telemetry.rs`):**
   - Bounded challenge rounds with nonces where TTL runs strictly from local start time, never arrival time.
   - Completed challenges cannot be reused.

---

## 2. Invariants & Gate B Rules

1. **Deterministic Selection:** Unequal weights, draining flags, and stale generations obey deterministic bounded selection.
2. **No Selection Attenuation Alteration:** The router cannot alter payload, extend original deadlines, or mint a request.
3. **Single Attempt Freezing:** The concrete target is frozen at submission; ambiguous or failed outcomes return without automatic retry.

---

## 3. Verification & Evidence

- Unit harness coverage: `tests/unit_cspace_smoke.rs` tests P08-B weighted selection, target freezing, deadline eviction, and challenge TTL (40/40 tests pass).
- Bare-metal compilation: `cargo check -p service-workload-router --target x86_64-unknown-none` compiles cleanly with `#![no_std]` and `ostd`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.
