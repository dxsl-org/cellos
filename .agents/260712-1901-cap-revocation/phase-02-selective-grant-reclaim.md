# Phase 02 (G2) — Selective grant reclaim via the existing `shared_to` link

## Context Links
- Plan: [plan.md](plan.md) | Depends on: P00. Independent of P01 (grants ≠ IOMMU).
- Design authority: dossier §"The derivation-tree question" — "reuse the grant table's existing `owner → shared_to` link as the minimal derivation record; do NOT build a general seL4-style CDT."

## Overview
- **Priority:** P1. **Speed:** G2. **Status:** completed (2026-09-25). **Law 1:** none. **Effort:** S.
- Reclaim page/reg grants **owned by** a revoked cell without waiting for its death, reusing the reaper's owner-side unmap path. Reclaiming the owner's grant unmaps the grantee too (the `shared_to` link is the derivation breadcrumb).

> **D36 precedence ruling (2026-08-01):** Midori phase 07 owns pin/quarantine
> mechanism and teardown ordering; this phase owns the revoke trigger/policy. Selective
> reclaim must consult in-flight pins and quarantine frames until cancellation or driver
> acknowledgement. Immediate free/fault semantics below are historical and must not be
> implemented for an in-flight CPU/DMA grant.

## Key Insights
- `reap_grants_for_task(dead_tid)` (`syscall.rs:192-239`) has two passes over `PAGE_GRANT_TABLE` + `REG_GRANT_TABLE`: (a) clear `shared_to` where the *grantee* died; (b) remove entries the *dead_tid* **owns** and `free_grant_pages`.
- For runtime revoke we want ONLY pass (b) against the target's owned grants — reclaiming an owned grant frees the pages, which unmaps them from both owner and any grantee (the grantee's mapping resolves to freed frames → its next access faults). That is the "reclaim unmaps the grantee too" guarantee, achieved through the existing structure with **no new derivation record**.
- Lock order is documented + load-bearing: `PAGE_GRANT_TABLE collect → unmap (KERNEL_ROOT) → FRAME_ALLOCATOR`; never hold `FRAME_ALLOCATOR` across `free_grant_pages` (`syscall.rs:190`). A selective reclaim MUST honor the identical order.
- SAS frame-identity invariant (memory MEMORY): freed frames must stay identity-mapped; `free_grant_pages` already respects this — reuse it, do not hand-roll unmap.

## Requirements
- **F1:** add `reclaim_grants_for_task(tid)` = the owner-side pass (b) of the reaper, extracted so both the reaper and revoke call ONE implementation (DRY — no fork of the collect/free logic).
- **F2:** it must NOT touch the target's grantee-side entries (the target keeps grants it *received*; those are governed by the granter's authority, not the target's).
- **F3:** identical lock order to `reap_grants_for_task`.
- **F4:** trigger scope (open-question #3, resolved): invoke on **any Class-2 revoke** of the target, because a shared grant can back MMIO/DMA cooperation and grants carry no source-cap tag. Reclaiming only owner grants of the revoked cell is safe and bounded (does not reach unrelated cells).

## Architecture
Refactor the reaper into two helpers:
- `clear_grantee_refs(tid)` — pass (a), used only on death.
- `reclaim_owned_grants(tid)` — pass (b), used by BOTH death and revoke.

`reap_grants_for_task` composes the same owner-side reclaim only after the
pin/quarantine authority allows reclaim. Revoke uses that mechanism and may quarantine
rather than immediately free.

Data flow (revoke): Class-2 revoke of `tid` → `reclaim_owned_grants(tid)` → collect owned keys under grant lock → drop lock → `free_grant_pages` per grant (unmaps + returns frames, keeping identity map) → grantee's stale mapping now points at freed frames → grantee faults on next access.

## Related Code Files
- **Modify:** `kernel/src/task/syscall.rs` (`reap_grants_for_task` :192 — extract the two passes; add `reclaim_owned_grants`).
- **Read-only:** `free_grant_pages`, `grant_table_lock`/`reg_grant_table_lock`, `PageGrant`/`RegGrant` `shared_to` fields.

## Implementation Steps
1. Extract pass (a) → `clear_grantee_refs(tid)`; pass (b) → `reclaim_owned_grants(tid)` (both grant tables).
2. Recompose `reap_grants_for_task` from the two — assert unchanged behavior (existing grant tests stay green).
3. Export `reclaim_owned_grants` at `pub(crate)` for the revoke arm.
4. Verify lock-order comment is carried onto the extracted fn verbatim.

## Todo List
- [ ] Extract `clear_grantee_refs` + `reclaim_owned_grants` from the reaper
- [ ] Recompose `reap_grants_for_task`; existing behavior identical
- [ ] `pub(crate)` export for revoke
- [ ] Test: cell A shares a page to B; reclaim A's owned grants; B's read of the page faults; A's entry gone
- [ ] Test: grants A *received* from C survive A's reclaim

## Success Criteria
- After `reclaim_owned_grants(A)`: A owns zero grants, frames returned, and a grantee B faults on next access to the reclaimed region — while grants A *received* are untouched.
- Grant reaper regression tests unchanged (refactor is behavior-preserving).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Lock-order violation → deadlock with FRAME_ALLOCATOR | Low×**High** | Extract verbatim; never hold FRAME_ALLOCATOR across free; reuse existing collect-then-free shape. |
| Reclaiming a grant a live grantee is mid-DMA into | Low×High | Pairs with P01 IOMMU teardown ordering; for CPU grantees the fault-on-next-access is the intended fail-closed. |
| Over-broad reclaim reaches unrelated cells | Low×Med | Scope strictly to `owner == tid`; grantee-received grants excluded. |

## Security Considerations
The shared grant is the ONLY surface that needs a derivation breadcrumb, and it already carries one (`shared_to`). Reclaiming the owner's grant is the correct, minimal cascade — no general CDT, no reverse-grant chase (monotonic downgrade guarantees none exists).

## Next Steps
- Consumed by P04's Class-2 teardown dispatch.
- Rollback: revoke stops calling `reclaim_owned_grants`; the refactor itself is behavior-preserving and can stay.

---

## Closure (2026-09-25)

**Verified on:** RV64 QEMU TCG 8.2.2, `harts=1`, kernel `76832e05…`, feature tuple
`native-domains,test-hooks` — raw log `docs/evidence/cap-revoke-qemu.{log,txt}`.

### Delivered
- `reclaim_owned_grants(tid)` — the owner-side pass of the reaper, extracted so death and
  runtime revoke call ONE implementation (both tables). It carries the reaper's lock order
  verbatim (grant table collect → `FRAME_ALLOCATOR` → `KERNEL_ROOT`), never holds
  `FRAME_ALLOCATOR` across `free_grant_pages`, and keeps the registered-grant
  transfer-to-live-grantee rule.
- `clear_grantee_refs(tid)` — pass (a), death-only: a grant the dead task *received*
  becomes unshared again while its owner keeps the entry and the frames.
- `sweep_orphan_reg_grants()` — the death-only sweep of entries with neither owner nor
  grantee (the residue of a transfer that found no live grantee). Kept out of the shared
  path: a revoke neither creates nor resolves that state.
- `reap_grants_for_task` recomposed from the three, so the death path's behavior is
  unchanged by construction.

### D36 (in-flight pins) — satisfied by construction
The reclaim routes every frame through the same `withhold_pinned_frames` transaction the
death path uses (`withhold_or_free`): a region an in-flight operation still holds is
**quarantined, not freed**, and `release_acked_frames` returns it once the driver
acknowledges. The plan's historical "immediate free/fault" wording is not implemented
anywhere.

### Evidence (markers in the published log)
| Property | Marker |
|---|---|
| Owned grant removed; its frames return to the allocator; the owner can no longer free it | `GRANT-RECLAIM-OWNED: PASS` |
| A grant the target *received* (owned by another Cell) survives, still owned by that Cell | `GRANT-RECLAIM-RECEIVED: PASS` |
| A pinned owned grant is withheld, then released by driver acknowledgement | `GRANT-RECLAIM-PINNED: PASS` |

Regression: `S22-RV64-GRANT-REVOKE: PASS` (the pre-existing grant-revoke fixture) and the
VFS lifetime fixture stay green in the same run.

### Deviations from the plan
- The plan's oracle was "the grantee faults on next access". What is asserted instead:
  the entry is gone, the frames are back, the received grant survives, and the pinned case
  quarantines. The fault itself is the same mechanism P03 proves directly on the page
  table (`MMIO-REVOKE-USERBIT`), and a synthetic grantee cannot take a real fault in a
  boot self-test.
