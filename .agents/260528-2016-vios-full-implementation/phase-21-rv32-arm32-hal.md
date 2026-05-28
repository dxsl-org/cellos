# Phase 21 — RV32 & ARM AArch32 HAL

**Effort:** 160h | **Priority:** P3 | **Status:** pending | **Blockers:** Phase 08

## Overview

Bring up the two 32-bit architectures as production HAL targets: RISC-V 32 (RV32) and ARM AArch32 (32-bit ARMv7-A). Reuses architectural patterns from Phases 03 (RV64) and 08 (AArch64); the work is largely paging-format and CPU-mode differences. After this phase, ViOS boots on 5 architectures total, covering embedded-class 32-bit targets.

## Context Links

- `docs/04-hardware.md` — multi-arch HAL design
- `hal/arch/riscv/src/rv32.rs` — 4-LOC stub
- `hal/arch/arm/src/aarch32.rs` — 3-LOC stub
- Reference patterns: `hal/arch/riscv/src/rv64/` (RV64) and Phase 08's `aarch64/`
- Law 3 (multi-arch awareness, VAddr/PAddr) — critical here

## Key Insights

### RV32 specifics
- 32-bit VAddr/PAddr. Confirm `libs/types::VAddr` is `usize`-based (works on both 32 and 64) — Law 3.
- Paging: SV32 (2-level, 4 KB pages, 4 MB superpages). PTE = 32-bit. PPN = 22 bits + 10 bits flags.
- Privilege: M-mode (firmware) → S-mode (kernel) → U-mode (cells). Same as RV64 minus address width.
- Interrupt controller: many RV32 SoCs use CLINT (not PLIC). For QEMU virt32, PLIC also available.
- ABI: ILP32. `int = long = pointer = 32 bits`. Compiler target `riscv32imac-unknown-none-elf`.

### ARM AArch32 specifics
- ARMv7-A; QEMU machine `virt` supports it (or `vexpress-a9` / `realview-pba8`)
- 32-bit registers (R0-R15), Thumb-2 instruction set
- MMU: short-descriptor format (Sv7) — 2-level, 1 MB sections + 4 KB small pages
- Coprocessor CP15 for MMU control (TTBR0, TTBR1, SCTLR, DACR, etc.)
- Interrupt: GIC-400 (same as AArch64 but with different access conventions via CP15)
- ABI: AAPCS, R0-R3 args, R0 return; floating-point via VFPv3 (optional)

## Requirements

**Functional**
- `cargo build --release --target riscv32imac-unknown-none-elf -Z build-std=core,alloc` succeeds
- `cargo build --release --target thumbv7em-none-eabihf -Z build-std=core,alloc` succeeds (or `armv7a-none-eabi`)
- Both kernels boot on QEMU virt with appropriate `-machine` / `-cpu`
- Banner appears on UART
- Ring 3 / EL0 smoke test passes (user_hello variant for each arch)

**Non-functional**
- Boot < 3s in QEMU
- Memory footprint: kernel + 3 services < 8 MB on 32-bit (tighter than 64-bit due to address space)
- API parity with 64-bit HALs

## Architecture (both targets)

```
RV32:
   M-mode firmware (OpenSBI rv32) → S-mode kernel
       ├─ SV32 page table (2-level)
       ├─ CLINT or PLIC for interrupts
       ├─ NS16550A UART (or sifive UART)
       └─ U-mode tasks (cells)

AArch32:
   EL2 (rarely; usually skipped on virt32) → EL1 (kernel) → EL0 (cells)
       ├─ Sv7 short-descriptor PT (2-level)
       ├─ GIC-400 via CP15 access
       ├─ PL011 UART
       └─ AAPCS calling convention
```

## Related Code Files

### RV32

**Investigate:**
- `hal/arch/riscv/src/rv32.rs` — current stub
- `hal/arch/riscv/src/rv32/` — verify presence
- `hal/arch/riscv/src/common.rs` + `common/{sbi,timer,uart_ns16550a}.rs` — shared with RV64 (audit for u64 assumptions)

**Create:**
- `hal/arch/riscv/src/rv32/boot.rs` — `_start` for RV32, M→S transition via SBI
- `hal/arch/riscv/src/rv32/context.rs` — CpuContext for RV32 (32 GPRs × 32 bits)
- `hal/arch/riscv/src/rv32/paging.rs` — SV32 2-level PT
- `hal/arch/riscv/src/rv32/trap.rs` — trap handler dispatching to common syscall path
- `kernel/linker-riscv32.ld` — load base typically `0x80000000` for QEMU virt
- `scripts/run-rv32.sh` — QEMU launch

### AArch32

**Investigate:**
- `hal/arch/arm/src/aarch32.rs` — current stub
- `hal/arch/arm/src/aarch32/` — verify presence
- `hal/arch/arm/src/common.rs` — audit for AArch64-only code

**Create:**
- `hal/arch/arm/src/aarch32/boot.rs` — `_start`, ARM/Thumb mode switching, BSS clear, stack
- `hal/arch/arm/src/aarch32/context.rs` — CpuContext (R0..R15, CPSR, SPSR)
- `hal/arch/arm/src/aarch32/paging.rs` — Sv7 short-descriptor PT, CP15 ops
- `hal/arch/arm/src/aarch32/trap.rs` — vector table + handler shims
- `hal/arch/arm/src/aarch32/gic.rs` — GIC-400 via CP15 + MMIO
- `hal/arch/arm/src/aarch32/uart_pl011.rs` — same as 64-bit, but 32-bit MMIO regs
- `kernel/linker-aarch32.ld`
- `scripts/run-aarch32.sh`

**Modify:**
- `hal/arch/riscv/src/lib.rs` — re-export `rv32` behind `rv32` feature
- `hal/arch/arm/src/lib.rs` — re-export `aarch32` behind `aarch32` feature
- `kernel/Cargo.toml` — `[target.'cfg(target_arch = "riscv32")'.dependencies]` and `[target.'cfg(target_arch = "arm")'.dependencies]`
- `kernel/build.rs` — pick linker script per target
- `.cargo/config.toml` — `[target.riscv32imac-unknown-none-elf]`, `[target.thumbv7em-none-eabihf]`
- Phase 02's CI matrix — add both targets (initially `continue-on-error`, promote when green)

## Implementation Steps (Common Pattern)

For each architecture, follow the same 8-step path used in Phases 03 and 08:

1. **Scaffold** sub-module files as empty stubs; ensure `cargo check --target …` compiles.
2. **UART first** for early println: `uart_ns16550a.rs` (RV32) or `uart_pl011.rs` (AArch32). Implement a single-byte write.
3. **Boot stub**: BSS clear, stack setup, branch to Rust `kmain`. For RV32: configure mscratch, switch to S-mode via SBI handoff (or directly from M-mode for bare-metal QEMU virt).
4. **MMU**:
   - RV32: build SV32 root, write `satp` with mode=1 (SV32), `sfence.vma zero,zero`
   - AArch32: configure DACR, build 2-level PT, write TTBR0, set SCTLR.M=1 + I=1 + C=1, `isb`
5. **Trap handler**:
   - RV32: same dispatch shape as RV64 (scause, mcause), narrower regs
   - AArch32: 7-entry vector at high or low VA (configurable via VBAR); each entry is 4 bytes branch
6. **Interrupt controller**: PLIC/CLINT (RV32) or GIC-400 via CP15 (AArch32)
7. **Context switch**: save/restore GPRs (narrower); `mret`+`sret` (RV32) or RFE/SUBS PC, LR (AArch32) to return to U-mode/EL0
8. **HAL trait impls** matching RV64/AArch64 surface
9. **Linker script** with arch-specific load base
10. **QEMU run script**
11. **Boot test → banner**
12. **Ring 3 / EL0 smoke** with arch-specific user_hello blob

## Per-arch QEMU launch (examples)

```bash
# RV32
qemu-system-riscv32 -machine virt -cpu rv32 -m 256M -nographic \
    -bios none -kernel target/riscv32imac-unknown-none-elf/release/kernel

# AArch32
qemu-system-arm -machine virt -cpu cortex-a15 -m 256M -nographic \
    -kernel target/armv7a-none-eabi/release/kernel
```

## Todo List

### RV32
- [ ] Scaffold `hal/arch/riscv/src/rv32/*` stubs (compile)
- [ ] Implement RV32 UART (NS16550A)
- [ ] Implement RV32 boot stub (M/S transition or direct S entry)
- [ ] Implement SV32 2-level paging
- [ ] Implement RV32 trap handler dispatch
- [ ] Implement PLIC or CLINT bind
- [ ] Implement RV32 context switch
- [ ] Implement Arch/PageTable/Interrupt/Uart/Timer traits
- [ ] Create `kernel/linker-riscv32.ld`
- [ ] Update build.rs + .cargo/config.toml + kernel/Cargo.toml
- [ ] Create `scripts/run-rv32.sh`
- [ ] Boot test → banner
- [ ] Ring 3 smoke test
- [ ] Add RV32 to CI matrix

### AArch32
- [ ] Scaffold `hal/arch/arm/src/aarch32/*` stubs (compile)
- [ ] Implement AArch32 PL011 UART
- [ ] Implement AArch32 boot stub (mode setup, BSS, stack)
- [ ] Implement Sv7 short-descriptor paging via CP15
- [ ] Implement AArch32 vector table + handlers
- [ ] Implement GIC-400 via CP15
- [ ] Implement AArch32 context switch
- [ ] Implement HAL traits
- [ ] Create `kernel/linker-aarch32.ld`
- [ ] Update build.rs + .cargo/config.toml + kernel/Cargo.toml
- [ ] Create `scripts/run-aarch32.sh`
- [ ] Boot test → banner
- [ ] EL0 smoke test
- [ ] Add AArch32 to CI matrix

## Success Criteria

- Both targets build cleanly with `-Z build-std`
- Both boot to "Hi from U-mode" via QEMU
- HAL trait parity with RV64 + AArch64
- CI matrix includes both as required (post-stabilization)
- Memory footprint < 8 MB on each

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| 32-bit address space too tight after HHDM + kernel + 3 services | Med | High | Skip HHDM identity for RAM > 256 MB; document; the 32-bit ports target ≤ 256 MB |
| `libs/types::VAddr` u64 assumptions leak in | High | High | Phase 01 + Phase 03 already preview the discipline; grep for `as u64` after this phase, replace with `as usize` or convert through VAddr |
| CP15 access varies subtly across ARMv7 cores | Med | Med | Target cortex-a15 in QEMU; document; broader support post-v1.0 |
| RV32 firmware (OpenSBI) availability quirky | Med | Med | Provide a `-bios none` direct kernel boot path; document |
| Some cells use `u64` in IPC types that don't fit 32-bit naturally | Med | Med | Audit: `CellId`, `CapId`, `Ticks` — keep as `u64` even on 32-bit (LL emul); benchmark impact |
| GIC access via CP15 vs MMIO differs from AArch64 — code drift | Cert | Med | Wrap GIC ops in a per-arch trait; share state types |

## Security Considerations

- Same defensive posture as 64-bit; narrower addresses mean less ASLR entropy (revisit post-v1.0)
- 32-bit kernels often used in embedded contexts → document expected threat model in `docs/security-model.md` (Phase 12)

## Rollback

Each arch is fully isolated under its target_arch cfg. Revert either drops back to stub; 64-bit builds unaffected. CI matrix tolerates failure if `continue-on-error` retained until stable.

## Next Steps

Phase 22 benchmarks all 5 archs side-by-side. Post-v1.0: 16-bit / DSP targets, MIPS, PowerPC if community demands. Patterns proven here scale to any RISC-style ISA.
