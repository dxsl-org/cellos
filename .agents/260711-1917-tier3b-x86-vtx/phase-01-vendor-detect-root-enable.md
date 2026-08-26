# Phase 01 — Vendor detect (CPUID) + SVM/VMX root-operation enablement + x86 HypervisorCap gate

## Context Links
- Plan: [plan.md](plan.md) · Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-01-el2-boot-mmu.md`
- Verified: `kernel/src/cpu_features.rs:12` (HAS_EL2 latch), `:18` (detect), `:33` (has_el2);
  `kernel/src/task/cap.rs:46` (HypervisorCap ZST), `:164-165` (arch-aware cap gate);
  `hal/arch/x86/src/hypervisor.rs:9-21` (all-NotSupported stub); `hal/arch/x86/src/x86_64/boot.rs:37`
  (kmain_x86); `hal/arch/x86/src/x86_64/apic.rs:11` (LAPIC phys); `docs/specs/05-application.md:313`.

## Overview
- **Priority:** P1 · **Status:** pending · **Depends on:** —
- Establish the x86 root-of-virtualization: detect the vendor (Intel VMX vs AMD SVM) via CPUID, enter
  **VMX operation** (`VMXON`) or enable **SVM** (`EFER.SVME`) on every CPU at boot, and extend the
  `HypervisorCap` grant so an x86 kernel that entered root operation can hand the cap to a
  manifest-declared hypervisor cell. **No guest runs yet** — success = a test-hook probe confirms the
  CPU is in root operation and the cap is granted. This is the x86 analog of ARM P01 "stay-at-EL2",
  but far smaller: no MMU/vector migration (cells already run ring-3 on the shipped x86 kernel).

## Key Insights
- **Vendor-neutral from the start (CI decision):** SVM ships first because QEMU TCG emulates it and VMX
  it does not (see plan Validation Log). Introduce `enum X86Virt { Svm, Vmx }` latched at boot; both
  backends implement the same internal contract. CPUID: Intel VMX = `CPUID.1:ECX[5]`; AMD SVM =
  `CPUID.8000_0001:ECX[2]`. Vendor string from `CPUID.0` (`GenuineIntel` / `AuthenticAMD`).
- **VMX enablement sequence (research #1):** set `CR4.VMXE` (bit 13); check/lock `IA32_FEATURE_CONTROL`
  (MSR 0x3A) — if lock bit (0) is set by firmware and the VMXON-outside-SMX bit (2) is clear, VMXON
  faults and there is **no recovery** (firmware disabled VT-x) → detect and downgrade to NotSupported
  gracefully. Allocate a 4 KiB, 4 KiB-aligned VMXON region; write the revision ID from
  `IA32_VMX_BASIC[30:0]` (MSR 0x480) into its first dword; execute `VMXON`. **Per-CPU** — must run on
  every AP as it comes online.
- **SVM enablement sequence (research #1):** set `EFER.SVME` (MSR 0xC000_0080 bit 12); check
  `VM_CR.SVMDIS` (MSR 0xC001_0114 bit 4) is clear (else BIOS locked SVM off); allocate a 4 KiB host
  save area and write its physical address to `VM_HSAVE_PA` (MSR 0xC001_0117). No revision-ID dance.
  Simpler than VMX — another reason SVM is the first target.
- **Where in boot:** after paging + GDT + IDT + LAPIC init, before the first Cell spawns — inside
  `kmain` on the x86 path (called from `boot.rs:37 kmain_x86`). Mirror `cpu_features::detect()` which is
  already the single boot-time feature-latch call site (`cpu_features.rs:18`).
- **Cap gate is a one-line OR (verified `cap.rs:164-165`):** today
  `m.has_hypervisor() && (has_h_ext() || has_el2())`. Add `has_x86_virt()` to the OR. `MANIFEST_FLAG_
  HYPERVISOR` + `HypervisorCap` ZST already exist from the ARM track — **no manifest/flag change.**
- **Law 4:** all `VMXON`/`WRMSR`/`CR4` writes are `unsafe` in HAL with `// SAFETY:`; cells never touch
  these. VMXON region + HSAVE area are kernel-owned frames, never mapped into any guest.

## Requirements
**Functional**
- `cpu_features::detect_x86()` (x86_64 only): CPUID vendor + VMX/SVM feature bits; latch `X86Virt` +
  `HAS_X86_VIRT: AtomicBool`. Graceful `false` if firmware-locked-off.
- `hal::x86_64::vmx::enter_root()` and `hal::x86_64::svm::enable()` — per-CPU root-operation entry with
  full precondition checks; return `ViResult<()>`.
- `cpu_features::has_x86_virt()` accessor; `false` on non-x86.
- Cap gate (`cap.rs:164`) grants `HypervisorCap` on x86 when manifest declares it AND root operation is
  live.

**Non-functional**
- Idempotent + AP-safe (SMP): each CPU enters root operation exactly once; re-entry is a no-op.
- Law 4: `// SAFETY:` on every privileged instruction/MSR write.

## Architecture
```
kmain (x86 path)
  ├─ cpu_features::detect_x86()      // CPUID → X86Virt{Svm|Vmx} + HAS_X86_VIRT latch
  ├─ match X86Virt:
  │     Svm → svm::enable()          // EFER.SVME, VM_HSAVE_PA, check VM_CR.SVMDIS
  │     Vmx → vmx::enter_root()      // CR4.VMXE, FEATURE_CONTROL, VMXON(revid region)
  └─ (per-AP: same call in AP bringup path)
loader / cap.rs::grant → HypervisorCap iff has_hypervisor() && has_x86_virt()
```
No guest structures here — just root operation. VMCS/VMCB allocation is P03.

## Related Code Files
**Create**
- `hal/arch/x86/src/x86_64/vmx.rs` — `enter_root()`, VMXON region alloc, capability-MSR readers (`// SAFETY:`)
- `hal/arch/x86/src/x86_64/svm.rs` — `enable()`, HSAVE area alloc, `VM_CR`/`EFER` writers (`// SAFETY:`)
**Modify**
- `kernel/src/cpu_features.rs` — add `HAS_X86_VIRT` + `X86_VIRT` statics, `detect_x86()`, `has_x86_virt()`; call from `detect()` under `#[cfg(target_arch="x86_64")]`
- `kernel/src/task/cap.rs:164-165` — extend OR with `crate::cpu_features::has_x86_virt()`
- `hal/arch/x86/src/x86_64.rs` — wire `pub mod vmx; pub mod svm;`
- x86 AP-bringup path — call root-enable per AP (verify SMP entry point)

## Implementation Steps
1. `detect_x86()`: read `CPUID.0` vendor; `CPUID.1:ECX[5]` (VMX) / `CPUID.8000_0001:ECX[2]` (SVM);
   latch `X86Virt` + `HAS_X86_VIRT`. Prefer SVM if both present under `-accel tcg` (config knob), else
   native vendor.
2. `vmx::enter_root()`: `CR4.VMXE=1`; read `IA32_FEATURE_CONTROL` — if locked && !vmxon-outside-SMX →
   return `NotSupported` (do not fault); else set lock+bit2 if unlocked; alloc VMXON region, write
   `IA32_VMX_BASIC` revid; `VMXON`. Check RFLAGS.CF/ZF for failure.
3. `svm::enable()`: check `VM_CR.SVMDIS==0`; `EFER.SVME=1`; alloc HSAVE; `VM_HSAVE_PA` = HSAVE phys.
4. Call the matching enable in `kmain` after LAPIC init; replicate on AP bringup.
5. Extend `cap.rs:164` OR-chain; add compile-time `cfg` so non-x86 keeps existing behaviour.
6. **test-hook probe:** read `CR4.VMXE`/`EFER.SVME` back and log `root operation active: <vendor>`.

## Todo List — code complete 2026-07-23 (compiles clean on x86_64-unknown-none)
- [x] `detect_x86()` CPUID vendor + VMX/SVM bit latch — `cpu_features.rs` `X86Virt`/`x86_virt_kind()`,
      SVM preferred over VMX (TCG-testable). NOTE: implemented as `x86_virt_kind()` returning the
      CPUID-advertised vendor; the `HAS_X86_VIRT` latch is set only AFTER root op actually enters.
- [x] `vmx::enter_root()` — `hal/arch/x86/src/x86_64/vmx.rs`: CR4.VMXE, FEATURE_CONTROL firmware-lock
      graceful downgrade (returns NotSupported, never faults), revid-stamped VMXON region
- [x] `svm::enable()` — `hal/arch/x86/src/x86_64/svm.rs`: EFER.SVME + VM_HSAVE_PA, VM_CR.SVMDIS check
- [x] Root-enable wired into `kmain` (BSP) — `main.rs` x86 block after `init_timers()`; non-fatal on
      failure (cap simply stays closed). **AP bringup wiring still TODO** (per-CPU idempotent enable).
- [x] `cap.rs` OR-chain grants HypervisorCap when `has_x86_virt()` true (latched post-root-enable)
- [ ] test-hook probe on QEMU `-cpu qemu64,+svm -accel tcg` — **PENDING**: default run-x86.ps1 uses
      `+pdpe1gb` (no `+svm`), and verifying the SVM-active log needs an ISO rebuild (WSL+limine+xorriso).
      On the current no-`+svm` config the code takes the graceful "not supported; cap closed" path.
- [ ] AP-bringup per-CPU root enable (BSP-only today)

## Success Criteria
- On `qemu-system-x86_64 -cpu qemu64,+svm -accel tcg`, the test-hook probe prints
  `root operation active: Svm` and a `hypervisor=true` cell is granted `HypervisorCap`.
- On a firmware-VT-x-disabled host (simulated by masking the feature bit), `detect_x86()` yields
  `false`, no VMXON fault occurs, and the cap is denied (logged in audit).
- No regression: existing x86 suites (`tests/integration/tests/*-x86.rs`) stay green; aarch64/riscv
  builds unchanged.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Firmware locked FEATURE_CONTROL → VMXON #GP with no recovery | Med×High | Detect lock+outside-SMX bits BEFORE VMXON; downgrade to NotSupported, never fault |
| SVM disabled by BIOS (VM_CR.SVMDIS) | Med×Med | Check VM_CR before EFER.SVME; graceful NotSupported |
| AP enters root op with stale/shared region | Low×High | Per-CPU VMXON region + HSAVE area (one frame per CPU); assert alignment |
| CR4.VMXE set but TCG lacks the vendor path | Med×Med | Prefer SVM under TCG (verified emulated); VMX only asserted on KVM/HW (P09) |
| Wrong revision ID in VMXON region | Low×High | Read IA32_VMX_BASIC[30:0] at runtime, never hard-code |

## Security Considerations
- Root operation is the deepest privilege on the machine; entering it is kernel-only and gated by the
  Kernel Boundary Law (hardware-mandated, EL0-equivalent ring-0 instruction). Cells cannot invoke it.
- VMXON region + HSAVE area are kernel frames excluded from any guest EPT/NPT (enforced in P02).
- Cap remains deny-by-default: a cell without `hypervisor=true` never receives the ZST token.

## Next Steps
- P02 builds the EPT/NPT nested-paging tree + guest-RAM carve on top of active root operation.
