# Phase 01 — x86 VMM bring-up (SVM-first, PVH, no-LAPIC MVP) + TCG-VMRUN spike

- **Track:** A (finish Tier 3b) · **Label:** **coding** — only the PVH ELF-note parser is truly host-unit-testable; **device model AND world-switch are real-HW / nested-KVM-gated** (device backends only run via world-switch exits) · **Tier:** thinking · **Effort:** XL (~5-6K LOC, estimate NOT trusted until the VMRUN spike passes)

## Context Links
- Continues (0% implemented — greenfield): `.agents/260711-1917-tier3b-x86-vtx/plan.md` (10 phases, full detail there).
- Substrate to mirror: shipped ARM64 EL2 path `kernel/src/hypervisor/registry.rs`, `cells/services/hypervisor/src/run_loop.rs`.
- Scout: [scout-report.md](scout-report.md) — verified `grep -rl svm/npt/vmcb kernel/src` = zero.

## Overview
- **Priority:** P2 · **Status:** pending
- Port the shipped ARM64 KVM-style split to x86_64: kernel owns VMXON/EFER.SVME + VMCS/VMCB + EPT/NPT + world-switch + exit decode (syscalls 220-227, arch-generic); the hypervisor cell adds an x86 personality (PVH loader, 16550 UART, 8259 PIC, 8253 PIT). AMD SVM first (only hardware-virt path QEMU-TCG emulates); Intel VT-x second on a real-HW/KVM lane.

## Key Insights
- **This is NOT "maturation of warm code" — it is a fresh bring-up.** No x86 virt code exists. Treat `.agents/260711-1917` as the authoritative sub-plan; this phase is the wrapper that folds it into the finish-line and labels its HW gates.
- **Red-team M6 correction — device-model is NOT independently TCG-testable.** Device backends receive control ONLY via world-switch exits (PIO/MMIO). If TCG cannot faithfully `VMRUN` a nested-SVM guest and deliver an exit, then the ENTIRE phase (~5-6K LOC) is real-HW-gated with NO CI signal. Only the PVH `vmlinux` ELF-note parsing is genuinely host-unit-testable (it is pure byte-parsing, no VMRUN).
- **MANDATORY early SPIKE (gates the LOC estimate):** before committing the ~5-6K LOC, prove `qemu-system-x86_64 -cpu qemu64,+svm -accel tcg` can (a) execute a trivial guest `VMRUN`, and (b) deliver exactly one PIO exit back to the VMM. If the spike fails, re-scope P01 to a real-HW/KVM-only lane and drop the "TCG CI" premise from `.agents/260711-1917`.
- PVH direct-boot needs the `XEN_ELFNOTE_PHYS32_ENTRY` note, present only in uncompressed `vmlinux`, not the shipped `vmlinuz` — extract/build `vmlinux` (sub-plan P05).

## Requirements
- **Functional:** boot Alpine-x86_64 to a serial shell under `-cpu qemu64,+svm -accel tcg`; virtio-blk→VFS + virtio-net→Net reuse the arch-generic cell stack.
- **Non-functional:** vendor-neutral `ViHypervisor` trait (Law 7) — `X86Svm` ships first, `X86Vmx` second; EPT/NPT + device models + boot protocol are vendor-agnostic.

## Architecture
Data flow mirrors ARM64: guest MMIO/PIO access → EPT/NPT-unmapped-fault (or port-I/O exit) → kernel decodes to `ViVmExit` → cell run loop dispatches to device model → `vcpu_regs` write-back → re-enter. New exit kinds (PortIn/PortOut/Hlt/Msr) require ABI variants.

## Related Code Files
- **Create:** `kernel/src/hypervisor/x86_svm.rs`, `x86_vmx.rs`, `kernel/src/memory/ept.rs` (NPT/EPT builder mirroring `stage2.rs`); `cells/services/hypervisor/src/pvh_loader.rs`, `pic_8259.rs`, `pit_8253.rs`, `uart_16550.rs`.
- **Modify (Law 1):** `libs/api/src/abi/hypervisor.rs` — add `PortIn`/`PortOut`/`Hlt`/`Msr` variants, `VERSION` 1→2.
- **Modify:** `kernel/src/hypervisor/registry.rs` (vendor dispatch), `run_loop.rs` (x86 exit arms).

## Implementation Steps
0. **SPIKE (gate):** TCG-VMRUN + one-PIO-exit proof. Trust the LOC estimate only if it passes; else re-scope to real-HW lane.
1. Execute `.agents/260711-1917` P01-P05 (vendor detect → NPT builder → VMCB/world-switch → ABI extension → PVH boot). **Split each sub-step's validation into host-unit-testable (PVH ELF-note parse ONLY) vs real-HW-only in the sub-plan's phase files.**
2. P06-P08: virtio-mmio on x86 guest, blk→VFS, net→Net.
3. P09: VT-x backend on the real-HW/KVM lane (non-blocking).
4. P10: CI job (SVM-TCG smoke if spike passed) + docs; correct `docs/system-architecture.md` "x86 Working" claim.
5. **ABI ordering:** x86 exit variants take discriminants 8-11; `ViVmExit::S2PermFault` (P05) must append at 12+ afterwards — coordinate the VERSION bump so both land append-only.

## Todo
- [ ] **SPIKE:** TCG VMRUN + one PIO exit (gate the estimate)
- [ ] ABI x86 variants (disc 8-11) + VERSION bump (2× user confirmation)
- [ ] NPT/EPT builder + guest-RAM carve (GPA 0, MMIO unmapped)
- [ ] VMCB world-switch + exit decode (real-HW validation lane)
- [ ] PVH loader (extract vmlinux — ELF-note parse is the only host-unit-testable unit) + 8259/8253/16550 no-LAPIC MVP
- [ ] virtio blk/net reuse; SVM-TCG CI smoke (only if spike passed)

## Success Criteria
- **Spike passes** (TCG VMRUN + PIO exit) OR phase re-scoped to real-HW lane. Alpine-x86 boots to serial shell under SVM (TCG if spike passed, else KVM/real-HW). **Device model AND world-switch marked real-HW-only validation** — only the PVH ELF-note parser has a host-unit test. Do not claim "x86 done" from a TCG pass alone.

## Risk Assessment
- **High:** device-model has no CI signal if TCG VMRUN is infidelitous — the spike exists to surface this before ~5-6K LOC is written. Mitigation: spike-gate; real-HW/KVM lane before any "x86 shipped" claim; keep VMX behind a feature flag.
- **Med:** PVH `vmlinux` extraction fragility → bzImage 64-bit fallback documented in sub-plan.

## Security Considerations
- SAS guest isolation: ViCell frames NEVER mapped into guest EPT/NPT; all GPA→HPA via `checked_add` (mirrors ARM C3). MMIO GPA left unmapped so accesses fault out.
- HypervisorCap gate reused (allowlist bit 44); deny-by-default in dispatch.

## Next Steps
- Track B P04 treats the x86 personality + arch-generic core as the shared VMM core to feature-flag.
