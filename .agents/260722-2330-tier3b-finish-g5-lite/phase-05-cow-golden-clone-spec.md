# Phase 05 — CoW-golden clone spec (ARM64) — OWNS provenance model + SAS multi-region guard rework

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design/spec, 0 LOC) · **Tier:** thinking · **Effort:** XL · **Law 1: FLAG** (likely new hypervisor syscall + `ViVmExit::S2PermFault` variant — design surfaces the ABI delta, commits nothing)

## Context Links
- G5 memory `project-g5-dual-profile-vm` lever 1 (strongest lever). x86 parity = [P05b](phase-05b-x86-cow-parity-spec.md).
- Substrate: `kernel/src/memory/stage2.rs` (read in full) — `S2_S2AP_RO` (line 38), `page_desc(pa, writable)` (line 102), single-region SAS guard (274-279), `map()` (246), `Drop` (453). Frame layer `frame.rs:120,142,339`.
- **THE Track-B hub. Sole owner of the provenance/refcount model consumed by P06/P07/P08.**

## Overview
- **Priority:** P2 · **Status:** pending
- Spec the ARM64 CoW-golden clone: boot a golden VM once → keep its guest-RAM frames as a read-only golden set G → each clone = a fresh `Stage2Table` mapping guest-IPA → G with `writable=false`; a guest write triggers a **new EL2 stage-2 permission-fault handler** that allocates a fresh frame, copies the golden page, remaps that IPA writable. **This phase re-architects the SAS isolation guard (its current single-region form cannot express a clone) and defines the frame-provenance/refcount model the rest of Track B depends on.**

## Key Insights
- **Correction to scout (red-team C1, 4/4 reviewers): the CoW substrate does NOT exist — only the RO/RW descriptor bits do.** The `map()` SAS guard (`stage2.rs:274-279`) checks ONE contiguous carved region and is SKIPPED when `guest_ram_pages==0`. A clone must map borrowed-golden frames (**another VM's region**) plus its own scattered overlay frames = two disjoint regions the single-region model cannot express → `SasViolation` on the golden map, or (if the clone does not carve) the guard is fully bypassed = **SAS escape**. This guard rework is THE CoW substrate blocker and is larger than P08's Drop fix.
- Missing exit path: today's `ViVmExit` covers MmioRead/Write (unmapped IPA data-abort) but NOT a permission fault on a *mapped-RO* page. Need ESR_EL2 decode: EC=0x24 (data abort from lower EL), DFSC = `0b0011xx` permission-fault class, WnR=1 (write).
- Frames are NOT zeroed on free (`frame.rs` bitmap-only) — relevant to overlay lifecycle (P06 owns the zeroing rule).

## Provenance/refcount model (THIS PHASE IS ITS SOLE OWNER — P06/P07/P08 consume)
Define the concrete types + rules; everything else references "P05 §Provenance":
- **Frame provenance tag:** each guest-mapped frame is either `Borrowed(golden_id)` (RO, shared, NOT owned by this table) or `Owned` (RW overlay, freed by this table).
- **Golden set G:** a kernel-held, **refcounted** structure `{frames: [PAddr], refcount: usize, generation: u32}`. `refcount` = number of live clones + the golden VM itself. Decoupled from any transient owner tid (P08 uses this for restart survival).
- **`Stage2Table` gains a per-table region allowlist** (replaces the single `(guest_ram_pa, guest_ram_pages)` pair): `Vec<HpaRegion { base, len, perm: RO|RW, provenance }>`. `map(writable=false)` may target a golden RO region; `map(writable=true)` only an owned RW overlay region; anything else → `SasViolation`. `guest_ram_pages==0` NEVER means "no check".
- **Drop rule:** free ONLY `Owned` frames; decrement G.refcount for `Borrowed`; never free golden frames directly (P06 reset + P08 all-path teardown enforce this across reap/kill/restart too).
- **Overlay quota:** each clone carries a per-clone overlay watermark (max owned frames) so one clone cannot starve the fleet (M3).

## Requirements
- **Functional (of the spec):** ESR_EL2 permission-fault decode; the CoW page flow; the multi-region SAS guard rework; the provenance/refcount type definitions; allocator-exhaustion policy; lock order; the ABI delta.
- **Non-functional:** clone O(1) at creation (map golden RO, no copy); write cost = one page copy + one TLB invalidate (P06 primitive); SAS multi-region guard holds for both golden RO and overlay RW.

## Architecture (proposed)
```
golden VM boot ──► freeze ──► G {frames RO, refcount=1, generation}
clone_from_golden(G):
  tbl = Stage2Table::new()
  tbl.add_region(golden.base..len, RO, Borrowed(G.id)); G.refcount += 1
  tbl.carve_overlay(watermark) → add_region(overlay, RW, Owned)
  for page: tbl.map(ipa, G[ipa], writable=false)      // borrowed RO
guest write ipa X ──► EL2 S2 perm-fault (EC=0x24, WnR=1, DFSC=perm):
  // lock order: acquire FRAME_ALLOCATOR, drop registry_lock BEFORE alloc (m1)
  if overlay.count >= watermark → exhaustion policy (M3)
  F = allocate_guest_ram(1); if None → exhaustion policy (M3, no panic)
  copy G[X] → F; tbl.map(X, F, writable=true); tlbi ipas2e1(X)   // P06 primitive
  overlay.insert(X, F)
```

## Related Code Files (design targets, no edits)
- Would add: `kernel/src/memory/stage2_cow.rs` (multi-region guard, provenance types, perm-fault apply); `ViVmExit::S2PermFault` in `libs/api/src/abi/hypervisor.rs` (Law 1, **disc 12+ AFTER P01's x86 variants at 8-11**, m2); handler wiring in `registry.rs`/`run_loop.rs`.
- Would rework: `stage2.rs` `map()` guard (274-279) + `Stage2Table` fields.
- Depends on: P06 TLB primitive + VMID lifecycle; consumed by P06/P07/P08.

## Implementation Steps (design deliverables)
1. Re-architect the SAS guard → multi-region HPA allowlist (C1) — the substrate blocker; spec first.
2. Define the provenance/refcount model (types + Drop/borrow rules + overlay quota) — SOLE OWNER.
3. ESR_EL2 permission-fault decode spec.
4. CoW apply algorithm incl. **allocator-exhaustion policy** (graceful clone-fail vs guest fault, no panic) + per-clone watermark (M3).
5. Lock order: FRAME_ALLOCATOR → registry_lock; handler drops registry_lock before overlay alloc (m1).
6. ABI-delta: new syscall (`sys_clone_vm_from_golden`) options + `S2PermFault` variant at disc 12+ (Law 1, for approval; P05→P01 ABI-ordering edge).
7. Test matrix: clone boots identical to golden; first write = exactly one copy; N clones share G with O(dirty) memory; golden map with `writable=true` rejected; overlay watermark exceeded → clean fail; allocator-None → bounded non-panic.

## Todo
- [ ] SAS multi-region HPA-allowlist guard rework (C1 — substrate blocker)
- [ ] provenance/refcount model (types + Drop/borrow rules + watermark) — SOLE OWNER
- [ ] ESR_EL2 perm-fault decode spec
- [ ] CoW apply + exhaustion policy (M3, no panic)
- [ ] lock order FRAME_ALLOCATOR→registry_lock (m1)
- [ ] ABI delta (Law 1; S2PermFault disc 12+ — m2)
- [ ] test matrix (design)

## Success Criteria
- Spec executable by `/hc-cook` without re-deriving the guard rework, fault decode, or provenance model. The provenance model is unambiguously owned here. Law-1 ABI delta explicit and awaiting approval. No code/ABI lands.

## Risk Assessment
- **High:** the single-region guard is a live SAS-escape hazard for any clone — the multi-region rework is a hard prerequisite, not an optimization. Correct the scout's "substrate EXISTS" claim wherever it appears.
- **High:** CoW is unsound without P06 (TLB + VMID) AND P08 (all-path refcount) — state both as hard dependencies.
- **High (highest uncertainty is P07):** RAM CoW is the easy 80%; vCPU+device state is the fiddly 20%.

## Security Considerations
- The provenance/refcount model IS the security substrate — P08 threats (poisoning, cross-path UAF) are mitigated by rules defined HERE. Security is fixed with the mechanism, not after (red-team M1).
- SAS multi-region guard must reject overlay HPA outside the clone's carved region AND reject any RW map into a golden region.

## Next Steps
- P05b (x86 EPT/NPT parity) reuses this provenance model with arch-specific fault/TLB. P06 (reset + TLB + VMID), P07 (snapshot), P08 (security) all consume P05 §Provenance.
