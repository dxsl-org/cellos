# Phase 05 — Hypervisor cell x86 personality: PVH loader + boot info + 16550 UART + 8259 PIC + PIT (no-LAPIC MVP) → **BOOT ALPINE (M2)**

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-04](phase-04-vmexit-abi-registry.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-05-hypervisor-cell-boot-alpine.md`
- Verified: `cells/services/hypervisor/src/run_loop.rs:25-160` (dispatch loop to extend), `:165` (advance_pc);
  `cells/services/hypervisor/src/main.rs`, `loader_image.rs` (ARM Image parser to parallel),
  `dtb.rs`/`pl011.rs`/`gicd.rs`/`psci.rs`/`timer.rs` (ARM device models — x86 gets its own set);
  `cells/services/hypervisor/src/vmm.rs:1-101` (syscall wrappers — arch-generic, reused).
- Research: #4 (PVH entry contract + Alpine vmlinux extraction), #5 (no-LAPIC MVP).
- **This is the central milestone of the whole plan.**

## Overview
- **Priority:** P1 · **Status:** P05a code-complete (all cell modules + HAL rework; compiles x86_64/aarch64/riscv64; M1 re-PASS on +svm) · **P05b pending:** Alpine vmlinux artifact + QEMU boot-to-shell iteration · **Depends on:** 04

> ### P05a — code-complete 2026-07-23 (compiles + M1 non-regressed)
> **HAL rework (SVM simplifications for PVH boot):**
> - Dropped **CR0-write intercept** (NPT drives paging on SVM; CR0.PG trap is VMX-only) and **CPUID intercept** (passthrough → consistent qemu64 view). Only INTR/HLT/IOIO/MSR + mandatory VMRUN remain.
> - **MSR handled entirely in-kernel** (`svm_vcpu::run`): WRMSR EFER re-asserts SVME; other WRMSR dropped; RDMSR→0. Guest-context syscall/sysenter/segment-base MSRs are **MSRPM-passthrough** (`svm::msrpm_passthrough_boot`) so `VMSAVE`/`VMLOAD` manage them natively — no VMM round-trip.
> - **Instruction-length advance fallback** for no-NRIPS hosts (TCG +svm nRIP=0): HLT=1, MSR=2, VMMCALL=3 (Linux emits these prefix-free). IOIO still advances via EXITINFO2.
> - **SVM EVENTINJ injection** (`svm_vcpu::inject_ext_irq` + `svm_registry::inject_irq`, wired into `registry::inject_irq` x86 branch) — the former x86 no-op. Delivered on guest HLT so jiffies advance.
>
> **Cell x86 personality (`#![forbid(unsafe_code)]`):** `uart_16550.rs`, `pic_8259.rs`, `pit_8253.rs`, `boot_info.rs` (hvm_start_info + e820 + modlist + cmdline), `loader_image_x86.rs` (vmlinux ELF + PHYS32_ENTRY note + sequential PT_LOAD routing), `run_loop_x86.rs`, `boot_x86.rs`; `main.rs` cfg-splits ARM/x86 personalities. cell-build emits the `i386:x86-64` PIE script (no custom .ld). x86 cell builds with `RUSTFLAGS=-C relocation-model=pic`.
>
> **Artifact pipeline:** `scripts/fetch-alpine-x86.sh` (fetch Alpine x86_64 netboot + extract uncompressed vmlinux + assert PVH note) + `scripts/make-hypervisor-fs-x86.sh` (build x86 cells + assemble `embedded-hv-x86/kernel_fs.img`).
>
> ### P05b — in progress 2026-07-23 (pipeline works; cell runs; blocked at cap grant)
> - **Artifacts obtained + verified:** `fetch-alpine-x86.sh` → Alpine 3.21 x86_64 (Linux 6.12.81). Extracted `vmlinux` (38 MB ELF) via upstream `extract-vmlinux` (gunzip @ 17093); **PVH note confirmed, entry=0x1000000**. `make`-side: `kernel/src/embedded-hv-x86/kernel_fs.img` (60 MB) = /bin/{init,shell,hypervisor}+/vmlinux+/initrd.gz — **mkfat32 needs `MSYS_NO_PATHCONV=1`** (Git Bash mangled `/bin/…` → `C:/…`). service-vfs/config/net not needed (littlefs C dep won't x86-cross; hv reads /vmlinux via kernel OpenCap/ReadCap on VIFS1).
> - **Boot chain works:** 61 MB kernel (`EMBEDDED_OVERRIDE=kernel/src/embedded-hv-x86`) → Limine ISO → `-cpu qemu64,+svm -accel tcg -m 2048` → VIFS1 mounts → init spawns `/bin/hypervisor` → **`[hv-x86] hypervisor cell starting (SVM PVH)`** runs with SVM root active.
> - **"cap bug" FIXED (was a syscall stub, not a cap issue):** the cell's `vmm.rs::syscall4` was aarch64-only — non-aarch64 returned ERR without issuing the syscall, so CreateVm never reached the kernel on x86. Added the x86_64 `syscall` path (RAX=id, RDI/RSI/RDX/R10; clobber RCX/R11). Loader granted hv correctly all along (tid=2, granted.hv=true). Now `CreateVm result ok=true`.
> - **Loader fixed** for the real vmlinux: PVH note at file off 21.6 MB (beyond the 256 KB prefix) — `load_segments` now captures the note during its single streaming pass and returns entry32=0x1000000. Segments load clean (paddr 16/40/46 MB, kernel_end 0x322c000, initrd @ 0x3400000).
> - **Early #PF root-caused + FIXED (world-switch bug):** faulting insn was `mov %rax,%gs:…` (per-CPU write) with GS.base=0 — `world_switch.rs` never `VMSAVE`d the guest after VMRUN, so the guest's `wrmsr GS_BASE` was lost on every exit. Added `vmsave` guest post-VMRUN. Unlocked real boot.
> - **M2-A (Option A — emulate ACPI+LAPIC) — deep progress, one wall left:** guest now prints **`Linux version 6.12.81`**, e820, earlycon, full ACPI (RSDP→XSDT→FADT→DSDT→MADT, mine), `x2apic: enabled by BIOS`, PCI enum, clocksource. Added cell `acpi.rs`/`lapic.rs`(x2APIC MSR+timer)/`cmos_rtc.rs`; HAL CPUID intercept (force x2APIC + ARCH_CAP), APIC_BASE/ARCH_CAP/XAPIC_DISABLE_STATUS MSR emulation, x2APIC MSRs surfaced to cell; cmdline `earlycon=… console=ttyS0 x2apic_phys rdinit=/bin/sh panic=1`.
> - **CURRENT WALL:** kernel disables x2APIC (`IRQ remapping doesn't support X2APIC mode`) → falls back to xAPIC **MMIO 0xFEE00000** → NPF → shutdown. 4 `x2apic_hw_locked()`-emulation attempts didn't keep x2APIC (can't `log::` in hal-x86 to see why). **Next:** either debug hw_locked, or emulate the xAPIC MMIO window (map-trap + minimal `mov` decode via SVM DecodeAssist) — the robust path. See memory `project-tier3b-x86-vmm-progress` for full detail.
- Add the x86 platform personality to the existing `cells/services/hypervisor/` Tier-1 cell
  (`#![forbid(unsafe_code)]`): a **PVH** guest loader, an `hvm_start_info` boot-info builder, an emulated
  **16550 UART** (port 0x3F8), an **8259 PIC** + **8253/8254 PIT**, and the run-loop x86 dispatch arms.
  Success = **M2**: Alpine x86_64 boots to a busybox `/ #` prompt over the emulated serial, driven
  entirely from the userspace cell, with `nolapic noapic acpi=off` (no LAPIC/ACPI emulation). No virtio
  yet (initramfs rootfs).

## Key Insights
- **PVH entry contract (research #4 — concrete):** enter at the kernel's 32-bit-protected-mode
  PHYS32_ENTRY (paging OFF), NOT BIOS/real-mode. VMM guest state (set in the VMCB/VMCS via P03/P04):
  `CR0=0x11 (PE|ET)`, `CR4=0`, `EFER=0`, `EFLAGS=0x2`, flat GDT {null, code `0xc09b`, data `0xc093`,
  TSS `0x008b` limit 0x67}, `CS=0x08 DS=SS=ES=0x10` (base 0, limit 4G), `RIP=PHYS32_ENTRY`,
  `RBX=hvm_start_info paddr`. The **guest kernel does its own long-mode bring-up** — the VMM does NOT
  build guest page tables (contrast bzImage fallback).
- **⚠️ vmlinux extraction (research #4 — load-bearing):** Alpine ships PVH support
  (`CONFIG_XEN_PVH=y → select PVH`, aports `lts.x86_64.config:52`, `virt.x86_64.config:51`), so the
  PHYS32_ENTRY note EXISTS — but only in the **uncompressed `vmlinux` ELF**, NOT the shipped
  `vmlinuz-*` bzImage. P05 must obtain `vmlinux`: extract via `scripts/extract-vmlinux` from the
  bzImage, or build/download an uncompressed image. Detect the entry via `readelf -n` (note name
  `"Xen"`, type 18 = `XEN_ELFNOTE_PHYS32_ENTRY`). **Fallback** if no PVH note: 64-bit Linux/bzImage
  protocol (VMM owns initial page tables + long-mode + zero-page) — heavier, in Risk Assessment.
- **`hvm_start_info` (research #4):** `magic=0x336ec578`, `version=1`, `flags=0`, `nr_modules`,
  `modlist_paddr` (→ `hvm_modlist_entry{paddr,size,cmdline_paddr,reserved}` for initramfs),
  `cmdline_paddr`, `memmap_paddr` + `memmap_entries` (→ `hvm_memmap_table_entry{addr,size,type,reserved}`
  e820-style), `rsdp_paddr`. All placed in low guest RAM.
- **ACPI/RSDP unneeded for M2 (research #4/#5 synergy):** cmdline `acpi=off nolapic noapic` → Linux
  skips ACPI + APIC probing entirely → `rsdp_paddr = 0`, no ACPI tables built. ACPI-at-0xE0000
  (Firecracker pattern) is required only if P09 drops `acpi=off`.
- **No-LAPIC MVP (research #5 — the big simplification):** emulate ONLY:
  - **16550 UART @0x3F8** (port I/O): on `PortOut` to THR forward the byte to ViCell serial via
    `println`/log; `PortIn` of LSR(0x3FD) returns THRE|TEMT (tx-ready); IER/FCR/LCR/MCR modeled minimally.
  - **8259 PIC** (0x20/0x21 master, 0xA0/0xA1 slave): accept ICW1-4 init sequence + OCW1 mask; track
    IRR/ISR; deliver via `sys_inject_irq(vector)` when the PIT (or later virtio) IRQ fires and unmasked.
  - **8253/8254 PIT** (0x40-0x43, gate 0x61): model counter 0 (timer). Load-bearing even without an IRQ
    because **Linux calibrates TSC via a PIT fallback** when CPUID 0x15 is absent. Provide a monotonic
    count so calibration converges. IRQ0 (vector 0x20 after PIC remap) injected on `Hlt`/`Preempted`.
- **Run-loop x86 arms (extend `run_loop.rs`, cfg-split or personality trait):** `PortOut`→UART/PIC/PIT
  write; `PortIn`→UART/PIC/PIT read (write reg via `vcpu_regs`, advance PC); `Hlt`→inject IRQ0 if PIT
  due (analog of ARM `Wfi`→timer); `MmioRead`/`MmioWrite`→virtio window (P06) else default-arm log;
  `Preempted`→service IPC + re-enter; `Shutdown`→teardown. Guest shutdown on x86 (no PSCI): detect the
  ACPI/`0x604`/triple-fault or a `PortOut` to QEMU's exit port convention, OR simply the guest halting
  forever → surface as `Shutdown`.
- **Instruction-length PC advance:** unlike ARM's fixed +4 (`run_loop.rs:165`), x86 instructions are
  variable-length. The P03 decoder must carry the exit instruction length (VMX exit-instruction-length
  field / SVM nRIP) so the x86 `advance_pc` adds the right delta.
- **Law 2/4/5/6/8:** cell stays `#![forbid(unsafe_code)]`; device models are safe Rust; modules < 200
  lines; `Vi`-prefixed public types; the cell needs its own `.ld` (per cell-heap gotcha).

## Requirements
**Functional**
- `loader_image_x86.rs`: parse `vmlinux` ELF, find PHYS32_ENTRY note, load PT_LOAD segments + initramfs
  into guest RAM via `sys_write_guest_memory`; place `hvm_start_info` + e820 + cmdline; set entry + RBX.
- `boot_info.rs`: build `hvm_start_info`, memmap (RAM 0..size minus MMIO holes), modlist (initramfs).
- `uart_16550.rs` (cell): emulated 16550 register model (THR/LSR/IER/…); byte → ViCell serial.
- `pic_8259.rs`: init-sequence + mask model; vector delivery via `sys_inject_irq`.
- `pit_8253.rs`: counter-0 monotonic model for TSC calibration + IRQ0.
- `run_loop.rs` x86 arms for PortIn/PortOut/Hlt/Mmio/Preempted/Shutdown; x86 `advance_pc(len)`.
- cmdline: `console=ttyS0 earlyprintk=serial,ttyS0,115200 acpi=off nolapic noapic rdinit=/bin/sh panic=1`.

**Non-functional**
- Law 4 cell `#![forbid(unsafe_code)]`; privilege via syscalls only. Cell `.ld` (bases map). Reuse
  arch-generic `vmm.rs` wrappers unchanged.

## Architecture
```
hypervisor cell (x86 personality):
  load_guest(): parse vmlinux PVH note → write PT_LOADs + initramfs + start_info + e820 (sys_write_guest_memory)
  create_vcpu(entry=PHYS32_ENTRY); vcpu_regs{ RBX = start_info_gpa }   // + P03 sets CR0/EFLAGS/GDT
  run loop (SCHED_TICK budget):
    PortOut 0x3f8         → uart.write(byte) → ViCell serial
    PortIn  0x3fd (LSR)   → return THRE|TEMT
    PortOut/In 0x20/0xA0  → pic.command/mask
    PortOut/In 0x40-0x43  → pit.counter
    Hlt                   → if pit_due { sys_inject_irq(0x20) }        // IRQ0 timer
    Mmio @ virtio window  → (P06) else default-arm log                 // m2 analog
    Preempted             → service IPC; re-enter
    Shutdown              → teardown + exit
```
Modules (Law 5, <200 lines each): `loader_image_x86.rs`, `boot_info.rs`, `uart_16550.rs`, `pic_8259.rs`,
`pit_8253.rs`, `run_loop.rs` (extended). ARM modules (`dtb.rs`/`pl011.rs`/`gicd.rs`/`psci.rs`) stay
`#[cfg(target_arch="aarch64")]`.

## Related Code Files
**Create**
- `cells/services/hypervisor/src/loader_image_x86.rs` — vmlinux ELF + PVH-note parse + segment/initramfs placement
- `cells/services/hypervisor/src/boot_info.rs` — hvm_start_info + e820 memmap + modlist builder
- `cells/services/hypervisor/src/uart_16550.rs` — emulated 16550 register model
- `cells/services/hypervisor/src/pic_8259.rs` — 8259 PIC init + mask + vector delivery
- `cells/services/hypervisor/src/pit_8253.rs` — 8253/8254 PIT counter-0 model
- `cells/services/hypervisor/hypervisor-x86.ld` — x86 cell linker script (bases map, page-aligned RO)
- `scripts/extract-vmlinux-alpine.*` — obtain uncompressed vmlinux + verify PVH note (`readelf -n`)
**Modify**
- `cells/services/hypervisor/src/run_loop.rs` — add x86 dispatch arms (cfg-split personality); x86 `advance_pc(instr_len)`
- `cells/services/hypervisor/src/main.rs` — cfg-select x86 loader + device set
- `cells/services/hypervisor/Cargo.toml` — x86 target deps if any (ELF parse: reuse existing loader crate or minimal parser)
- workspace cell build manifest / bases map / embed list

## Implementation Steps
1. `scripts/extract-vmlinux-alpine`: fetch Alpine, extract `vmlinux`, assert `readelf -n` shows the Xen
   PHYS32_ENTRY note; pin + checksum.
2. `loader_image_x86.rs`: parse ELF PT_LOADs → `sys_write_guest_memory`; place initramfs; find note → entry.
3. `boot_info.rs`: build `hvm_start_info` (magic/version), e820 memmap (RAM minus MMIO holes), modlist,
   cmdline; place at low GPA; return start_info_gpa for RBX.
4. `uart_16550.rs`: THR write → serial; LSR read → THRE|TEMT; model IER/FCR/LCR/MCR minimally.
5. `pic_8259.rs`: ICW1-4 + OCW1 mask; IRR/ISR; `deliver()` → `sys_inject_irq(vector)` when unmasked.
6. `pit_8253.rs`: counter-0 monotonic; on `Hlt` if due, request IRQ0 (vector 0x20 post-remap).
7. `run_loop.rs`: x86 arms; variable-length `advance_pc(instr_len)` from decoder.
8. Validate incrementally: (a) `earlyprintk=serial` bytes appear via UART; (b) TSC calibration completes
   (no "TSC calibration failed" / hang); (c) full boot to `/ #`. Compare against a reference
   `qemu-system-x86_64 -kernel vmlinux -append ...` boot log.

## Todo List
- [ ] extract-vmlinux script + PVH-note verification (readelf -n type 18 "Xen")
- [ ] loader_image_x86: ELF PT_LOAD + initramfs placement, PHYS32_ENTRY discovery
- [ ] boot_info: hvm_start_info + e820 + modlist + cmdline (rsdp_paddr=0, acpi=off)
- [ ] emulated 16550 UART (earlyprintk output works)
- [ ] 8259 PIC init + mask + vector delivery
- [ ] 8253 PIT counter-0 (TSC calibration converges) + IRQ0 on Hlt
- [ ] run_loop x86 arms + variable-length advance_pc
- [ ] **Alpine boots to busybox `/ #`** (ready for P06 virtio)

## Success Criteria
- On `qemu-system-x86_64 -cpu qemu64,+svm -accel tcg -m 1G`, the hypervisor cell boots Alpine x86_64
  (PVH `vmlinux` + initramfs) to a busybox `/ #` prompt on ViCell serial within ~180s (TCG). `uname -a`
  prints the Linux x86_64 banner.
- Guest halt/`poweroff` → run loop surfaces `Shutdown` → VM teardown → cell exits cleanly (guest RAM
  freed via `reap_vms_for_task`; ViCell shell still alive).
- No LAPIC/ACPI emulation exists yet and boot still succeeds (proves the `nolapic acpi=off` MVP path).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| **Alpine vmlinux lacks PVH note / extraction fails** | Med×High | Verify `readelf -n` in the fetch script; fallback = bzImage 64-bit protocol (VMM builds page tables + zero-page) — documented, larger scope |
| TSC calibration fails without LAPIC/PM-timer → boot hang | Med×High | PIT counter-0 monotonic model (Linux PIT-fallback calibration); verify against reference boot log |
| No console output (wrong port/cmdline) | Med×High | `console=ttyS0 earlyprintk=serial`; emulate LSR THRE exactly; bisect with earlyprintk |
| PIC init sequence mismodeled → IRQ delivery broken | Med×Med | Full ICW1-4 state machine; trace port writes vs QEMU 8259 |
| Variable-length PC advance wrong → guest runs garbage | Med×High | Use decoder instr-length (VMX field / SVM nRIP), never a fixed delta |
| TCG boot exceeds CI timeout | Med×Med | 180s timeout; minimal initramfs; KVM-accel note (P10) |
| x86 guest-shutdown detection ambiguous (no PSCI) | Low×Med | Detect QEMU exit-port / triple-fault /永-halt; surface Shutdown |

## Security Considerations
- Guest reaches UART/PIC/PIT only as **trapped I/O** — never real hardware; the cell mediates every
  byte. ViCell's real 16550 driver (`hal/arch/x86/src/x86_64/uart_16550.rs`) is untouched; guest output
  is forwarded via log/serial, not raw port writes from the cell.
- SAS invariant (P02): guest kernel/initramfs/boot-info all live inside the carved guest-RAM region; the
  cell writes them via the bounds-checked `sys_write_guest_memory` path, never touching host frames.
- **m2 analog:** the run loop x86 match MUST have an explicit default arm for unregistered ports/MMIO —
  log port/size/dir, return without silent fall-through.
- Capability gate (P01/P04): only this manifest-declared cell creates the VM.

## Next Steps
- P06 adds virtio-mmio transport + virtio-console (reusing the arch-generic stack) — proper console and
  the foundation for blk/net.
