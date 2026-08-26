# Phase 06 — Reset-to-golden + VMID lifecycle + S2 TLB-invalidation + zero-on-free + atomic reset (ARM64)

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design/spec, 0 LOC) · **Tier:** thinking · **Effort:** L · **Kernel-Boundary: FLAG** (new EL2 TLB-invalidation + VMID-recycle mechanisms — privileged, kernel-side) · **Depends:** P05 (provenance model)

## Context Links
- G5 memory lever 1 (reset half). Consumes P05 §Provenance. x86 parity = [P06b](phase-06b-x86-invept-vpid-spec.md).
- Substrate: `stage2.rs` `unmap_single` (428, clears desc, **no free, no TLB invalidate**), `Drop` (453), MMIO doc warning (68). VMID: `registry.rs:58` `AtomicU16::new(1)`, `:105` `fetch_add` allocate-only. Frame: `frame.rs:120,142` bitmap-only (no zero).

## Overview
- **Priority:** P1 (correctness/security) · **Status:** pending
- Spec reset-to-golden: drop the dirty overlay + re-point IPAs back to golden RO → O(dirty pages), no re-zero/re-boot. **Plus the three correctness/security fixes reset depends on: a VMID lifecycle (C3), the S2 TLB-invalidation primitive, zero-on-free of overlay frames (M2), and a transactional/atomic reset (M4).**

## Key Insights
- **TLB gap is correctness AND security, not perf.** `unmap_single` never issues `tlbi`; **no `tlbi` primitive exists in `stage2.rs`.** Re-pointing a dirty IPA to golden-RO while a stale RW TLB entry survives → guest keeps writing the freed/reused overlay frame → silent cross-clone leak.
- **C3 — VMID wraps and is never recycled.** `AtomicU16::new(1)` + allocate-only `fetch_add` → a fast clone fleet wraps u16 → reuses a live VMID → stale S2 TLB entries match across VMs → cross-VM read/write. VMID lifecycle is a hard dependency of the TLB primitive (a recycled VMID MUST be TLB-flushed before reuse).
- **M2 — frames are not zeroed on free** (`frame.rs` bitmap-only; SAS frame-identity keeps contents). Reset frees overlay to the general pool unzeroed → next tenant's `carve_guest_ram` reads the prior clone's secret. The "no re-zero" speed claim applies ONLY to the RO-golden re-point, NEVER to frames leaving VM ownership.
- **M4 — reset must be atomic.** Mid-loop kill/panic frees some overlay before `overlay.clear()` → double-free or half-golden/half-freed corrupt guest, no rollback.

## Requirements
- **Functional (of the spec):** the VMID free-list/generation lifecycle; the VMID-scoped S2 TLB-invalidation primitive (per-IPA `ipas2e1` + full-VM `vmalls12e1is`); zero-on-free (or zero-on-carve) rule for overlay frames; the transactional reset algorithm; interaction with `Drop` (reset ≠ Drop); vCPU quiesce precondition.
- **Non-functional:** O(dirty pages); crash-consistent (kill at any step leaves a recoverable state).

## Architecture (proposed)
```
// VMID lifecycle (C3)
alloc_vmid(): pop free-list OR bump; carry a generation counter
free_vmid(v): tlbi vmalls12e1is(v)  BEFORE returning v to the free-list  // no stale match on reuse

// S2 TLB primitive (new) — VMID-scoped
tlbi_ipas2e1(vmid, ipa)          // per-page (CoW remap, reset re-point)
tlbi_vmalls12e1is(vmid)          // whole-VM (teardown, VMID recycle)
   with DSB ISH; ISB ordering

// transactional reset (M4) — consumes P05 §Provenance
reset_to_golden(clone):
  quiesce vCPU (single-thread vCPU invariant)
  build new-mapping-set (all overlay IPAs → golden RO)   // stage, don't mutate yet
  apply swap; for each: tlbi_ipas2e1(vmid, ipa)
  free Owned overlay frames — ZERO each on free (M2); never touch Borrowed golden
  overlay.clear()
  // kill between "apply swap" and "free" → frames still tracked → reap zeroes them (no double-free)
```

## Related Code Files (design targets, no edits)
- Would add: `tlbi_ipas2e1`/`tlbi_vmalls12e1is` primitive (`stage2.rs` or `stage2_tlb.rs`); VMID free-list/generation in `registry.rs` (replace `NEXT_VMID` allocate-only); `reset_to_golden` in `stage2_cow.rs`; zero-on-free in `frame.rs` deallocate path (or zero-on-carve in `allocate_guest_ram`).
- Would fix: `unmap_single` TLB gap.

## Implementation Steps (design deliverables)
1. VMID free-list/generation lifecycle (C3); flush-before-reuse rule.
2. VMID-scoped S2 TLB-invalidation primitive (per-IPA + whole-VM) + DSB/ISB ordering.
3. Zero-on-free (or zero-on-carve) rule for guest-RAM frames leaving VM ownership (M2); scope the "no re-zero" claim to RO-golden re-point only.
4. Transactional reset algorithm (stage → swap → tlbi → zero+free overlay) with crash-consistency (M4).
5. Reset ≠ Drop: reset frees Owned only; Drop decrements golden refcount (per P05 §Provenance).
6. Test matrix: reset restores golden byte-for-byte; post-reset write re-faults (TLB invalidated); reset of clone A leaves clone B intact; VMID recycle after teardown → no stale TLB match; freed overlay reads zero; kill-injection at each reset step → no double-free / recoverable.

## Todo
- [ ] VMID free-list/generation + flush-before-reuse (C3)
- [ ] S2 TLB-invalidation primitive (per-IPA + whole-VM + DSB/ISB)
- [ ] zero-on-free / zero-on-carve rule (M2)
- [ ] transactional reset + crash-consistency (M4)
- [ ] reset-vs-Drop distinction (consume P05 §Provenance)
- [ ] test matrix (design)

## Success Criteria
- Spec complete without re-deriving the TLB/VMID contracts. Explicitly names the missing `tlbi` primitive + VMID recycle as CoW-soundness blockers. Zero-on-free and atomic-reset specified with tests. No code lands.

## Risk Assessment
- **High:** reset without TLB invalidation OR VMID recycle → silent cross-clone/cross-VM leak. Mitigation: both are hard gates; reset-then-write test must observe a re-fault; VMID-reuse test must show a flush.
- **High:** unzeroed overlay to general pool → cross-tenant secret disclosure. Mitigation: zero-on-free mandatory for frames leaving VM ownership.
- **Med:** non-atomic reset races kill. Mitigation: transactional staging + crash-consistency test.

## Security Considerations
- Cross-clone/cross-VM/cross-tenant isolation depends on: correct TLB invalidation + VMID recycle + zero-on-free + atomic reset. All feed P08's threat model; P08 consumes this phase.

## Next Steps
- P06b provides the x86 INVEPT/INVVPID + VPID analog. P07 completes reset (vCPU/device state). P08 audits the frame-lifecycle safety across ALL teardown paths.
