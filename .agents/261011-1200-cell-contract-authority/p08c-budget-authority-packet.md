# Phase 08-C (P08-C) Budget Authority Implementation Packet

## Decision requested and current status

**APPROVED DEVELOPMENT DOSSIER FOR P08-C INDEPENDENT BUDGET AUTHORITY, GENESIS, AND DETERMINISTIC PLACEMENT.** Recorded 2026-10-16.

This packet establishes the formal development and verification dossier for **P08-C**, the second ordered milestone of Phase 08 (`P08-A → P08-C → P08-B → P08-D → P08-E`). It transitions the multi-dimensional application capacity ledger, one-use bootstrap reservation, live destination atomic claim, Closing serialization, and monotonic enforcement fence into the native `service-budget-authority` service.

All 38 scoped execution locks remain formally in effect; Checkpoint 2 (CP2) and remote production activation remain pending.

---

## 1. Scope and Technical Boundary

P08-C governs the independent Budget Authority service in `cells/services/budget-authority/`:

1. **Multi-Dimensional Application Capacity Ledger (`ledger.rs`):**
   - 6 tracked dimensions: Cell/replica count, thread count, capability count, operation count, memory bytes, and aggregate CPU execution entitlement (ns).
   - Immutable application maxima ($M$), admitted ceiling ($B \le M$), and effective allocated charge ($A \le B$).
   - Capacity debit validation: any dimension exceeding available headroom is rejected (`CapacityExceeded`). No minting of unbudgeted capacity (Gate C).
2. **One-Use Bootstrap Reservation (`bootstrap.rs`):**
   - Issuer precommitted genesis adoption. One-use bootstrap grant consumed during initial supervisor spawn. Re-consumption or uncommitted initialization fails closed.
3. **Atomic Reservation & Live Destination Claim (`reservations.rs`, `claim.rs`):**
   - Unique 16-byte create key $K$ atomically debited against ledger before grant emission.
   - Live atomic claim: destination binds to reservation under epoch matching; subsequent claim fails (`AlreadyClaimed`).
   - Closing serialization: `Closing(K)` blocks subsequent claims and guarantees credit-once release upon quiescence.
4. **Monotonic Enforcement Fence (`fence.rs`):**
   - Non-regressing authority fence ($\text{fence} \ge \text{floor}$). Monotonic advance succeeds; regressions fail closed.
5. **Fail-Closed Crash Recovery (`recovery.rs`):**
   - Distinguishes valid genesis from uninitialized storage or corrupt media. Refuses to manufacture a writable ledger on corrupt or missing state.

---

## 2. Invariants & Gate C Rules

1. **Gate C - No Minting:** Malicious or misbehaving controllers issuing distinct reservations cannot exceed the aggregate ceiling; excess requests fail with `CapacityExceeded`.
2. **Atomic Credit-Once:** Releasing a reservation restores the allocated vector exactly once; duplicate or over-release fails with `MonotonicityViolation`.
3. **Claim Serialization:** Claim competes atomically with Closing; a closed key cannot be claimed, and a claimed key cannot be reclaimed by a foreign destination.

---

## 3. Verification & Evidence

- Unit harness coverage: `tests/unit_cspace_smoke.rs` tests P08-C multi-vector capacity bounds, reservation & claim lifecycle, and fence monotonicity (37/37 tests pass).
- Bare-metal compilation: `cargo check -p service-budget-authority --target x86_64-unknown-none` compiles cleanly with `#![no_std]` and `ostd`.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.
