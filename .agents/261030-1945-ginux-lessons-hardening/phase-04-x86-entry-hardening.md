---
phase: 4
title: "x86 privileged-entry hardening"
status: pending
priority: P1
effort: "1.5-2w"
dependencies: [2, 3]
tier: thinking
---

# Phase 04: x86 privileged-entry hardening

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

The scout confirmed these gaps:

- Every IDT gate uses `ist: 0` (`hal/arch/x86/src/x86_64/idt.rs:51-60,80-87`).
- An NMI or #MC between `swapgs` and the kernel RSP load (`syscall.rs:229-234`) runs at CPL0 on the user-controlled stack.
- #DF from a kernel stack overflow has no good stack.
- SMEP/SMAP/UMIP are off, and `IA32_FMASK` is `0x0300` (IF+DF), so AC survives SYSCALL (`syscall.rs:153`).

User-copy fault recovery (F2) already exists; this phase extends it only for SMAP.

## Requirements

- Functional:
  - Full architectural TSS (`rsp[3]`, `ist[7]`, iomap base). Per CPU (`MAX_CPUS`), three distinct IST stacks: IST1 NMI, IST2 #DF, IST3 #MC. Each is 16 KiB with an unmapped 4 KiB guard page below and above.
  - IDT vectors 2/8/18 use IST1/2/3. Frame decoding treats IST vectors as always carrying saved RSP/SS, even when the saved CS is CPL0. The current `EntryFrame::old_rsp` decodes RSP only on a privilege change (`idt/entry.rs:29-45`).
  - **Paranoid GS** for NMI and #MC: decide kernel vs user GS by reading `IA32_GS_BASE` against the per-CPU kernel base, not from the saved CS. Swap on entry only if needed; restore exactly on exit.
  - Policy decision **D-04a**: NMI = diagnostic marker + IRET (it is a fatal halt today). #DF and #MC stay fatal halt, with a dump (vector, RIP, decoded old RSP, CR2; MCG_STATUS/MCi_STATUS for the reporting bank).
  - AC is clear at every kernel entry:
    - `IA32_FMASK` adds bit 18.
    - The common IDT entry clears AC in live RFLAGS before any Rust code runs, with a sequence that is valid on CPUs without SMAP (`pushfq; andq $~0x40000,(%rsp); popfq`). Saved user RFLAGS stays intact for IRET.
  - SMEP and UMIP enabled when CPUID reports them (no SAS dependency).
  - SMAP (**hard gate, D-04b**) is enabled only after every x86 kernel access to U=1 memory outside the guarded copy is migrated to `TaskCopyView`/guarded copies. Known examples:
    - non-native-domain raw slices (`copy_glue/mod.rs:229-245,280-290`);
    - `vcpu_regs` (`kernel/src/hypervisor/svm_registry.rs:483-503`).

    Until the migration is complete, SMAP stays off in **every** profile that admits Tier-1 SAS Cells. `native-domains` is a default feature and still admits Tier-1 device Cells (`kernel/Cargo.toml:70,133-138`, `loader/domain_admission.rs:179-201`), so it is not a safety boundary.
  - `stac`/`clac` scoped to the guarded copy commit only; the #PF landing pad executes `clac`.
- Non-functional: no `libs/api` change. User-copy error stays `ViError::InvalidInput`.

## Architecture

The SYSCALL instruction sequence is unchanged (no push/call before `mov %gs:0,%rsp`). Its window is made safe by IST plus paranoid GS. Nested NMIs are avoided by construction: the NMI handler takes no faults and never executes `iretq` from a nested exception. A future NMI watchdog/IPI user must add the Linux nested-NMI protocol first; this constraint is documented in Spec 02.

## Assumptions

- **Claim:** QEMU monitor `nmi` and `mce` inject on q35 TCG and KVM. **Confidence:** medium. **How to verify:** run `nmi` and `mce 0 1 0xb200000000000000 0x5 0 0` against the current IDT image; observe the existing fatal markers.
- **Claim:** a 4 KiB guard can be unmapped inside a static stack reservation in the x86 kernel mapper. **Confidence:** medium. **How to verify:** read the mapper used for task stack guards (`kernel/src/task/stack.rs`).
- **Claim:** the SMAP migration set is small enough to finish in this phase. **Confidence:** low. **How to verify:** the step 1 inventory. If it is large, SMAP enablement is split into its own phase (recorded as a deviation), and SMEP/UMIP/IST/AC still land here.

## Related Files

- Modify: `hal/arch/x86/src/x86_64/{gdt.rs,idt.rs,idt/entry.rs,idt/dispatch.rs,idt/policy.rs,idt/fatal.rs,syscall.rs,boot.rs}`, `hal/arch/x86/src/x86_64.rs`, `kernel/src/main.rs` (init order), `kernel/src/task/user_copy/guard.rs`, `kernel/src/task/copy_glue/mod.rs` and every SAS raw-access site found in step 1, `scripts/mutable-globals.toml`.
- Runners: `scripts/qemu-x86_64-idt-test.sh` (QEMU monitor socket + injection), `scripts/x86/qemu-domain-test.sh` (SMAP/SMEP/UMIP cases, Tier-1 device and hypervisor Cells included).
- Docs: `docs/specs/02-memory.md` §4, `docs/specs/19-hardware-isolation-layers.md`, `docs/security-model.md`.

## Implementation Steps

1. **SMAP inventory:** list every x86 kernel access to U=1 memory outside `user_copy`, for each profile. Log the list and the D-04b scope.
2. TSS layout plus per-CPU IST stacks with guards. Boot self-check marker: `X86-IST: cpu=N ist1=.. ist2=.. ist3=.. guards=ok`.
3. IST-aware gate constructor and vector assignment; IST-aware frame decoding.
4. NMI/#MC paranoid-GS entry; NMI log+IRET; #DF/#MC fatal dump.
5. AC clearing (FMASK + common IDT entry); SMEP/UMIP enablement.
6. Migrate the step-1 sites; then enable SMAP (CPUID-gated) with stac/clac in the guard.
7. Test hooks (`test-hooks` builds only):
   - (a) **Deterministic entry-window NMI:** a test-only stackless rendezvous immediately after `swapgs`, before the kernel RSP load. It writes a marker byte to a per-CPU slot and the serial port with no stack use, spins on a per-CPU release flag, the runner injects `nmi`, then releases. Expect `X86-NMI: ist=1 gs=kernel origin=syscall-window` and normal syscall completion.
   - (b) NMI during CPL3 and during CPL0 kernel work; GS identity asserted before and after.
   - (c) Kernel stack overflow → `X86-DF: ist=2 old_rsp=<guard addr>` → halt.
   - (d) `mce` → `X86-MC: ist=3 bank=1` → halt.
   - (e) A CPL3 task sets AC, then enters via `int 0x80`, a timer interrupt and a #PF. Each handler asserts live AC=0.
   - (f) Kernel read of a user page outside the guard → SMAP #PF; (g) kernel jump to a user page → SMEP #PF; (h) CPL3 `sgdt` → #GP (UMIP).
   - (i) Tier-1 device Cell and hypervisor `vcpu_regs` paths pass on the SMAP image.
8. Re-run the existing IDT and domain gates; run the Phase 03 bench and log the overhead.

## Success Criteria

- [ ] `bash scripts/build-x86_64-idt-test-ci.sh && bash scripts/qemu-x86_64-idt-test.sh` passes cases 7a–7e on TCG, and on KVM where available.
- [ ] `bash scripts/build-x86_64-domain-test-ci.sh && bash scripts/x86/qemu-domain-test.sh` passes 7f–7i and the existing copy/fault cases.
- [ ] Negative controls: the 7a rendezvous on the pre-change image faults at the window (no IST) with a recorded marker; 7e on an image without the IDT AC-clear shows AC=1. The logs are kept.
- [ ] The Phase 03 KVM budget still passes. Overhead is recorded.
- [ ] `mutable-globals.py --check` passes with the IST and rendezvous state classified.
- [ ] If SMAP is deferred by step 1, the deviation entry names the remaining sites and their owner.

## Security Considerations

This closes the CPL0-on-user-stack window and the AC-set entry bypass, and adds kernel-execute and kernel-access barriers against user pages. The rendezvous hook exists only in `test-hooks` builds; the success criteria include a check that production images have no rendezvous symbol (`nm` on the kernel ELF).

## Risk Notes

A paranoid-GS bug corrupts per-CPU state silently; 7a/7b assert GS identity. D-04a (NMI non-fatal) matches the Linux default. Undo: revert and cold boot; no persistent state.

## Deviation Log

None.
