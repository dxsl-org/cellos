# Phase 05b — x86 EPT/NPT CoW-golden parity spec

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design/spec, 0 LOC) · **Tier:** thinking · **Effort:** L · **Law 1: FLAG** (x86 CoW exit variant) · **Depends:** P05 (provenance model + guard rework), P01 (x86 world-switch — real-HW-gated)

## Context Links
- Added per user scope decision #1 (Track B is no longer ARM64-only; budget x86 parity). Resolves red-team C4.
- Mirror of [P05](phase-05-cow-golden-clone-spec.md) for the x86 EPT/NPT substrate that P01 builds.

## Overview
- **Priority:** P2 · **Status:** pending
- Spec the x86_64 equivalent of ARM64 CoW-golden: RO-golden EPT/NPT pages, write-violation fault decode, and the copy-on-write remap — **reusing P05's provenance/refcount model unchanged** (only the fault + page-table mechanics are arch-specific).

## Key Insights
- **CoW is arch-specific, provenance is shared.** P05b defines NO new provenance/refcount type — it consumes P05 §Provenance. It defines only: EPT/NPT RO page bits, x86 write-violation decode, the x86 CoW remap.
- **EPT (Intel):** RO page = R=1,W=0,X per-config; a guest write → EPT-violation VM-exit (basic reason 48) with the **exit qualification** bit distinguishing write (bit1) + "was RO" — decode this to trigger CoW. **NPT (AMD):** guest write to a RO NPT page → `#VMEXIT` NPF (exit code 0x400) with `EXITINFO1` fault bits (P/RW/US/reserved) — RW=1 + P=1 → write to present RO page → CoW.
- Gated on P01: the EPT/NPT builder + world-switch must exist first, and both are real-HW-validated (TCG VMRUN fidelity per P01 spike). So P05b design is now-able but its *implementation* trails P01's real-HW lane.
- The multi-region SAS-guard rework (P05 C1 fix) applies to the EPT/NPT builder too — the x86 nested-paging table needs the same golden-RO ∪ overlay-RW region allowlist.

## Requirements
- **Functional (of the spec):** EPT-violation-qualification + NPT-EXITINFO1 write-to-RO decode; RO-golden EPT/NPT page encoding; x86 CoW remap; confirmation that P05's provenance/refcount model applies unchanged; the x86 CoW ABI variant.
- **Non-functional:** same O(1)-clone / one-copy-per-write cost model as ARM64; the x86 nested-paging builder carries the multi-region guard.

## Architecture (proposed)
Same flow as P05, arch-specific fault + remap:
```
guest write to RO-golden GPA X:
  Intel: EPT-violation (reason 48), qual bit1=write, bit3=readable-but-not-writable → CoW
  AMD:   NPF #VMEXIT (0x400), EXITINFO1 RW=1 & P=1 → CoW
  → allocate frame, copy golden page, remap X writable in EPT/NPT, INVEPT/INVVPID (P06b)
```

## Related Code Files (design targets, no edits)
- Would add: `kernel/src/memory/ept_cow.rs` (mirrors `stage2_cow.rs`); x86 CoW exit variant in `libs/api/src/abi/hypervisor.rs` (Law 1, append-only after P01's variants).
- Reuses: P05 provenance/refcount types; depends on P01's `ept.rs` builder + world-switch; P06b INVEPT/INVVPID + VPID.

## Implementation Steps (design deliverables)
1. EPT-violation qualification decode (Intel) + NPT EXITINFO1 decode (AMD) for write-to-RO-page.
2. RO-golden EPT/NPT page encoding + multi-region guard applied to the nested-paging builder.
3. x86 CoW remap algorithm (reuse P05 provenance model verbatim).
4. ABI delta: x86 CoW exit variant (Law 1, append-only after P01's 8-11 and P05's S2PermFault).
5. Test matrix (parity with P05, x86 flavor): write to RO-golden GPA → one copy; provenance model behaves identically.

## Todo
- [ ] EPT-violation + NPT EXITINFO1 write-to-RO decode
- [ ] RO-golden EPT/NPT encoding + multi-region guard on nested-paging builder
- [ ] x86 CoW remap (reuse P05 provenance)
- [ ] x86 CoW ABI variant (Law 1, append-only)
- [ ] parity test matrix (design)

## Success Criteria
- Spec complete + explicitly reuses P05's provenance model (defines no new one). Marks its implementation as trailing P01's real-HW world-switch. No code/ABI lands.

## Risk Assessment
- **High:** if P01's TCG-VMRUN spike fails, x86 CoW has no CI validation path at all — flag this dependency explicitly.
- **Med:** EPT vs NPT decode divergence (Intel qualification vs AMD EXITINFO1) → vendor-specific handler bugs. Mitigation: vendor-neutral trait, per-vendor decode unit.

## Security Considerations
- Same golden-poisoning threat surface as ARM64 (P08) — the x86 nested-paging RO does NOT protect against the kernel identity-map write path; P08 mitigation is arch-independent.

## Next Steps
- P06b provides the x86 INVEPT/INVVPID + VPID lifecycle this remap requires.
