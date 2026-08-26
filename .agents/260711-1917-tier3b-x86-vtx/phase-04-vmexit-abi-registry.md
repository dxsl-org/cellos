# Phase 04 — `ViVmExit` ABI extension (PortIn/PortOut/Hlt/Msr) + x86 registry wiring + smoke cell

> ⚠️ **Law 1: needs 2× user confirmation.** This phase adds `#[repr(C,u8)]` variants to the **frozen**
> `ViVmExit` (`libs/api/src/abi/hypervisor.rs:17`, VERSION=1) and bumps `VERSION` 1→2. New discriminants
> only (8+); NEVER change existing discriminants 0-7 or their fields. Confirm twice before implementing.

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-03](phase-03-vmcs-worldswitch-exit.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-04-syscalls-vmexit-abi.md`
- Verified: `libs/api/src/abi/hypervisor.rs:17-42` (frozen ViVmExit, variants 0-7, `VERSION=1` at :41);
  `hal/traits/hypervisor/src/lib.rs:10-20` (HAL ViVmExit); `libs/api/src/abi/syscall.rs:273-298`
  (syscalls 220-227 exist), `:545-547` (allowlist bit 44), `:637-643` (from_u64);
  `kernel/src/task/syscall.rs:3067-3149` (arch-generic dispatch), `:322` (caller_has_hypervisor);
  `kernel/src/hypervisor/registry.rs:242-252` (HAL→API exit conversion, aarch64).

## Overview
- **Priority:** P1 · **Status:** **ABI + registry DONE 2026-07-23** (Law-1 2× confirmed: user chose
  P04 + approved the exact delta via AskUserQuestion). `libs/api` ViVmExit VERSION 1→2, append-only
  discriminants 8-11 (PortIn/PortOut/Hlt/Msr); `const _: () = assert!(size_of::<ViVmExit>() == 80)`
  pins the CC envelope (I1); 3 CC-neutrality invariants recorded in the ABI doc-comment. registry x86
  `run_vcpu` now does the real HAL→API conversion (placeholder Unknown removed). **Runtime-verified:**
  the kernel M1 smoke was switched to drive the **public syscall-level `run_vcpu`** (exercises the
  HAL→API conversion) → `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)` on QEMU +svm TCG. Built green on
  x86_64 + aarch64 (incl. `service-hypervisor` cell match arms) + riscv64. · **Depends on:** 03
- **Userspace smoke CELL deferred to P05:** a `hypervisor=true` cell driving 220-227 from userspace
  cannot QEMU-run until the x86 hypervisor-cell spawn path exists (P05 builds the x86 personality).
  The kernel↔registry API path is already runtime-proven above; the cap gate (`caller_has_hypervisor`)
  is arch-generic and already tested on ARM. No half-wired cell left in the tree.
- Extend the frozen `ViVmExit` ABI with the x86-specific exit reasons the P03 decoder produces, wire the
  x86 branch of `registry::run_vcpu` to convert HAL→API and surface them, and ship a smoke cell that
  drives the M1 guest from userspace. **No new syscalls, no manifest-flag bump** — 220-227 +
  `MANIFEST_FLAG_HYPERVISOR` already exist and are arch-generic (verified). Only the ABI enum grows and
  the registry gains an x86 conversion arm.

## Key Insights
- **New variants (Law 1 — enumerate ALL now, freeze once):** append at discriminants 8-11 so existing
  0-7 (ARM) are untouched:
  - `PortIn  { port: u16, size: u8, reg: u8 }              = 8` — I/O read (16550 UART, PIC, PIT)
  - `PortOut { port: u16, size: u8, val: u32 }             = 9` — I/O write
  - `Hlt                                                    = 10` — HLT (x86 idle; timer-inject point)
  - `Msr     { index: u32, is_write: bool, val: u64 }      = 11` — RDMSR/WRMSR (TSC, future x2APIC)
  Bump `ViVmExit::VERSION` 1→2 (`hypervisor.rs:41`). Reuse `MmioRead`/`MmioWrite`/`Preempted`/`Shutdown`/
  `Unknown` (0,1,5,6,7) as-is for EPT-violation/NPF, budget, triple-fault, and catch-all. `Hvc`/`Wfi`/
  `SysReg` (2,3,4) stay ARM-only, never emitted on x86 — harmless dead variants (the enum is a union).
  The existing `_`-less match in `run_loop.rs:46` must gain arms for the new variants (P05).
- **Padding budget:** the ARM enum's largest variant is `Hvc{imm,regs:[u64;8]}` ≈ 66 bytes → the new
  x86 variants (≤ 16 bytes) fit within the existing size envelope; adding them does **not** grow
  `size_of::<ViVmExit>()`, so `validate_user_buf(out_ptr, size_of::<ViVmExit>())` (`syscall.rs:3100`)
  stays correct. Confirm with a `const_assert` on size.
- **Syscalls are already arch-generic (verified):** `syscall.rs:3067-3149` dispatches 220-227 to
  `crate::hypervisor::registry::*` which cfg-branches internally — the x86 branches land in P02/P03/P04,
  **no syscall.rs change needed** beyond what already exists. `caller_has_hypervisor` (`:322`) is
  arch-neutral.
- **`InjectIrq` semantic overload (open question):** ABI param `intid` (`syscall.rs:288`, doc says
  "GICv2 0≤intid≤1019") is reinterpreted on x86 as an **8259 IRQ line / interrupt vector**. No ABI
  change; document the overload in the x86 registry branch + a doc-comment note. The `intid ≤ 1019`
  guard (`syscall.rs:3122`) is harmless for x86 vectors (0-255).
- **HAL→API conversion (mirror `registry.rs:242-252`):** the x86 `run_vcpu` branch converts
  `HalVmExit::{PortIn,PortOut,Hlt,Msr,MmioRead,MmioWrite,Preempted,Shutdown,Unknown}` → the API enum,
  same pattern as the aarch64 arm.
- **Cap gate already x86-aware after P01:** `cap.rs:164` OR includes `has_x86_virt()`. Deny path
  unchanged (audited PermissionDenied).

## Requirements
**Functional**
- Append `PortIn`/`PortOut`/`Hlt`/`Msr` to BOTH `libs/api/src/abi/hypervisor.rs` (`#[repr(C,u8)]`, disc
  8-11) and `hal/traits/hypervisor/src/lib.rs` (HAL enum). `VERSION=2`.
- `registry::run_vcpu` x86 branch: HAL→API conversion incl. the 4 new variants.
- `const_assert!(size_of::<ViVmExit>()` unchanged); `VERSION` bump documented.
- x86 smoke cell: manifest `hypervisor=true`, exercises 220-227 against the M1 guest, asserts `PortOut`.

**Non-functional**
- Law 1 (2× confirm). Law 4: cell `#![forbid(unsafe_code)]`. Law 8: VM registry Drop-on-death already
  wired (`registry.rs:415 reap_vms_for_task`) — x86 branch frees NestedPageTable via Drop.

## Architecture
```
x86 smoke cell (hypervisor=true)
  sys_create_vm(pages) → registry(x86): carve + NestedPageTable (P02)
  sys_map_guest_memory / sys_write_guest_memory → load M1 blob
  sys_create_vcpu(entry) → X86Svm vcpu (P03)
  loop sys_run_vcpu(&mut ViVmExit) → match { PortOut{0x3f8,..} → print; Hlt → done }
```

## Related Code Files
**Modify (⚠️ libs/api — Law 1)**
- `libs/api/src/abi/hypervisor.rs:17-42` — add variants 8-11; `VERSION=2`; `const_assert` size
- `hal/traits/hypervisor/src/lib.rs:10-20` — add matching HAL variants
**Modify (kernel)**
- `kernel/src/hypervisor/registry.rs` — x86 `run_vcpu` HAL→API conversion arm (mirror `:242-252`); doc InjectIrq overload
**Create**
- `cells/apps/hypervisor-x86-smoke/` — smoke cell (parallel to shipped `cells/apps/hypervisor-test/`)
- `Cargo.toml` — add smoke cell to workspace

## Implementation Steps
1. **(Law 1 confirm #1)** Enumerate + append variants 8-11 to `libs/api` ViVmExit; `VERSION=2`;
   `const_assert!(core::mem::size_of::<ViVmExit>() == <unchanged>)`.
2. **(Law 1 confirm #2)** Mirror the 4 variants in the HAL enum; ensure the P03 decoder emits them.
3. registry x86 `run_vcpu` conversion arm for all 9 x86-reachable variants; `write_guest_memory`/
   `read_guest_memory`/`vcpu_regs` x86 branches (mirror aarch64, GP-reg layout = x86 order).
4. Document `InjectIrq` intid→vector overload in the x86 registry branch.
5. Build the smoke cell (`#![forbid(unsafe_code)]`, manifest hypervisor=true + allowlist bit 44);
   load M1 blob via `sys_write_guest_memory`; assert `PortOut`.
6. Verify end-to-end from userspace on SVM/TCG; verify a non-hypervisor cell gets PermissionDenied.

## Todo List — ABI + wiring DONE 2026-07-23 (Law-1 2× confirmed)
- [x] ⚠️ Append PortIn/PortOut/Hlt/Msr (disc 8-11) to libs/api ViVmExit + VERSION=2
- [x] ⚠️ HAL ViVmExit already carries the 4 variants (P03); field-map (`value`→`val`, PortIn reg=0) in conversion
- [x] `const _: () = assert!(size_of::<ViVmExit>() == 80)` — envelope pinned (I1); validate_user_buf stays correct
- [x] registry x86 run_vcpu HAL→API conversion (real variants, placeholder removed) + write/read/regs x86 branches (svm_registry)
- [~] InjectIrq intid→x86-vector overload — no-op stub on x86 (EVENTINJ lands P05); overload documented at the registry inject_irq x86 arm
- [ ] **CC-neutrality review before freeze** (roadmap §"Confidential computing for Tier 3", G2/G3):
      the VERSION=2 ABI must not preclude TDX/SEV-SNP/ARM-CCA guests later — (a) exit variants carry
      port/MSR **values in the struct itself**, never host-dereferenceable guest-RAM pointers (CC guest
      memory is encrypted; the VMM cannot read arbitrary GPA); (b) discriminants stay append-only so an
      attested-launch exit class can be added without renumbering; (c) no field assumes the host can
      inspect guest register state beyond what the exit explicitly delivers. One paragraph in the Law-1
      confirm notes recording this check — prevents a protocol redesign when CC lands.
- [ ] **Record the 3 CC-neutrality invariants in the ABI doc-comment** (verdict GO-WITH-CHANGES,
      full analysis in `review-cc-neutral-abi-freeze.md`; all doc-only, zero code):
      **(I1, CRITICAL)** the size envelope — pinned by the `Hvc{regs:[u64;8]}` const_assert (~80B) — is
      the true freeze boundary; a future CC exit MUST carry a shared-region reference
      (`ghcb_gpa`+metadata), never an inline guest register file (TDX TDG.VP.VMCALL ~13 GPRs ~104B
      would overflow it → break every VERSION=2 cell's `validate_user_buf`).
      **(I2)** field-provenance: every field is a value the guest EXPLICITLY delivered
      (ISV=1 syndrome / GHCB / TDVMCALL); `Hvc.regs[]` = published hypercall args only; no variant ever
      carries guest RIP or raw instruction bytes.
      **(I3)** a CC sysreg/attested-launch path is always a NEW append-only variant (disc 12+), never a
      reshape of PortIn/PortOut/Msr/SysReg. By-value return + append-only discriminants confirmed safe;
      do NOT add a bounce-region field now (over-engineering).
- [x] **CC-neutrality invariants I1/I2/I3 recorded in the ABI doc-comment** (verdict GO-WITH-CHANGES; doc-only, zero code)
- [→] hypervisor-x86-smoke **userspace** cell — deferred to P05 (needs x86 hypervisor-cell spawn path; API path already runtime-verified kernel-side)

## Success Criteria
- The smoke cell (userspace, `#![forbid(unsafe_code)]`) runs the M1 guest via syscalls 220-227 and
  prints `vmexit=PortOut port=0x3f8 val=0x4b` ('K') — proving the full kernel↔cell x86 VMM ABI.
- A cell WITHOUT `hypervisor=true` calling `sys_create_vm` gets `PermissionDenied` (audited).
- Killing the smoke cell mid-run frees guest RAM (frame count → baseline; reuse P02 leak test via
  `reap_vms_for_task`).
- `cargo build` for aarch64 + riscv64 + x86_64 all succeed (new variants are additive; ARM match arms
  unaffected).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| ABI churn after freeze (Law 1) | Med×High | Enumerate ALL x86 variants now via P03 decoder; VERSION bump; append-only discriminants |
| `size_of::<ViVmExit>()` grows → breaks validate_user_buf | Low×High | const_assert on size; new variants ≤ existing Hvc envelope |
| ARM match exhaustiveness breaks (`run_loop.rs`) on new variants | Med×Med | ARM run_loop match gains no-op arms for x86 variants (or `#[cfg]`); compile all 3 targets |
| InjectIrq overload confuses future readers | Low×Low | Explicit doc-comment on the x86 branch |

## Security Considerations
- Capability gate is the trust boundary (P01): only a manifest-declared hypervisor cell on an
  SVM/VMX-enabled kernel gets the cap. Deny-by-default, audited.
- `ViVmExit` carries no host pointers — only guest ports/GPA/regs — so a cell cannot learn host addresses.
- VM_REGISTRY keyed by owner tid (existing): a cell drives only VMs it created; Drop-on-death frees all.

## Next Steps
- P05 builds the x86 platform personality in the hypervisor cell (PVH loader, UART, PIC, PIT) to boot
  Alpine to a shell.
