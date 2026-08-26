# Phase 06b — x86 INVEPT/INVVPID + VPID lifecycle parity spec

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design/spec, 0 LOC) · **Tier:** thinking · **Effort:** M · **Kernel-Boundary: FLAG** (privileged TLB/VPID mechanism) · **Depends:** P06 (ARM64 lifecycle pattern), P05b (x86 CoW)

## Context Links
- Added per user scope decision #1. x86 analog of [P06](phase-06-reset-to-golden-spec.md)'s VMID + S2-TLB work.
- Mirrors the ARM64 VMID/tlbi fixes (C3 + TLB primitive) onto Intel VPID + AMD ASID.

## Overview
- **Priority:** P1 (correctness/security) · **Status:** pending
- Spec the x86 cache-invalidation + tag lifecycle for CoW reset and clone teardown: `INVEPT` (nested-paging TLB), `INVVPID` (linear-address TLB per VPID), and a VPID (Intel) / ASID (AMD) free-list mirroring the ARM64 VMID fix.

## Key Insights
- **The ARM64 C3 (VMID wrap → cross-VM leak) has an exact x86 twin.** Intel VPID and AMD ASID are the tags that scope cached translations per-VM; a monotonic allocate-only counter wraps and reuses a live tag → stale EPT/NPT + linear TLB entries match across VMs. Same free-list/generation + flush-before-reuse rule.
- **Two invalidation instructions, not one:** `INVEPT` (single-context type 1, by EPTP) flushes EPT-derived translations; `INVVPID` (single-context type 1, by VPID) flushes linear-address translations tagged by that VPID. A CoW remap or reset re-point needs BOTH on Intel; AMD uses TLB_CONTROL / ASID flush in the VMCB. Spec the per-vendor mapping.
- Reset/remap must issue single-context invalidation (not global) for the affected EPTP/VPID after re-pointing a golden page.

## Requirements
- **Functional (of the spec):** VPID/ASID free-list + generation + flush-before-reuse; `INVEPT`/`INVVPID` single-context contract (Intel) + VMCB TLB_CONTROL/ASID flush (AMD); when each fires in the CoW remap + reset paths.
- **Non-functional:** single-context (not global) invalidation for cost; mirrors P06 ARM64 semantics so the provenance model behaves identically.

## Architecture (proposed)
```
alloc_vpid()/alloc_asid(): free-list + generation (mirror P06 VMID)
free_vpid(v): INVEPT(single-context, EPTP) + INVVPID(single-context, v)  BEFORE reuse   // Intel
free_asid(a): VMCB TLB_CONTROL flush-this-ASID  BEFORE reuse                            // AMD
CoW remap / reset re-point of GPA X:
  Intel: after remap → INVEPT(EPTP) [+ INVVPID if linear cached]
  AMD:   after remap → set VMCB TLB_CONTROL flush-by-ASID on next VMRUN
```

## Related Code Files (design targets, no edits)
- Would add: `INVEPT`/`INVVPID` intrinsics + VPID/ASID free-list in the x86 hypervisor kernel module (P01's `x86_svm.rs`/`x86_vmx.rs`); wiring in `ept_cow.rs` (P05b).
- Mirrors: P06 VMID lifecycle + tlbi primitive.

## Implementation Steps (design deliverables)
1. VPID (Intel) / ASID (AMD) free-list + generation + flush-before-reuse (mirror P06 C3 fix).
2. `INVEPT` + `INVVPID` single-context contract (Intel); VMCB TLB_CONTROL/ASID flush (AMD).
3. When each invalidation fires in the CoW remap + reset re-point paths.
4. Test matrix (parity with P06): VPID/ASID recycle after teardown → no stale match; post-reset write re-faults on x86.

## Todo
- [ ] VPID/ASID free-list + generation + flush-before-reuse
- [ ] INVEPT/INVVPID single-context (Intel) + VMCB flush (AMD)
- [ ] invalidation firing points in remap/reset
- [ ] parity test matrix (design)

## Success Criteria
- Spec complete + mirrors P06 semantics per-vendor. Marks implementation as trailing P01/P05b real-HW lane. No code lands.

## Risk Assessment
- **High:** VPID wrap = the same cross-VM leak as ARM64 C3; must not ship x86 CoW without the free-list. Mitigation: hard gate, recycle test.
- **Med:** Intel needs both INVEPT + INVVPID; missing one leaves stale linear translations. Mitigation: explicit per-instruction contract + test.

## Security Considerations
- Cross-VM isolation on x86 depends entirely on correct VPID/ASID lifecycle + single-context invalidation. Feeds P08 (arch-independent poisoning mitigation still applies).

## Next Steps
- Completes the x86 CoW correctness substrate; P08 security is arch-independent and covers both.
