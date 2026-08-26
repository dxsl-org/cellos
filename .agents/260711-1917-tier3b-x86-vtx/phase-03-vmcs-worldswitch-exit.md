# Phase 03 — VMCS/VMCB + vCPU world-switch + exit decode + bare-metal smoke (M1)

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-02](phase-02-ept-npt-guest-ram.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-03-vcpu-worldswitch-trap.md`
- Verified: `kernel/src/hypervisor/registry.rs:182` (`run_vcpu` world-switch shell, aarch64 branch to mirror),
  `:119` (create_vcpu + test-hooks smoke blob), `kernel/src/hypervisor/smoke_guest.rs`;
  `hal/traits/hypervisor/src/lib.rs:10` (HAL `ViVmExit`), `hal/arch/x86/src/x86_64/idt.rs`,
  `hal/arch/x86/src/x86_64/context.rs`.
- **This is the riskiest phase of the whole plan — the make-or-break world switch.**

## Overview
- **Priority:** P1 · **Status:** **SVM path DONE — M1 PASS 2026-07-23** (QEMU `-cpu qemu64,+svm
  -accel tcg`, serial `X86-VMM-SMOKE: PASS`). VMX path (`vmx.rs`) compiles + returns NotSupported;
  real bring-up deferred to P09. · **Depends on:** 01,02
- **Implemented:** `hal/arch/x86/src/x86_64/{vmcb.rs,world_switch.rs,vmexit_decode.rs,svm_vcpu.rs}`
  + `kernel/src/hypervisor/svm_registry.rs` + x86 branches in `registry.rs`. HAL `ViVmExit` gained
  `PortIn/PortOut/Hlt/Msr` (P04 freezes the `libs/api` mirror). Guest-RAM host access goes through
  `phys_to_virt` (x86 HHDM; the ARM identity-map assumption does NOT hold — that was the first M1 bug).
- **Two bring-up bugs found + fixed during M1:** (1) guest_pa (physical) used directly as a pointer
  for the smoke-blob write → #PF at ~18 MiB (x86 phys≠virt) → wrap in `phys_to_virt`. (2) RIP did not
  advance past the trapped `OUT` (both exits were PortOut) because **QEMU TCG `+svm` does not advertise
  NRIPS → VMCB nRIP reads 0**; IOIO must advance via **EXITINFO2** (next-instr RIP), which is valid
  without NRIPS. Other instruction intercepts (HLT/MSR/CPUID) still rely on nRIP → P05 needs an
  instruction-length fallback for no-NRIPS hosts (not hit by the M1 blob).
- Allocate and populate a real **VMCS** (Intel) / **VMCB** (AMD), world-switch a vCPU into a tiny
  bare-metal guest, catch the first VM-exit, and decode it into a HAL `ViVmExit`. Success = **M1**: a
  ~16-byte 32-bit-protected-mode guest blob writes a byte to port 0x3F8 (or memory) and `HLT`s; the
  kernel decodes the resulting I/O / HLT exit. Vendor-dispatched: **SVM path is the CI target** (TCG),
  VMX path is stubbed here and brought up on hardware in P09.

## Key Insights
- **Minimal control-bit config for PVH 32-bit-protected entry (research #3):**
  - **Unrestricted-guest NOT needed** — PVH enters in 32-bit protected mode with paging off, which is a
    legal VMX guest state without unrestricted-guest. (Real-mode/big-real-mode would need it; PVH avoids
    it entirely — a key simplification.)
  - Enable EPT (secondary exec ctrl bit1) + EPTP from P02. Pin: external-interrupt exiting (bit0) +
    VMX-preemption timer (bit6). Primary: HLT exiting (bit7), use-I/O-bitmaps (bit25) OR
    unconditional-I/O exiting (bit24), activate-secondary (bit31). Entry: "IA-32e mode guest"=**0**
    initially. Exit: "host address-space size"=1, "acknowledge interrupt on exit"=1.
  - Compute every control field by reading the **"true" capability MSRs** (`IA32_VMX_TRUE_PINBASED_CTLS`
    etc.) and applying `allowed-0`/`allowed-1` masks — never hard-code, or VM-entry fails consistency
    checks with a cryptic error number.
- **SVM VMCB intercepts (research #1/#3):** intercept `IOIO` (I/O), `HLT`, `CPUID`, `MSR`, `NPF`
  (nested-page-fault = MMIO), `INTR` (physical interrupt, for the budget timer). NPT via nCR3 (P02).
  `TLB_CONTROL`, ASID ≥ 1. Guest state (CR0=0x11, EFLAGS=0x2, CS/DS flat) written directly into VMCB
  save area — no capability-MSR dance (simpler → why SVM is first).
- **IA-32e transition (cross-cutting invariant):** guest starts non-64-bit. Set CR0 guest/host mask so
  the guest's `CR0.PG` 0→1 write **exits** (VMX: CR-access exit reason 28; SVM: CR0 write intercept).
  On that exit, set entry-control "IA-32e mode guest"=1 (VMX) / adjust EFER.LMA (SVM) before resuming.
  Do NOT let long-mode enable free-run into an entry consistency failure.
- **Exit decode → HAL `ViVmExit`:** EPT-violation(48)/NPF → `MmioRead`/`MmioWrite` (GPA from
  GUEST_PHYSICAL_ADDRESS field + instruction decode for size/reg/direction); I/O exit(30)/IOIO →
  `PortIn`/`PortOut` (NEW, P04) from exit qualification (port[31:16], size[2:0], dir[3]); HLT(12) →
  `Hlt` (NEW, P04); MSR read/write(31/32) → `Msr` (NEW, P04); preemption-timer(52)/budget → `Preempted`;
  external-interrupt(1) → handle host IRQ, re-enter (not surfaced to cell); triple-fault/shutdown →
  `Shutdown`; anything else → `Unknown{reason, qual}`. **P03 emits the HAL enum; the `#[repr(C)]` ABI
  freeze + new variants are P04** (same staging as ARM: HAL enum in P03, ABI in P04).
- **World-switch save/restore (Law 4):** on VMLAUNCH/VMRESUME (VMX) or VMRUN (SVM), host GP + segment +
  MSR state must be saved/restored around the guest run. Host-state VMCS area handles CS/SS/RIP/RSP on
  exit; GP regs saved manually (mirror the ARM `run_vcpu_impl` register-swap discipline). Reuse the
  existing x86 trap/context scaffolding (`context.rs`, `idt.rs`) for host IDT composition.
- **SVM budget vendor difference (open question):** SVM has **no** preemption timer. Implement
  `budget_ns` by arming a host one-shot timer (LAPIC/HPET) before VMRUN + INTR intercept; the resulting
  external-interrupt exit is converted to `Preempted` when the budget deadline is hit. Spike this under
  TCG before building the run loop on it.
- **test-hooks smoke blob (mirror `registry.rs:119`):** write a tiny 32-bit blob into guest RAM at the
  entry GPA so the M1 test needs no userspace guest memory. Blob: `mov dx,0x3f8; mov al,'K'; out dx,al;
  hlt`.

## Requirements
**Functional**
- `X86Svm`/`X86Vmx` structs impl the internal vCPU contract: `new(entry, cr3, ...)`, `run() -> HalVmExit`.
- VMCS/VMCB alloc (revision-ID'd for VMCS), guest-state init for PVH entry, control fields from true-MSRs.
- World-switch asm (VMLAUNCH/VMRESUME + VMREAD/VMWRITE; or VMRUN) with host GP save/restore.
- Exit-reason → HAL `ViVmExit` decoder (incl. instruction-length advance data for P05).
- CR0.PG-write trap → IA-32e entry-control flip.
- `budget_ns` → `Preempted` (VMX preemption timer; SVM host-timer+INTR).

**Non-functional**
- Law 4: all VMX/SVM instructions + VMREAD/VMWRITE + asm in HAL `unsafe` with `// SAFETY:`.
- SVM path builds+runs under TCG; VMX path compiles + is exercised only on KVM/HW (P09).

## Architecture
```
registry(x86).run_vcpu(vcpu, budget_ns):
  vendor match:
    Svm: write budget host-timer; VMRUN(vmcb); on #VMEXIT read exitcode/exitinfo → decode
    Vmx: VMWRITE preempt-timer; VMLAUNCH/VMRESUME; on exit VMREAD reason/qual → decode
  decode → HalVmExit → (P04) ApiVmExit → *exit_out
M1 smoke: create_vcpu(entry) [+ test-hook blob] → run_vcpu → assert PortOut{port=0x3f8,val='K'} or Hlt
```
Guest entry state (PVH, research #4): CR0=0x11(PE|ET), CR4=0, EFER=0, EFLAGS=0x2, flat GDT
{null, code 0xc09b, data 0xc093}, CS=0x08/DS=SS=0x10 (base0/limit4G), RIP=entry, RBX=start_info (P05).

## Related Code Files
**Create**
- `hal/arch/x86/src/x86_64/vmcs.rs` — VMCS alloc/format, field enums, VMREAD/VMWRITE wrappers, guest/host-state setup, control-field computation from true-MSRs
- `hal/arch/x86/src/x86_64/vmcb.rs` — VMCB layout, intercept setup, guest-state setup, VMRUN wrapper
- `hal/arch/x86/src/x86_64/world_switch.rs` — asm entry (VMLAUNCH/VMRESUME/VMRUN) + host GP save/restore
- `hal/arch/x86/src/x86_64/vmexit_decode.rs` — exit-reason → HAL `ViVmExit`
**Modify**
- `hal/arch/x86/src/hypervisor.rs` — replace NotSupported stubs (`:16-20`) with SVM-backed `X86_64Hypervisor` (VMX arm returns NotSupported until P09); wire `Vm`/`Vcpu`/`Stage2Table` assoc types to P02/P03 types
- `kernel/src/hypervisor/registry.rs` — x86 branch of `create_vcpu`(`:119`)/`run_vcpu`(`:182`) driving the HAL vCPU; x86 test-hook smoke blob
- `hal/arch/x86/src/x86_64.rs` — module wiring

## Implementation Steps
1. `vmcb.rs` (SVM first): VMCB layout, set intercepts (IOIO/HLT/CPUID/MSR/NPF/INTR), nCR3=P02, ASID=1,
   guest save area = PVH entry state.
2. `world_switch.rs`: VMRUN loop with host GP save/restore; `#[naked]`/global_asm; CET-IBT landing pad
   (mirror `boot.rs:80` ENDBR64 convention).
3. `vmexit_decode.rs`: SVM exitcode → HalVmExit (IOIO→PortIn/Out, HLT→Hlt, NPF→Mmio, etc.); carry
   instruction length for PC-advance.
4. CR0-write intercept → EFER.LMA / IA-32e handling on long-mode enable.
5. `budget_ns`: SVM host one-shot timer + INTR intercept → `Preempted`. **Spike under TCG first.**
6. `vmcs.rs` + VMX world-switch: build to the same contract but gate real bring-up to P09 (compiles,
   returns NotSupported at runtime on non-HW).
7. Wire registry x86 `create_vcpu`/`run_vcpu`; add x86 test-hook smoke blob (port-out 'K' + HLT).
8. **M1:** run the smoke guest via the kernel test-hook; assert the decoded exit; loop 1000× and assert
   host register snapshot equality after each run (mirror ARM m5 — proves register isolation, not just
   liveness).

## Todo List — SVM path complete 2026-07-23 (M1 PASS)
- [x] VMCB (SVM) layout + intercepts + PVH guest-state init — `vmcb.rs` `VmcbView` (view over a
      kernel-owned frame; no alloc/Drop — kernel owns frames). ASID=1, NP_ENABLE, N_CR3, VMRUN
      intercept, IOIO/HLT/MSR/CPUID/INTR/CR0-write intercepts, EFER.SVME, G_PAT default.
- [x] World-switch asm (VMRUN) + host GP save/restore + ENDBR64 — `world_switch.rs` naked
      `svm_vmrun` with the **VMSAVE/VMLOAD host pair** (GS.base fence) + guest GPR load/save; no `gs:`
      in the VMRUN→VMLOAD window; caller runs IF=0 (`svm_registry::run_vcpu_hal` cli/sti).
- [x] Exit decoder SVM exitcode → HAL ViVmExit — `vmexit_decode.rs` (IOIO→PortIn/Out, MSR→Msr,
      HLT→Hlt, NPF→Mmio w/ PT-walk guard, else Unknown).
- [x] CR0.PG-write trap handling — CR0-write intercept applies the decode-assist GPR to VMCB CR0;
      EFER-WRMSR handler forces SVME back in (else next VMRUN → INVALID). (SVM auto-derives LMA — the
      explicit "IA-32e mode guest" flip is VT-x only, P09.)
- [~] `budget_ns` → Preempted — INTR intercept + V_INTR_MASKING set; MVP surfaces INTR as Preempted.
      Host one-shot-timer arming (dossier §6 spike) **deferred** — M1 uses HLT-yield (no timer armed).
- [x] VMCS (VMX) path compiles (`vmx.rs` enter_root); guest bring-up deferred to P09.
- [x] registry x86 create_vm/create_vcpu/map_guest/run_vcpu + test-hook smoke blob (`svm_registry.rs`).
- [x] **M1** — guest `out 0x3f8,'K'` decoded as `PortOut{0x3F8,1,0x4B}` then `Hlt`; `X86-VMM-SMOKE:
      PASS` on QEMU +svm TCG. **1000× register-isolation snapshot test still TODO** (M1 liveness proven;
      the host-GS/LSTAR equality assert from dossier §7 is the next hardening step).

## Success Criteria
- On `-cpu qemu64,+svm -accel tcg`, the M1 test-hook runs the smoke blob and the kernel decodes
  `PortOut{port=0x3f8, size=1, val='K'}` followed by `Hlt`. Logged + asserted.
- 1000× run loop: host GP-register snapshot is identical before/after every VMRUN (no guest→host leak).
- `budget_ns` expiry yields a clean `Preempted` exit under TCG (SVM host-timer path validated).
- VMX code compiles for x86_64; runtime on non-HW returns NotSupported without faulting.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| **VM-entry consistency-check failure** (cryptic error, silent) | High×High | Compute ALL controls from true-MSRs; log VM-instruction-error / SVM exitcode; bisect guest-state fields; start from a known-good minimal VMCS/VMCB |
| World-switch corrupts host state → kernel crash | Med×Crit | Register snapshot assert (m5); save/restore discipline; run on scratch stack |
| SVM budget-timer path unreliable under TCG | Med×High | Spike step 5 before run loop depends on it; fallback = HLT-only yield (guest idles → natural exit) |
| TCG SVM emulation-fidelity gaps (rare exitcodes) | Med×Med | Unknown{} catch-all logs exitcode; expand decoder as gaps surface |
| CR0.PG transition mishandled → guest #GP on long-mode enable | Med×High | Explicit CR0-write intercept + EFER.LMA flip; test with a 32→64 transition blob |
| VMX untestable in CI → drift from SVM | Med×Med | Single decoder + control-computation shared; VMX bring-up (P09) on KVM/HW lane |

## Security Considerations
- World switch is the trust boundary: host state fully restored on every exit; guest cannot influence
  host RIP/RSP/CR3 (host-state area is kernel-owned, written once).
- EPT/NPT (P02) confines guest physical accesses; NPF/EPT-violation on any out-of-carve GPA.
- `ViVmExit` (P04) carries only guest GPA/regs, never host addresses — a cell learns no host layout.

## Next Steps
- P04 freezes the new `ViVmExit` variants (PortIn/PortOut/Hlt/Msr) as `#[repr(C)]` ABI and wires the
  x86 registry to surface them to userspace (Law 1).
