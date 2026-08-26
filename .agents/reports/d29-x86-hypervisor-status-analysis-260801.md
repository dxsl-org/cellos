# D29 — Correct the x86 hypervisor status

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

Spec 05 labels x86_64 an ENOSYS stub. Current x86 code is asymmetric:

- AMD SVM has root enablement, an owner-scoped VM registry, vCPU state/world-switch,
  guest memory mapping, exit conversion, IRQ injection, and an x86 Hypervisor Cell loop.
- Intel VMX performs feature/firmware checks and enters VMX root operation, but VMCS,
  EPT, and guest world-switch remain later work.
- RISC-V remains the unsupported architecture in the common registry.

The architecture overview is likewise stale when it calls all x86 virtualization
"design-plan only". Conversely, AMD SVM is still described elsewhere as an MVP and does
not imply production hardware qualification.

## Recommended ruling [FINAL]

**Approve recommendation A: split status by backend and evidence level.**

1. Mark AMD SVM as implemented MVP, with its registry/run-loop path and current test
   evidence stated explicitly.
2. Mark Intel VMX as root-operation plumbing only; VMCS/EPT/guest execution pending.
3. Keep RISC-V H-extension as unsupported/pending.
4. Remove generic x86 ENOSYS/design-only claims, but do not promote x86 VMM to production
   qualified until real-hardware and lifecycle/security gates pass.
