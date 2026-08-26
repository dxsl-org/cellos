# Phase 07 — vCPU + device-state save/restore split + ARM64 snapshot SPIKE + validated restore

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design/spec + a coding SPIKE on the shipped ARM64 guest) · **Tier:** thinking · **Effort:** L (0 LOC design + SPIKE) · **Law 1: FLAG** (`sys_snapshot_vcpu`/`sys_restore_vcpu`) · **HIGHEST-UNCERTAINTY item — do NOT bucket into the flat design estimate** · **Depends:** P05 (provenance), P03 (MemBackend validator)

## Context Links
- G5 memory lever 1: "vCPU + device state save/restore = fiddly 20%". Red-team M8: the "20%" **understates** — `vcpu_regs` today captures ≈1/10 of even the register surface.
- Substrate: `registry.rs:354-385` `vcpu_regs` copies ONLY 32×u64 (GPRs `gp[0..31]` + `g_elr_el2`); `run_loop.rs` device state in cell structs; P03 `MemBackend` validator.

## Overview
- **Priority:** P2 · **Status:** pending
- Enumerate exactly what non-RAM state a clone/reset must snapshot; mark this as the highest-uncertainty item; require an ARM64 SPIKE (not armchair spec) for the consistency contract; and route restore through P03's validator with cross-surface rollback.

## Key Insights
- **`vcpu_regs` captures ≈1/10 of the register surface (M8).** Verified `registry.rs:354-385` = GPRs x0-x30 + `g_elr_el2` only. **MISSING:** SPSR/PSTATE, SCTLR_EL1, TTBR0_EL1/TTBR1_EL1, TCR_EL1, MAIR_EL1, VBAR_EL1, SP_EL0/SP_EL1, TPIDR_EL0/1, CNTV/CNTP timer (CVAL/CTL/offset), and ALL vGIC state (GICH LR ×n, VMCR, active/pending). A snapshot built on today's `vcpu_regs` would restore a guest that faults immediately (no SCTLR/TTBR) — this is why it is understated, not fiddly.
- **Restore must reuse the P03 validator (M5).** P07 restore reconstructs virtqueue indices directly into cell device structs, skipping the live-notify `cur<q_size` clamp → first post-restore QueueNotify walks OOB. Route restore through the same P03 `MemBackend` validator as the notify path.
- **Two-surface restore has no rollback (M5).** Kernel `sys_restore_vcpu` (vCPU blob) + cell `DeviceSnapshot` are separate; device restored but vCPU rejected = inconsistent guest. Validate-all (especially the kernel vCPU blob, treated as untrusted input) BEFORE mutating any cell-side device state; define abort/rollback.
- **Virtio queue-index coherence is the hardest sub-problem:** indices live BOTH in guest RAM (descriptor rings, part of the CoW set) AND cell struct state (negotiated features, ready flags) — must be captured at one quiesced instant.

## Requirements
- **Functional (of the spec):** the FULL kernel-side vCPU/vGIC/timer register inventory (corrected from M8); the cell-side per-backend device inventory; the RAM↔device consistency contract; the validated-restore path (P03 validator + cross-surface rollback); the ABI delta.
- **Non-functional:** snapshot only at a quiesced point; restore idempotent + validate-before-mutate.

## Architecture (proposed)
Two save/restore surfaces at one quiesced instant: (1) kernel-side full vCPU/vGIC/timer via new `sys_snapshot_vcpu`/`sys_restore_vcpu` (Law 1) — **extend `vcpu_regs` to the full sysreg set or a dedicated blob**; (2) cell-side `DeviceSnapshot` (no ABI, cell-internal). Restore: validate BOTH blobs (kernel vCPU via bounds/canonicalization, device indices via P03 `MemBackend`) → only then apply → rollback on any rejection.

## Related Code Files (design targets, no edits)
- Would add: kernel snapshot syscalls (Law 1); cell `device_snapshot.rs`.
- Would extend: `vcpu_regs` (`registry.rs:354`) to the full register surface (or a dedicated snapshot path); restore path routed through P03 `MemBackend`.

## SPIKE (required — not armchair spec)
Before finalizing the consistency contract, run a snapshot/restore SPIKE on the shipped ARM64 Alpine guest: snapshot at a quiesced point, restore into a fresh vCPU/table, confirm the guest resumes correctly. This surfaces the true missing-state set empirically (M8). The SPIKE is small coding on the shipped guest — the only Track-B item with a code component.

## Implementation Steps (design deliverables)
1. FULL kernel-side vCPU/vGIC/timer inventory (correct M8 gaps) with register list.
2. Cell-side per-backend device inventory (PL011, virtio-mmio×N, blk, net, timer; x86 PIC/PIT).
3. RAM↔device consistency contract (quiesce point, virtio-index coherence).
4. Validated-restore path: P03 `MemBackend` for device indices + kernel-blob validation + cross-surface rollback (M5).
5. ABI delta for `sys_snapshot_vcpu`/`sys_restore_vcpu` (Law 1, for approval).
6. Run the ARM64 SPIKE; feed empirical missing-state findings back into (1)-(3).
7. Uncertainty ranking + staged deliverable (RAM-CoW + GPR/timer first; full device snapshot second).

## Todo
- [ ] FULL kernel-side vCPU/vGIC/timer inventory (M8)
- [ ] cell-side per-backend device inventory
- [ ] RAM↔device consistency contract (quiesce)
- [ ] validated restore (P03 validator + rollback, M5)
- [ ] snapshot/restore ABI delta (Law 1)
- [ ] ARM64 snapshot SPIKE (coding)
- [ ] uncertainty ranking + staged deliverable

## Success Criteria
- Complete inventory (corrected past the M8 understatement) + consistency contract + validated-restore design + ABI delta, explicitly highest-uncertainty. The ARM64 SPIKE has run and its empirical missing-state findings are folded in. No production code/ABI lands (SPIKE is throwaway).

## Risk Assessment
- **High (uncertainty):** missing one piece of vCPU/device state → clone boots but diverges (immediate fault, clock jump, stuck queue). Mitigation: SPIKE surfaces the real set; staged deliverable ships RAM-CoW value before full snapshot.
- **High:** restore of attacker-crafted blob without validation → guest/host compromise. Mitigation: validate-before-mutate via P03 validator + kernel-blob canonicalization.
- **Med:** snapshot at non-quiesced point → torn virtio state. Mitigation: quiesce precondition shared with P06.

## Security Considerations
- Snapshot blobs cross the kernel↔cell boundary; restore input is untrusted (bounds, register canonicalization, virtqueue clamp). Feeds P08.

## Next Steps
- Gates the sub-10ms headline (snapshot-resume). P08 covers the security of the shared golden bundle across all teardown paths.
