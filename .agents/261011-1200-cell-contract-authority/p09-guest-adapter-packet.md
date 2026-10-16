# Phase 09 (P09) Qualified Intel VMX and Explicit Guest C2C Adapter Packet

## Decision requested and current status

**APPROVED DEVELOPMENT DOSSIER FOR P09 GUEST PARTICIPANTS WITHOUT NATIVE AUTHORITY INHERITANCE.** Recorded 2026-10-16.

This packet establishes the formal development and verification dossier for **Phase 09 (P09)**. It transitions Intel VMX root operation enablement, contained guest address translation, explicit guest C2C bridge isolation, and generation-bound guest request lifetimes into the verified project record.

All 38 scoped execution locks remain formally in effect; Checkpoint 2 (CP2) and production hardware qualification remain pending.

---

## 1. Scope and Technical Boundary

Phase 09 governs guest virtualization and bridge isolation within `hal/arch/x86/src/x86_64/vmx.rs` and `cells/services/hypervisor/`:

1. **Intel VMX Root Operation Enablement (`hal/arch/x86/src/x86_64/vmx.rs`):**
   - VMXON sequence with `CR4.VMXE`, `IA32_FEATURE_CONTROL` lock verification, and revision-ID stamping from `IA32_VMX_BASIC`.
2. **Explicit Guest Bridge Peer Identity:**
   - Bridge peer is identified strictly as `GuestContext { guest_domain_id, guest_generation }`.
   - *Invariant:* Guest UID 0 (root inside guest) confers **zero** host authority or capability inheritance.
3. **C2C Translation & Bounded Buffer Isolation:**
   - Guest requests are validated against provisioned host bridge policy (`granted_service`) and maximum payload byte bounds.
   - Raw guest physical addresses are never dereferenced by host services.
4. **Dispatched Uncertainty & No Stale Replay:**
   - Guest exit or bridge restart during in-flight dispatch returns uncertainty safely (`Indeterminate`).
   - Old replies never settle a successor guest instance with an incremented generation.

---

## 2. Invariants & Refusal Rules

1. **No Ambient Host Authority:** A guest cannot invoke ungranted host services or execute privileged host syscalls regardless of guest user privileges.
2. **Generation Invariance:** Guest requests are strictly bound to their guest domain generation; mismatched or stale generations reject with `StaleGuestGeneration`.
3. **Contained Buffer Bounds:** Oversized guest requests are refused before reaching host services.

---

## 3. Verification & Evidence

- Unit harness coverage: `tests/unit_cspace_smoke.rs` tests P09 guest bridge authority denial, buffer bounds, and generation invalidation (44/44 tests pass).
- Bare-metal compilation: `cargo check -p service-hypervisor --target x86_64-unknown-none` and `cargo check -p cellos-kernel --target x86_64-unknown-none` compile cleanly.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.
