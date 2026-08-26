# Phase 08 — SECURITY: golden-frame poisoning + lifecycle across ALL teardown paths + restart survival

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design + fault-injection test spec; tests coded later) · **Tier:** thinking (ultra-worthy) · **Effort:** L · **Kernel-Boundary: FLAG** (golden-RO-in-identity-map + kernel-held refcounted registry) · **MANDATORY dedicated phase** · **CONSUMER of P05 §Provenance** · **Depends:** P05, P06

## Context Links
- G5 memory ⚠️ NEW security risk; `project-sas-frame-identity-invariant` (kernel identity-maps freed frames WRITABLE).
- Red-team M1: **provenance/refcount model is owned by P05, NOT here** — P08 consumes it and fixes the security-critical teardown enforcement. Red-team C2: lifecycle beyond clone-Drop.
- Substrate: `stage2.rs` `Drop` (453, frees all guest frames), `registry.rs:531` `reap_vms_for_task` (frees all, no refcount), registry keyed by owner_tid.

## Overview
- **Priority:** P1 (security) · **Status:** pending
- The golden frame set is a shared trust anchor across ALL clones. Spec: (T1) poisoning via the kernel identity map; (T2) shared-frame UAF across ALL teardown paths (Drop + reap + kill + reset), not just clone-Drop; (T3) DMA-path poisoning; and (C2) golden survival across owner-cell restart. Uses the provenance/refcount model **defined in P05 §Provenance** — this phase does not redefine it.

## Key Insights (threat model)
- **T1 — golden poisoning via kernel identity map.** SAS identity-maps every frame RW for the kernel. Stage-2/EPT RO blocks the *guest*, but a stray kernel/EL2 write (bug, or a driver cell with a bad DMA grant) to a golden HPA silently corrupts ALL clones. Blast radius = every tenant sharing G. Traditional hosts keep golden as a file/mmap, not writable-by-default in the VMM's own map.
- **T2 — shared-frame UAF across ALL teardown paths (C2a).** `Stage2Table::Drop` (`stage2.rs:453`) AND `reap_vms_for_task` (`registry.rs:531`) free ALL guest frames unconditionally, consulting no refcount. Kill the golden VM's owner while clones are live → golden frames freed → every clone dangles = cross-tenant UAF. The refcount (P05 §Provenance) must gate Drop, reap, kill, AND reset — every path, not just clone-Drop.
- **C2b — never-die restart wipes the baseline.** The registry is keyed by owner_tid; a hypervisor-cell restart gets a NEW tid → `reap_vms_for_task(old_tid)` frees golden + all clones = instant-restart destroys the golden baseline it exists to protect. Golden ownership must be a kernel-held refcounted registry **decoupled from the transient hypervisor-cell tid**, with a re-attach path after restart.
- **T3 — DMA path bypass.** Even with golden RO in the CPU identity map, a driver cell with a DMA grant covering a golden HPA poisons it via device DMA (IOMMU is the only boundary). Golden HPAs must be excluded from grantable DMA ranges.

## Requirements
- **Functional (of the spec):** T1 mitigation (RO-in-identity-map OR checksum-verify); enforcement that the P05 refcount gates ALL teardown paths (T2/C2a); a kernel-held golden registry surviving owner restart + re-attach (C2b); DMA-exclusion (T3); a fault-injection test spec with measurable pass conditions.
- **Non-functional:** T1 amortized (per-clone verify) or one-time (RO at freeze); does not regress the frame-identity invariant for non-golden frames.

## Architecture (proposed mitigations)
- **T1 — spec both, recommend (a):** (a) **RO-in-identity-map** — at golden-freeze, downgrade the kernel's own identity mapping of golden frames to RO (new privileged `mark_frames_ro(paddr, n)`); any kernel write faults immediately. Preventive. (b) **checksum-verify-before-clone** — hash G at freeze, re-verify before each clone; detective, cheaper. Recommend (a) preventive + (b) defence-in-depth.
- **T2/C2a — refcount gates ALL paths:** Drop, `reap_vms_for_task`, kill, reset each check `Borrowed` vs `Owned` (P05 §Provenance) and decrement G.refcount for borrowed; free golden frames only when refcount hits 0.
- **C2b — restart survival:** golden registry is kernel-held + refcounted, keyed by a stable golden-id (not owner_tid); a restarted hypervisor cell re-attaches by golden-id; `reap_vms_for_task` on the old tid decrements refcounts, never hard-frees golden while refcount > 0.
- **T3 — DMA exclusion:** golden HPAs removed from the grantable-DMA range set; `sys_grant_dma` rejects a golden frame.

## Related Code Files (design targets, no edits)
- Would add: `mark_frames_ro` privileged op (`kernel/src/memory/`); kernel-held refcounted golden registry (decouple from `registry.rs` owner_tid keying); DMA-exclusion check in the grant path.
- Would fix: `Stage2Table::Drop` (453) + `reap_vms_for_task` (531) to respect P05 §Provenance refcount (the T2/C2a bug).

## Implementation Steps (design deliverables)
1. Threat model: T1 poisoning, T2/C2a all-path UAF, C2b restart wipe, T3 DMA bypass — blast radius each.
2. T1 mitigation (a) RO-identity-map (recommend) + (b) checksum (DiD).
3. Enforce P05 refcount across Drop/reap/kill/reset (T2/C2a) — consume, do not redefine, the model.
4. Kernel-held refcounted golden registry + re-attach path (C2b).
5. T3 DMA-exclusion of golden HPAs.
6. **Fault-injection test spec (measurable):**
   - (1) kernel-context write to a golden frame → caught (fault) BEFORE any clone reads corrupted data.
   - (2) kill golden VM's owner with a live clone → golden frames stay allocated (refcount holds).
   - (3) restart hypervisor cell → golden survives + re-attaches by golden-id.
   - (4) `sys_grant_dma` on a golden HPA → rejected.

## Todo
- [ ] threat model (T1/T2/C2a/C2b/T3) with blast radius
- [ ] T1 mitigation (RO-identity-map recommended + checksum DiD)
- [ ] refcount enforcement across ALL teardown paths (consume P05 §Provenance)
- [ ] kernel-held golden registry + restart re-attach (C2b)
- [ ] T3 DMA-exclusion rule
- [ ] fault-injection test spec (4 measurable pass conditions)

## Success Criteria
- Threat model + mitigations complete; **4 fault-injection tests with concrete measurable pass conditions** (golden-write caught; kill-owner-with-live-clone keeps frames; cell restart survives; DMA-grant on golden rejected). Uses P05's provenance model (does not redefine it). No code lands (tests coded when the real-HW testbed exists).

## Risk Assessment
- **High:** T2/C2 (all-path shared-frame UAF + restart wipe) is under-recognized — the memory names only poisoning. Directly UAF-exploitable across tenants and self-inflicted on restart. Mitigation: refcount-gates-all-paths + kernel-held registry are hard prerequisites for ANY CoW coding.
- **High:** SAS gives ONE software boundary (LBI) + HW (S2/IOMMU); cannot stack a 2nd host-process boundary like KVM-on-Linux → less defence-in-depth → mitigation must be preventive (T1a), not just detective.
- **Med:** RO-in-identity-map mis-scope could fault legitimate kernel access. Mitigation: scope strictly to frozen golden frames; test non-golden frames stay RW.

## Security Considerations
- This phase IS the security consideration. Golden integrity + lifecycle across every teardown path is the linchpin of the CoW multi-clone model; without T1+T2+C2+T3 the dual-purpose ROI is unsafe to ship. Arch-independent (applies to ARM64 P05/P06 and x86 P05b/P06b).

## Next Steps
- Gates any CoW coding. When a real-HW virt testbed exists, implement mitigations + fault-injection tests before enabling clone-from-golden.
