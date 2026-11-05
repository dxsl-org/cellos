---
phase: 5
title: "XSAVE/AVX user state on x86_64"
status: pending
priority: P2
effort: "4-7d [inference]"
dependencies: ["R0 (D-R5, D-R6, XSAVE CP1 approved)", "Intel WIP lane", "S1 releases hal/arch/x86/**"]
tier: thinking
checkpoint: true
---

# Phase R5: XSAVE/AVX user state

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) D-R5, D-R6, D-R7; [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §3 row "AVX/YMM user state", §7 Q5
- Intel SDM Vol. 1 ch. 13 (XSAVE feature set) and Vol. 3A §2.5–2.6 (CR4.OSXSAVE, XCR0). These are external specifications, not repo code.
- Lock: [cp1-pcie-irq-power-abi.md](../261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md) §7 (`hal/arch/x86/**` is in S1's handoff)

## Overview

Cellos enables only FXSAVE-class state on x86. AVX, AVX2 and FMA code faults with #UD, and CPUID reports OSXSAVE = 0. Mesa does not need AVX (SSE2/SSE4.1 only). llama.cpp's CPU paths do, for a fair G4 comparison with Linux (D-R7).

R5 enables XSAVE with XCR0 = x87 | SSE | AVX and widens the per-entry save image. It is kernel/HAL work only, with no `libs/api` change. It is not on the G3 critical path.

## Key Insights

**FPU state today:**
- `init_sse` sets CR0 (EM = 0, TS = 0, MP) and CR4 bits 9-10 (OSFXSR, OSXMMEXCPT) only, then `fninit` and MXCSR `0x1F80` (`hal/arch/x86/src/x86_64.rs:554-571`).
- It runs on the BSP (`kernel/src/main.rs:233-236`; `x86_64.rs:485`) and on APs (`x86_64.rs:281`).
- The boot policy requires FXSAVE, SSE and SSE2 (`x86_64.rs:250-251`).

**Entry and exit save a 528-byte FXSAVE64 image** on the current stack:
- syscall path: `hal/arch/x86/src/x86_64/syscall.rs:343-352`;
- interrupt and exception path: `hal/arch/x86/src/x86_64/idt/entry.rs:146-155`.

The context switch keeps only MXCSR and x87 control (`hal/arch/x86/src/x86_64/context.rs:4-8`, `:50`, `:60-63`). User FP state therefore lives only in the entry image on each task's kernel stack. Widening that image is the whole change; the scheduler needs no change.

**IST stacks are sized for the 528-byte area:** about 1 KiB deepest path in 8 KiB, with a measured `ist-hw=` report (`hal/arch/x86/src/x86_64/ist.rs:14-17`).

**PKU is live on entry.** The entries execute `WRPKRU` (`syscall.rs:322-325`; `idt/entry.rs:129-134`). PKRU is XSAVE component 9 (SDM). If it were in the requested-feature bitmap, XRSTOR would overwrite PKRU outside the kernel's own sequence. v1 keeps PKRU out of XCR0 and out of the instruction mask.

**No ABI record describes x86 FPU state.** Grep of `libs/api`, `libs/types` and `docs/abi-boundary-records.md` found only AArch64 setjmp notes (`libs/api/src/services/posix/setjmp.rs:109-116`). Law 1's scope is `libs/api` and `libs/types` (`docs/code-standards.md:42`). The user-visible contract (CPUID.OSXSAVE, XGETBV(0), which registers survive) is why D-R6 recommends a CP1-style check anyway.

**Hypervisor.** The VMX exit reasons `XSETBV` (55) and `XSAVES`/`XRSTORS` (63/64) are defined (`hal/arch/x86/src/x86_64/vmcs.rs:439`, `:447-448`), but no XCR0 handling exists in `kernel/src` or `hal/arch/x86` (grep this session). Once the host enables XSAVE, any guest path that allows XSETBV must swap guest and host XCR0 around entry and exit `[inference: check the SVM MVP path in step 1]`.

**Evidence available locally:**
- QEMU 8.2.2's CPU model list includes `xsave`, `xsaveopt`, `xsavec`, `avx2` and `avx512f` (`qemu-system-x86_64 -cpu help`, this session). TCG executes AVX2 `[inference: QEMU ≥ 7.2 TCG AVX support]`.
- `/dev/kvm` exists, and the host CPU reports `avx2` and `xsave` (`/proc/cpuinfo`).

## Requirements

**Detection:**
- CPUID.1:ECX.XSAVE → set CR4.OSXSAVE (bit 18) on every CPU before any user entry.
- XCR0 = 0x7 if CPUID reports AVX, else 0x3.
- Use XSAVEOPT if present, else XSAVE. No XSAVEC or XSAVES in v1. They need compacted layouts and supervisor states.

**Save image:**
- size = CPUID.(EAX = 0Dh, ECX = 0).EBX for the enabled XCR0, rounded to 64 bytes, 64-byte aligned (about 832 bytes for x87 + SSE + AVX per the SDM `[inference: read at boot, never hard-coded]`);
- EDX:EAX mask = XCR0 (never PKRU);
- a CPU without XSAVE keeps the existing FXSAVE path unchanged.

**Every CPU uses the same XCR0.** An AP whose CPUID differs from the BSP's refuses to come online and logs why (single policy, like `publish_boot_cpu_policy`).

**XRSTOR of a malformed image cannot happen,** because the image is written only by the kernel's own XSAVE. Fresh threads start from a kernel-built init image (the XSAVE header with XSTATE_BV = 0), replacing the `fninit` sequence in `boot.rs:92-106`.

**IST re-measurement:** `ist-hw` must stay below 50 % of the stack.

**Hypervisor:** guest and host XCR0 are isolated on every guest entry and exit.

**Out of v1:** AVX-512 (about 2.7 KiB more per entry), AMX (needs XFD), and CET user state.

## Architecture

```text
entry (syscall | IDT): r12=rsp; align 64; sub XSAVE_SIZE; xsaveopt64 [rsp] (mask=XCR0 & ~PKRU)
  fninit; ldmxcsr 0x1F80   # kernel stays soft-float; sanitize for any FP use
  call dispatch
exit: xrstor64 [r13] (same mask); rsp=r12
fallback (no XSAVE): current fxsave64/fxrstor64 path, byte-for-byte unchanged
```

## Related Code Files

- Modify: `hal/arch/x86/src/x86_64.rs` (`init_sse` → FPU init with OSXSAVE/XCR0, feature policy), `hal/arch/x86/src/x86_64/syscall.rs` (entry/exit image), `hal/arch/x86/src/x86_64/idt/entry.rs` (entry/exit image), `hal/arch/x86/src/x86_64/boot.rs` (fresh-user init image), `hal/arch/x86/src/x86_64/ist.rs` (sizing comment and budget), hypervisor guest entry/exit (VMX/SVM paths, XCR0 swap), `docs/abi-boundary-records.md` (a new row for the user CPU-state contract, if D-R6 is accepted)
- Create: `cells/tests/x86-avx-preserve/**` (witness Cell + runner), this directory's `cp1-x86-xsave-user-state.md` (R0)
- Not touched: `libs/api/**`, `libs/types/**`, target JSONs

## Implementation Steps

1. CP1 (R0) approved and the lane granted. Read the SVM/VMX guest entry for XCR0 exposure and record it.
2. Detection, CR4.OSXSAVE and XCR0 on BSP and APs; boot log `fpu: xsave xcr0=0x7 size=N` or `fpu: fxsave`.
3. Entry and exit image swap on both paths; fresh-thread init image.
4. Hypervisor XCR0 isolation.
5. Witnesses:
   - two Tier-2 threads with distinct YMM0-15 patterns across preemption, syscalls and device interrupts;
   - a `-cpu qemu64,-xsave` run proves the fallback;
   - `ist-hw` report;
   - the existing x86 suites.
6. CP2 with the exact delta and evidence.

## Todo List

- [ ] CP1 approved, lane granted
- [ ] Detection + XCR0 on all CPUs
- [ ] Entry/exit image (syscall + IDT)
- [ ] Fresh-thread init image
- [ ] Hypervisor XCR0 isolation
- [ ] Witnesses (TCG, KVM, no-XSAVE)
- [ ] CP2

## Success Criteria

- [ ] With XSAVE, the YMM witness passes 10 000 preemptions per thread under TCG and KVM, and XGETBV(0) = 0x7 in user mode.
- [ ] Without XSAVE, the boot log shows `fpu: fxsave` and the R1 SSE witness passes.
- [ ] `ist-hw` < 4 KiB, and every existing x86 QEMU suite is unchanged.

## Risk Assessment

- **Kernel-stack growth** by about 300 bytes per entry level, more with nesting. Mitigation: measure; the IST has an 8× margin today (`ist.rs:14-17`).
- **PKRU or CET interaction.** Mitigation: an explicit mask; v1 excludes both from XSAVE.
- **XSAVEOPT's modified-optimisation hazard across CPUs.** Migration with a stale "init" tracking state `[inference: SDM 13.6]`. Mitigation: per-CPU tracking is safe for XSAVEOPT followed by XRSTOR on the same image; fall back to plain XSAVE if the witness shows corruption.
- **Lane contention with S1.** Mitigation: R5 waits. It is not on the G3 path (D-R7).

## Security Considerations

- **Information leak between tasks or Cells through stale YMM upper halves** if any path forgets the AVX component. Mitigation: the same mask on save and restore, an init image for new threads, and a witness that a fresh thread reads zero YMM.
- **Guest/host XCR0 confusion** could let a guest run with host features or the reverse. Mitigation: explicit swap, plus a test in the hypervisor suite.

## Next Steps

G4 builds llama.cpp with `GGML_AVX2=ON` only after R5 lands. Until then, `GGML_AVX*=OFF` and SSE ≤ 4.2 (port gaps §3).

## Deviation Log

None.
