# Slice VH — VMX host-state repair (Intel WIP lane)

**Status: closed 2026-11-05.** Recorded 2026-11-05. After 04b closed, the owner (the user) chose **"Sửa VMX -cpu host trước"**: this repair takes the Intel WIP lane before S2. The choice was offered with this file set as the required handoff, so the handoff is approved with it.

**Defect** ([report](../debug/debug-261105-1036-kvm-host-vmx-boot-exit.md)): under KVM `-cpu host` a test-hooks x86 kernel runs the VMX smoke (`x86_smoke`) and triple-faults. In `hal/arch/x86/src/x86_64/vmx_vcpu.rs` the host GS base is written from the low 32 bits only, `HOST_TR_BASE` is never written, and `EXIT_ACKNOWLEDGE_INTERRUPT` acknowledges an external interrupt that no handler then EOIs. Present since `15830f4fd`.

**File set:** `hal/arch/x86/src/x86_64/vmx_vcpu.rs`, `hal/arch/x86/src/x86_64/gdt.rs` (new `tss_base()`), `kernel/src/hypervisor/svm_registry.rs` (smoke loop opens an interrupt window on `Preempted`), plus this record, the TODO entry and the changelogs. No `libs/api`/`libs/types` change; regression/security repair, not a Law-1 delta.

**Acceptance:** under KVM `-cpu host` the mlibc, cpp-hosted and drm-lite witnesses pass and the boot logs `X86-VMM-SMOKE: PASS`; the same runners pass on TCG and KVM `qemu64,+pdpe1gb`; `hal-x86` and kernel host tests pass; `cellos-sign --check` clean; 04b driver lanes still pass.

**Not in scope:** the SVM-guest triple fault the interrupt window exposes on TCG (`[hv-x86] guest triple-fault: rip=0x6663`); it stays a TODO item.

## Result — closed 2026-11-05 (commit `7b8f42eea`)

Acceptance met; [evidence](../../docs/evidence/runtime-witnesses-x86-261105.txt):
- KVM `-cpu host`: `root operation active (VMX)`, `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)`, `MLIBC-WITNESS: PASS`, `CPP-HOSTED-SMOKE: PASS`, `DRM-LITE-WITNESS: PASS`.
- The same runners pass on KVM `qemu64,+pdpe1gb` and on TCG.
- Host: `hal-x86` 25/25, kernel 432/432; `cellos-sign --check` F1 OK.
- 04b driver lanes on the fixed tree, TCG and KVM `-cpu host`: nvme 11/11, ahci 15/15, xhci 4/4, iommu-dmar 3/3, virtio 4/4, nic 2/2, igb 2/2, serial 1/1; a manual headless (non-test-hooks) KVM `-cpu host` boot reaches `Cellos >` with VMX root active and no reset.

Carried open (`.agents/TODO.md`): the SVM smoke guest triple-faults on TCG (`rip=0x6663`); the smoke only logs. Evidence ceiling: QEMU 8.2.2, KVM nested under Hyper-V (WSL2); no bare-metal VMX. The slice releases the Intel WIP lane; S2 is next and still needs H-S2-2…H-S2-11.
