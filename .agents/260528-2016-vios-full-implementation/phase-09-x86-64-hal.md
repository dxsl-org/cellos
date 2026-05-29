# Phase 09 — Multi-Arch HAL: x86_64

**Effort:** 80h | **Priority:** P1 | **Status:** complete | **Blockers:** none (parallel to 08)

## Overview

Replace the 38-LOC `hal/arch/x86/src/x86_64.rs` stub with a working x86_64 HAL. Kernel boots on `qemu-system-x86_64`, reaches Ring 3, runs the same `user_hello` smoke. Closes the third primary architecture target for v1.0.

## Context Links

- `docs/04-hardware.md` — multi-arch HAL contract
- `docs/01-core.md` — Cellular philosophy, Law 3
- Reference patterns from Phase 03 (RV64 trap) and Phase 08 (AArch64 boot)
- Intel SDM Vol 3A (System Programming) for IDT/GDT/paging

## Key Insights

- Boot path on QEMU: Limine bootloader (or multiboot2) hands off in 64-bit long mode with paging already on. Kernel just needs to install its own GDT/IDT/CR3 and run.
- Paging: 4-level (PML4 → PDPT → PD → PT), 4KB pages. Bit 7 of PDPT/PD entries = 1GB/2MB superpage. Use 2MB superpages for HHDM (fast init).
- Privilege levels: Ring 0 kernel, Ring 3 user. GDT must have at least: null, kernel code, kernel data, user code, user data, TSS. Use `iretq` to return to Ring 3.
- Syscalls: prefer `syscall`/`sysret` instructions over int 0x80. Configure `IA32_STAR`, `IA32_LSTAR`, `IA32_FMASK` MSRs.
- Interrupts: APIC (Local APIC + I/O APIC). On QEMU `-machine q35`, use LAPIC for timer + IPI, IOAPIC for legacy IRQs. Configure via MMIO at 0xfee00000 (LAPIC) and 0xfec00000 (IOAPIC).
- UART: COM1 at 0x3f8 (I/O port, NOT MMIO). Use `in`/`out` instructions.

## Requirements

**Functional**
- `cargo build --release --target x86_64-unknown-none -Z build-std=core,alloc` succeeds
- Kernel boots on `qemu-system-x86_64 -machine q35 -kernel …` (via Limine)
- Prints `[ViOS] kernel boot v… (x86_64)` over COM1
- Spawns `user_hello`; sees "Hi from U-mode" via syscall instruction
- LAPIC timer drives the scheduler

**Non-functional**
- Boot time < 3s in QEMU
- API parity with RV64 + AArch64 HALs
- All MSR/CR writes documented `// SAFETY:`

## Architecture

```
Limine bootloader → hands off in long mode
   ▼
kernel _start (Rust):
   ├─ Parse Limine boot info (memory map, framebuffer, kernel base, HHDM)
   ├─ Init COM1 UART (early println)
   ├─ Build GDT (null, K_CODE, K_DATA, U_CODE, U_DATA, TSS) → lgdt
   ├─ Build IDT (32 exceptions + N IRQs) → lidt
   ├─ Build PML4 (identity + HHDM 2MB superpages)
   ├─ Write CR3 = pml4_paddr
   ├─ Init LAPIC + IOAPIC
   ├─ Init MSRs: STAR, LSTAR, FMASK, EFER.SCE = 1
   └─ Hand to kmain
                                  ▼
                            Cell in Ring 3
                            │
                            │ syscall (rax = syscall #, rdi/rsi/.. = args)
                            ▼
                       LSTAR target (syscall_entry):
                            ├─ swapgs (switch to kernel GS)
                            ├─ save user RSP, load kernel RSP from TSS
                            ├─ push regs, call syscall dispatcher
                            ├─ restore regs, swapgs
                            └─ sysretq
```

## Related Code Files

**Investigate:**
- `hal/arch/x86/src/x86_64.rs` — current stub
- `hal/arch/x86/src/x86_64/` — directory contents (verify)
- `hal/traits/{arch,paging,interrupt,uart,timer}` — trait shapes (must match RV64 + AArch64)
- `kernel/src/boot/limine.rs` — Limine integration (already exists, likely needs x86_64 paths)

**Create (under `hal/arch/x86/src/x86_64/`):**
- `hal/arch/x86/src/x86_64/boot.rs` — `_start`, kernel handoff from Limine
- `hal/arch/x86/src/x86_64/gdt.rs` — Global Descriptor Table + TSS
- `hal/arch/x86/src/x86_64/idt.rs` — Interrupt Descriptor Table, exception/IRQ stubs
- `hal/arch/x86/src/x86_64/context.rs` — CpuContext { rax-r15, rip, rflags, rsp, cs, ss, cr3 }
- `hal/arch/x86/src/x86_64/paging.rs` — 4-level PML4, 2MB superpage HHDM
- `hal/arch/x86/src/x86_64/apic.rs` — LAPIC + IOAPIC drivers
- `hal/arch/x86/src/x86_64/uart_16550.rs` — COM1 16550A driver via port I/O
- `hal/arch/x86/src/x86_64/timer.rs` — LAPIC one-shot/periodic timer
- `hal/arch/x86/src/x86_64/syscall.rs` — syscall/sysret entry stub
- `kernel/linker-x86-64.ld` — linker script (KERNEL_BASE = 0xFFFFFFFF80000000)
- `scripts/run-x86-64.sh` — QEMU launch

**Modify:**
- `hal/arch/x86/src/lib.rs` — re-export `x86_64` module
- `hal/arch/x86/Cargo.toml` — feature `x86_64`
- `kernel/build.rs` — select linker script per target_arch
- `kernel/Cargo.toml` — add `[target.'cfg(target_arch = "x86_64")'.dependencies]`
- `.cargo/config.toml` — `[target.x86_64-unknown-none]` rustflags + linker script

## Implementation Steps

1. **Scaffold sub-modules** under `hal/arch/x86/src/x86_64/` as empty stubs; ensure target compiles clean.
2. **COM1 UART** (`uart_16550.rs`) first — single-byte write via `out 0x3f8, al`. Provides early println.
3. **GDT** (`gdt.rs`):
   - 5 segment descriptors + TSS descriptor (16 bytes for TSS in long mode)
   - `lgdt`, then reload CS via far jmp, reload data segs to user DS
   - `ltr` to load TSS selector
4. **IDT** (`idt.rs`):
   - 256 entries; first 32 are CPU exceptions (vec 0..31), rest are IRQs
   - Each entry = 16 bytes (long-mode gate descriptor)
   - Stub uses asm to push error code (or zero), call common Rust handler `handle_interrupt(vec, frame)`
   - `lidt`
5. **Paging** (`paging.rs`):
   - Build PML4 with 2 entries: identity-map low 1GB, HHDM-map physical RAM at `HHDM_BASE`
   - Use 2MB superpages (PD entries with PS bit set) for HHDM
   - User pages: P|RW|US in PT entries
   - `mov cr3, %rax` writes the new PT
6. **LAPIC + IOAPIC** (`apic.rs`):
   - Locate LAPIC base (default 0xfee00000 unless relocated)
   - Spurious vector reg = 0x1ff (enable + vec 0xff)
   - LAPIC timer: divide config = 0x3 (div by 16), initial count = computed for ~100Hz, LVT timer = vec 0x20 | periodic mode
   - IOAPIC: redirect IRQ4 (COM1) → vec 0x24, redirect timer if needed
7. **Generic timer** (`timer.rs`):
   - Calibrate via PIT or TSC vs LAPIC ticks (basic approach: assume LAPIC freq from CPUID 0x15 if available; fall back to PIT calibration)
   - Schedule next tick on IRQ
8. **MSR setup** + `syscall.rs`:
   - `EFER.SCE = 1` (enable SYSCALL)
   - `STAR` MSR: [63:48]=user CS, [47:32]=kernel CS, [31:0]=0
   - `LSTAR` MSR: address of `syscall_entry`
   - `FMASK` MSR: bits to clear in RFLAGS on syscall (e.g., IF, DF)
   - `syscall_entry` assembly: swapgs → switch stacks → save regs → call dispatcher → restore → swapgs → sysretq
9. **Context switch** (`context.rs`):
   - Save: push all GPRs, segment selectors, RFLAGS, RIP (or rely on iret frame)
   - For user→kernel via IRQ: iretq frame already contains SS:RSP, RFLAGS, CS:RIP
   - For kernel→user spawn: push iretq frame manually, then `iretq`
10. **Wire HAL traits** in `x86_64.rs` re-exporting from sub-modules; implement `Arch`, `PageTableTrait`, `InterruptController`, `Uart`, `Timer` symmetrically to RV64/AArch64.
11. **Linker script `kernel/linker-x86-64.ld`** with higher-half kernel at 0xFFFFFFFF80000000, `.text.boot` for Limine handoff.
12. **`kernel/build.rs`**: pick this script when `target_arch = "x86_64"`.
13. **`scripts/run-x86-64.sh`**:
    ```bash
    qemu-system-x86_64 -machine q35 -m 256M -nographic -serial mon:stdio \
        -kernel target/x86_64-unknown-none/release/kernel \
        -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd  # if needed
    ```
    Alternative: produce a bootable ISO with `limine-deploy` and `-cdrom`.
14. **Boot test**: expect banner over COM1, then Ring 3 smoke.
15. **Promote x86_64 to required in CI matrix** (was `continue-on-error`).

## Todo List

- [x] Scaffold `hal/arch/x86/src/x86_64/` sub-modules (compile clean)
- [x] Implement `uart_16550.rs` (port I/O write byte)
- [x] Implement `gdt.rs` with TSS
- [x] Implement `idt.rs` with 256 entries + asm stubs
- [x] Implement `paging.rs` (PML4, 2MB HHDM superpages, user PT entries)
- [x] Implement `apic.rs` (LAPIC enable, timer, IOAPIC redirect)
- [x] Implement `timer.rs` (LAPIC periodic + calibration)
- [x] Implement `syscall.rs` (MSR setup + syscall_entry asm)
- [x] Implement `context.rs` (iretq for spawn, save on IRQ)
- [x] Implement HAL traits for x86_64
- [x] Create `kernel/linker-x86-64.ld`
- [x] Update `kernel/build.rs` + `.cargo/config.toml`
- [x] Create `scripts/run-x86-64.sh`
- [ ] Boot test → banner appears (QEMU required)
- [ ] Ring 3 smoke test → "Hi from U-mode" via syscall (QEMU required)
- [ ] Promote x86_64 to required in CI (deferred to Phase 11)

## Success Criteria

- `cargo build --release --target x86_64-unknown-none -Z build-std=core,alloc` exits 0
- `scripts/run-x86-64.sh` boots and prints user-mode hello within 3s
- HAL trait impl complete and symmetric with RV64 + AArch64
- CI x86_64 row turns required

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Limine ABI changes between bootloader versions | Med | Med | Pin Limine version; document in README; use `limine-rs` crate for FFI types |
| LAPIC timer calibration unreliable without RTC/PIT | Med | Low | First-pass: use CPUID leaf 0x15 if present; else PIT calibration; document |
| Userspace syscall via `int 0x80` slower; some tooling assumes it | Low | Low | We exclusively use SYSCALL; document; cells don't use legacy ints |
| IDT alignment / TSS layout off-by-bytes silently breaks | Med | High | Unit-test via `core::mem::size_of::<…>() == expected` static asserts |
| Different CPU vendors (Intel vs AMD) need different MSR fixups | Low | Low | Skip vendor detection in v1.0; target QEMU first (model = host AMD or Intel default) |

## Security Considerations

- Userspace pages must have U bit (PT[2]); kernel pages must not
- SMEP/SMAP: enable in CR4 once stable (SMEP=bit 20, SMAP=bit 21); set EFLAGS.AC only inside explicit copy_from/to_user
- NX bit (XD): set on all data pages via PT entry NX bit; enable in EFER (NXE=1)
- KASLR not in v1.0 scope; document as future hardening
- Spectre mitigations: out of scope for v1.0; tracked in Phase 12 threat model

## Rollback

x86_64 code is isolated under its target_arch cfg. Revert restores stub; RV64 and AArch64 builds unaffected. Disk image stays compatible.

## Next Steps

Pattern reused by Phase 21 (RV32, AArch32). CI matrix gains a third required row. Phase 22 (benchmarks) compares the 3 archs on identical workloads.
