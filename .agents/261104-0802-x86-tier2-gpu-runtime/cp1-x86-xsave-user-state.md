# CP1 — x86-64 XSAVE user state (x87 | SSE | AVX) with FXSAVE fallback

Status: **design check APPROVED by the owner on 2026-11-04 ("duyệt"), X-1…X-7 as §9 recommends** ([record](handoff-and-xsave-approval.md)). The R5 handoff is approved but **not yet effective**: it needs the Intel WIP lane and S1's release of `hal/arch/x86/**`. CP2 is required after implementation (X-7).
- Written 2026-11-04 as phase R0's XSAVE package for [phase R5](phase-05-xsave-avx-user-state.md).
- The owner already approved the scope (D-R5: XCR0 = x87 | SSE | AVX; AVX-512, AMX and PKRU outside the mask) and the governance route (D-R6: a CP1/CP2-style check although the change lies outside Law 1's `libs/api`/`libs/types` scope, `docs/code-standards.md:42`). See the [approval record](owner-decisions-approval.md).
- This package asks for the design check (§9) and the lock handoff (§8).
- Every `file:line` was read in this session. `[INFERENCE]` marks claims that were not read in repo code. SDM references are to the Intel SDM (external specification).

## 1. Scope

**In:**
- enabling CR4.OSXSAVE and XCR0 on every CPU;
- widening the per-entry user FP image from FXSAVE64 (528 B) to an XSAVE image;
- a kernel-built init image for fresh user threads;
- isolating the host XCR0 from guests;
- keeping the FXSAVE path byte-for-byte on CPUs without XSAVE;
- the user-visible contract below.

**User-visible contract (new):**
- On a CPU with XSAVE and AVX: CPUID.1:ECX.OSXSAVE = 1 and XGETBV(0) = `0x7`. YMM0-15 survive syscalls, interrupts, exceptions and preemption.
- Without AVX: XCR0 = `0x3`.
- Without XSAVE: exactly today's behaviour (OSXSAVE = 0; XMM state preserved by FXSAVE).

**Out:**
- AVX-512 (opmask/ZMM components 5-7), AMX (17-18, needs XFD), PKRU (component 9), CET user state (11-12, supervisor);
- XSAVEC/XSAVES (compacted format);
- lazy FPU switching;
- any `libs/api`/`libs/types` change, syscall, record or allowlist bit;
- VMX guest execution (Intel VMX P09, still pending).

## 2. Evidence

| # | Fact | Evidence |
|---|---|---|
| E1 | FPU init sets CR0 EM = 0, TS = 0, MP, CR4 OSFXSR (bit 9) and OSXMMEXCPT (bit 10), `fninit`, MXCSR `0x1F80`. No OSXSAVE and no XSETBV | `hal/arch/x86/src/x86_64.rs:554-571` |
| E2 | `init_sse` runs on the BSP before the GDT/IDT and on every AP | `kernel/src/main.rs:233-236`; `hal/arch/x86/src/x86_64.rs:281`, `:485` |
| E3 | The boot policy demands FXSAVE, SSE and SSE2 | `hal/arch/x86/src/x86_64.rs:250-251` |
| E4 | Syscall path: `sub $528`, `fxsave64`, `fninit`, kernel MXCSR, dispatch, `cli`, `fxrstor64` | `hal/arch/x86/src/x86_64/syscall.rs:340-353` |
| E5 | IDT path: 16-byte align, `sub $528`, `fxsave64`, …, `fxrstor64` | `hal/arch/x86/src/x86_64/idt/entry.rs:144-156` |
| E6 | The context switch keeps only MXCSR and x87 control; user FP state lives in the entry image on the task's kernel stack | `hal/arch/x86/src/x86_64/context.rs:4-8`, `:50`, `:60-63` |
| E7 | A fresh user thread gets zeroed x87 and MXCSR `0x1F80` | `hal/arch/x86/src/x86_64/boot.rs:92-106` |
| E8 | IST stacks: 8 KiB; deepest path ≈ 1 KiB including the 528-byte area; `ist-hw=` is measured | `hal/arch/x86/src/x86_64/ist.rs:14-17` |
| E9 | Entries run `WRPKRU` (kernel PKRU = 0) before the FP save | `hal/arch/x86/src/x86_64/syscall.rs:322-325`; `hal/arch/x86/src/x86_64/idt/entry.rs:129-134` |
| E10 | SVM guests: intercepts are INTR, CPUID, PAUSE, HLT, IOIO, MSR and VMRUN. **XSETBV is not intercepted** | `hal/arch/x86/src/x86_64/vmcb.rs:53-59`, `:109-114` |
| E11 | VMX exit reasons for XSETBV (55) and XSAVES/XRSTORS (63/64) are defined only; VMX is root-operation only | `hal/arch/x86/src/x86_64/vmcs.rs:439`, `:447-448`; `docs/specs/05-application.md:430` |
| E12 | No XCR0 handling exists anywhere in `kernel/src` or `hal/arch/x86` | grep `xsetbv|xcr0` this session |
| E13 | No ABI record or `libs/api` item describes x86 FP state | grep of `libs/api`, `libs/types`, `docs/abi-boundary-records.md` this session; only AArch64 setjmp notes (`libs/api/src/services/posix/setjmp.rs:109-116`) |
| E14 | Local evidence hosts: QEMU 8.2.2 models list `xsave`, `xsaveopt`, `xsavec`, `avx2`, `avx512f`; `/dev/kvm` exists; the host CPU has `avx2` and `xsave` | `qemu-system-x86_64 -cpu help`, `/proc/cpuinfo` (this session) |

## 3. Design

**3.1 Detection and policy (BSP, then every AP)**
- If CPUID.1:ECX.XSAVE = 0, keep the FXSAVE path (§3.5) and log `fpu: fxsave`.
- Otherwise:
  - set CR4.OSXSAVE (bit 18);
  - `XCR0 = 0x7` if CPUID.1:ECX.AVX = 1, else `0x3`, written with XSETBV;
  - read the image size `S` from CPUID.(EAX = 0Dh, ECX = 0).EBX **after** XSETBV (SDM: size for the enabled XCR0), and round `S` up to 64. That is 832 B for x87 + SSE + AVX `[INFERENCE: SDM standard layout; never hard-coded]`;
  - log `fpu: xsave xcr0=0x7 size=S`.
- The policy (mode, XCR0, `S`) is published once by the BSP, next to `publish_boot_cpu_policy` (E3). An AP whose CPUID differs refuses to come online with `fpu: ap mismatch`. One XCR0 for the whole machine keeps every image interchangeable.
- **Instruction choice: plain `XSAVE64` / `XRSTOR64` in v1.**
  - This supersedes R5's "XSAVEOPT if present".
  - XSAVEOPT's modified/init optimisation depends on the linear address and the mode of the last XRSTOR. With per-task images on different kernel stacks and VMRUN in between, that is easy to get wrong `[INFERENCE: SDM §13.6]`.
  - XSAVEOPT can follow as a measured, kernel-internal optimisation behind its own CP2 note.

**3.2 Image and mask**
- The image is on the current kernel stack, as today. RSP is aligned down to 64 bytes, then `S` is subtracted.
- The requested-feature bitmap is EDX:EAX = XCR0, so PKRU (bit 9) is never saved or restored by XRSTOR. PKRU stays under the existing WRPKRU sequence (E9). RDPKRU/WRPKRU depend on CR4.PKE, not on XCR0.
- Kernel FP hygiene after save is unchanged: `fninit` + MXCSR `0x1F80`. The kernel is soft-float. The sanitising protects any FP use in kernel helpers.
- The exit path is `cli` then `xrstor64 (%r13)` with the same mask, replacing E4/E5's `fxrstor64`.

**3.3 Fresh threads.** A kernel-built init image replaces E7's `fninit`/`fldz` sequence:
- legacy area zero except FCW `0x037F` and MXCSR `0x1F80`;
- XSAVE header XSTATE_BV = 0, so XRSTOR puts every component in its init state;
- XCOMP_BV = 0.
A fresh thread therefore reads zero YMM. This is the leak test in §6.

**3.4 XRSTOR fault handling**
- XRSTOR #GP sources (SDM): a misaligned image, reserved bits in XSTATE_BV or XCOMP_BV, XSTATE_BV bits outside XCR0, or reserved MXCSR bits.
- **By construction none is reachable from user state:**
  - the image lives on the kernel stack, which is not user-mapped;
  - it is written only by the kernel's own XSAVE, or it is the kernel's init image;
  - MXCSR in the image comes from hardware, and LDMXCSR with reserved bits already faults in user mode.
- **Policy:** a #GP or #AC at the XRSTOR site is a kernel invariant violation and is handled exactly like today's FXRSTOR fault: a fatal kernel fault with the RIP named.
  - The fatal message adds `fpu: xrstor fault img=<addr> xstate_bv=<v>` so a corrupted image is diagnosable.
  - `[INFERENCE: kernel-mode #GP on x86 is fatal today; step 1 confirms the IDT dispatch policy before relying on it.]`
- No exception-table fixup is introduced. That would be a new mechanism for an unreachable case.
- A `test-hooks` selftest corrupts XSTATE_BV in a scratch image, calls XRSTOR inside a fault-expecting probe, and asserts the fatal path's message. It is a test hook only, never in shipping images.
- **Guest-induced XCR0 change** is the one reachable route to a mask mismatch. If a guest lowered XCR0, host XRSTOR would silently skip components; if it raised it, the host would save extra state. §3.6 closes it.

**3.5 Fallback.** With no XSAVE, the existing `fxsave64`/`fxrstor64` sequences (E4, E5) and the E7 thread init stay byte-identical, chosen at boot.
- The selection is one patched branch or two entry stubs, chosen by the boot policy `[INFERENCE: pick the lower-risk form in step 2]`.
- Rollback is "force the fallback" (a boot flag `fpu=fxsave`, kept permanently as a diagnostic).

**3.6 Hypervisor (SVM MVP)**
- Intercept XSETBV (VMCB intercept word 2, XSETBV bit `[INFERENCE: AMD APM vol. 2 VMCB layout]`).
- Emulate it against a per-vCPU guest XCR0, and load that value only for the VMRUN window.
- Restore the host XCR0 immediately after #VMEXIT, before any host XSAVE/XRSTOR.
- Without the intercept today (E10), a guest's XSETBV writes the physical XCR0. That is harmless only while the host does not use XSAVE.
- VMX (E11) inherits the same rule when P09 enables guest execution.

**3.7 Size and stack budget**
- Each nested entry grows by `S - 528` (≈ 304 B at `S` = 832).
- Kernel task stacks and the 8 KiB IST stacks (E8) are re-measured: `ist-hw` must stay < 4 KiB.
- Deep nested paths are covered by the existing IDT fixture.

## 4. Wire and ABI

No syscall, record, flag or allowlist change. Highest syscall number in the tree: 435 (`libs/api/src/abi/syscall.rs:698`).

The new user-visible contract (§1) gets one row in `docs/abi-boundary-records.md` at CP2: class K, "x86 user CPU state", added by the integrating session.

## 5. Failure semantics

| Event | Result |
|---|---|
| CPU without XSAVE | FXSAVE path, `fpu: fxsave`, OSXSAVE = 0 (as today) |
| AP CPUID differs from the BSP policy | AP stays offline with `fpu: ap mismatch`; the BSP continues |
| User executes AVX with XCR0 = 0x3 | #UD in the Cell, contained as any user fault |
| User executes AVX-512/AMX | #UD (components not enabled) |
| XRSTOR #GP or #AC | fatal kernel fault with the image diagnostics (§3.4) |
| Guest XSETBV | intercepted and emulated; host XCR0 restored on every #VMEXIT |
| Stack budget exceeded | caught by `ist-hw` and the existing stack guard pages (`ist.rs:9-12`) |

## 6. Test plan

**Host:** unit tests for the policy computation (CPUID inputs → mode, XCR0, `S`), the init-image bytes, and mask construction (never bit 9).

**QEMU TCG** (`-cpu max`) and **QEMU/KVM** (`-cpu host`):
- **Preserve:** witness Cell `x86-avx-preserve` (Tier-2, test-hooks domain lane). Two threads load distinct YMM0-15 patterns and MXCSR, then spin across 10 000 preemptions, `Yield`, a blocking syscall and device interrupts. They verify each round and report XGETBV(0) = `0x7`.
- **Init/leak:** a fresh thread reads all-zero YMM.
- **Fallback:** `-cpu qemu64,-xsave`: the boot log shows `fpu: fxsave`, and the R1 `x86-fpu-preserve` witness passes.
- **No AVX:** `-cpu qemu64,+xsave,-avx`: XCR0 = `0x3`.
- **SMP:** the 4-CPU domain lane runs the witness with threads migrating.
- **Guest isolation:** the SVM smoke guest under TCG executes XSETBV(0, `0x3`), and the host witness still passes afterwards.
- **Fatal path:** the `test-hooks` XRSTOR selftest (§3.4).
- **Regression:** every existing x86 QEMU suite, plus the IDT fixture with `ist-hw` < 4 KiB.

The evidence ceiling is QEMU TCG + QEMU/KVM (local). The physical machine is not required.

## 7. Files a later implementation would touch

- **HAL:**
  - `hal/arch/x86/src/x86_64.rs` (`init_sse` → FPU policy, AP check);
  - `hal/arch/x86/src/x86_64/syscall.rs` (E4);
  - `hal/arch/x86/src/x86_64/idt/entry.rs` (E5);
  - `hal/arch/x86/src/x86_64/boot.rs` (E7);
  - `hal/arch/x86/src/x86_64/context.rs` (comment);
  - `hal/arch/x86/src/x86_64/ist.rs` (budget comment);
  - `hal/arch/x86/src/x86_64/vmcb.rs` and `svm_vcpu.rs` (XSETBV intercept and emulation; XCR0 swap around `svm_vmrun`).
- **Kernel:** `kernel/src/main.rs` (x86 early-init log line only, if the policy log is not emitted from the HAL).
- **Tests and scripts:** new `cells/tests/x86-avx-preserve/**`; a new runner script that reuses `scripts/build-x86_64-domain-test-ci.sh` and boots through `scripts/x86/qemu-domain-test.sh` (read-only use); the IDT fixture's `ist-hw` bound.
- **Launch edge:** init launches the Tier-2 witness through the reviewed rows in `kernel/src/loader/launch_profile/profiles.rs:39-54` (Tier-2 fixtures `/bin/tier2-smoke`, `/bin/cpp-smoke`), `targets.rs:29-75` and the mirrored lists in `tests.rs:986-988`. `profiles.rs` and `tests.rs` sit in S1's extended exception (`cell-contract-0024-locks.md:159`). See [handoff-requests.md](handoff-requests.md) §2 (decision H-1).
- **Docs:** `docs/abi-boundary-records.md` (row, by the integrating session), `CHANGELOG.md`, the x86 note in `docs/guides/tier1b-c-zig.md`.

## 8. Lock relations and scheduling

- **Lane.**
  - S1 holds the Intel WIP lane, `hal/arch/x86/**` and the x86 init in `kernel/src/main.rs` (`.agents/261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md:122-130`; `.agents/cell-contract-0024-locks.md:154-161`).
  - This package takes effect only after the owner gives R5 the lane and S1 releases `hal/arch/x86/**`.
  - It queues against the GPU DMA-export slice and the large-memory slice, which also wait for the lane (`cell-contract-0024-locks.md:163-171`, line numbers as of 2026-11-04 after Main's edits). The owner orders them. R5 is not on the G3 path (D-R7: before G4).
- **CELL-CONTRACT-0024 controlling area.** P03 (task/thread identity, SMP) for per-task state, and P09 for the guest XCR0 rule (`cell-contract-0024-locks.md:98`, `:104`).
- **Rows to rule compatible at handoff:**
  - `260711-1917-tier3b-x86-vtx` (Intel VMX/EPT P09, `:186`): VMX inherits §3.6 and is not executed here.
  - `260927-0739-kernel-architecture-repair` (`:210`): no grant/domain change.
  - `261004-1957-x86-pc-lane` (`:212`): no device authority.
- **Precedent.** The Ginux Phase 04 slice edited the same HAL entry/IDT/IST files without a Law-1 delta (`cell-contract-0024-locks.md:129`).
- **Not authorised by this draft:** any source edit before the lane and the handoff; AVX-512, AMX, XSAVEOPT, XSAVES.

## 9. Decisions for the owner

| ID | Question | Recommendation |
|---|---|---|
| X-1 | Plain XSAVE/XRSTOR in v1; XSAVEOPT later as a measured kernel-internal change (supersedes R5's "XSAVEOPT if present") | Yes |
| X-2 | One machine-wide XCR0; an AP with a different CPUID stays offline | Yes |
| X-3 | XRSTOR fault = fatal kernel fault with diagnostics; no fixup mechanism (§3.4) | Yes |
| X-4 | Intercept and emulate guest XSETBV on SVM; swap XCR0 around VMRUN; VMX inherits the rule at P09 | Yes |
| X-5 | Keep the FXSAVE path selectable by the permanent boot flag `fpu=fxsave` | Yes |
| X-6 | File set §7 as the R5 handoff, effective with the lane and S1's release of `hal/arch/x86/**` | Yes |
| X-7 | CP2 after implementation with the exact delta, evidence, and the `abi-boundary-records` row | Yes |

## 10. Not checked

- The kernel-mode #GP dispatch policy on x86, and whether an exception fixup facility exists. Assumed fatal; step 1 confirms (§3.4).
- AMD APM VMCB intercept-bit position for XSETBV, and SDM image sizes and offsets. These are external specifications that were not read in this session.
- Kernel task-stack size on x86. It is re-measured, not assumed.
