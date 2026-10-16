# Phase 08-A (P08-A) Authenticated Transport & Lifecycle Substrate Packet

## Decision requested and current status

**APPROVED DEVELOPMENT DOSSIER FOR P08-A AUTHENTICATED TRANSPORT AND LIFECYCLE SUBSTRATE.** Recorded 2026-10-16.

This packet establishes the formal development and verification dossier for **P08-A**, the first ordered milestone of Phase 08 (`P08-A → P08-C → P08-B → P08-D → P08-E`). It transitions the ratified Wire ABI C2C transport envelope (D12) and ingress evaluation core into an authenticated multi-gate substrate without ambient cluster root or unverified remote execution authority.

All 38 scoped execution locks remain formally in effect; Checkpoint 2 (CP2) and remote production activation remain pending.

---

## 1. Scope and Technical Boundary

P08-A governs the transport and ingress boundaries within `cells/services/net-broker/`:

1. **C2C Canonical Envelope (D12):**
   - 112-byte fixed header encoding `version=1`, `kind (1..7)`, `retry_class (1..3)`, `request_id`, `src_node`, `dst_node`, `src_boot_epoch`, `dst_server_epoch`, `cluster_id`, `service_id`, `export_id`, `deadline_ms`, followed by framed payload ($\le 3712$ bytes).
2. **Six-Gate Ingress Evaluation Core (`c2c_ingress.rs`):**
   - Gate 1: Source identity match against transport-authenticated peer (`AuthenticatedPeer`).
   - Gate 2: Destination node match against local broker identity (`CellNetId`).
   - Gate 3: Method authorization against independently provisioned peer policy (`ProvisionedPeerPolicy`).
   - Gate 4: Local export prototype match against active registry (`RemoteExports`).
   - Gate 5: Live target local service generation match against server epoch (`LiveDestinationProof`).
   - Gate 6: Source boot epoch non-rollback proof (`ProtectedEpochProof`).
3. **Deduplication and Quota Bounding (`c2c_dedup.rs`):**
   - In-flight deduplication and cached replay for idempotent calls.
   - Per-peer concurrency limit (`MAX_PER_PEER_WORK = 4`) and byte limit (`MAX_PER_PEER_BYTES`).
   - Global in-flight capacity limit (`MAX_GLOBAL_WORK = 16`).

---

## 2. Invariants & Refusal Rules

1. **No Ambient Trust in Payload:** Remote authenticated channels carry protocol-bound cryptographic identities; payload CellIds are never trusted for authority.
2. **Immutable Origin Operation Binding:** Request grants authorize exactly one origin operation; tampering with payload, deadline, or service IDs fails closed at Gate 1/3.
3. **Stale Epoch Rejection:** Non-increasing server epochs or mismatched boot epochs are rejected with `DestinationNotLive` or `UnverifiedEpochProvenance`; no stale replay is permitted.
4. **Clean Cutover, No Shims:** Malformed frames, unsupported versions, or unrecognized kinds return `MalformedEnvelope` or `UnsupportedKind` immediately.

---

## 3. Verification & Evidence

- Unit harness coverage: `tests/unit_cspace_smoke.rs` tests P08-A 112-byte C2C header encoding, multi-gate ingress evaluation, and epoch rejection.
- Bare-metal compilation: `cargo check -p service-net-broker --target x86_64-unknown-none` compiles cleanly with `#![no_std]` and `ostd`.
- Types validation: `cargo test -p types --target x86_64-unknown-linux-gnu` passes 54/54 tests clean.
