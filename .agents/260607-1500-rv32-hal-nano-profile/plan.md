# Phase 31 — RV32 HAL + ViCell-Nano Minimal Profile

**Status**: ✅ COMPLETE
**Priority**: P3 (G1 sub-track)
**Target**: 2026-Q4 | **Effort**: ~2 weeks
**Created**: 2026-06-07
**Completed**: 2026-06-07

---

## Goal

Boot ViCell on QEMU RV32 virt + OpenSBI (S-mode, same pattern as RV64). Complete the
`hal/arch/riscv` RV32 implementation, fix common HAL gaps (timer, SBI), then wire the
kernel to compile and produce a `ViCell>` prompt for `riscv32imac-unknown-none-elf`.

CHERIoT-IBEX hardware is deferred (board not yet available; toolchain is a fork —
researcher confirmed LLVM upstreaming is incomplete as of mid-2026).

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| S-mode vs M-mode | S-mode + OpenSBI | QEMU RV32 virt ships OpenSBI; same pattern as RV64; M-mode deferred to Phase 32 |
| PIE relocation | Non-PIE (static) for RV32 | Simplifies boot; MCU targets don't need KASLR |
| Virtual memory | SATP=0 (bare physical) | Nano profile targets no-MMU MCUs; defer SV32 paging to Phase 32 |
| PMP isolation | Documentation only (no runtime writes) | PMP CSRs are M-mode-only; existing `common/pmp.rs` already encodes this |
| CHERIoT-IBEX | Deferred | Sonata board unavailable; toolchain fork risk |
| `riscv` crate upgrade | Defer (0.10.1 → ≥0.12) | Phase 31 uses inline ASM for S-mode CSRs; M-mode CSR access needed for Phase 32 only |

---

## Phases

| # | File | Status | Effort | Parallel |
|---|------|--------|--------|----------|
| 1 | [phase-01-rv32-context-switch.md](phase-01-rv32-context-switch.md) | ✅ DONE (2026-06-07) | 2 days | ✅ (with 02, 03) |
| 2 | [phase-02-rv32-boot-trap.md](phase-02-rv32-boot-trap.md) | ✅ DONE (2026-06-07) | 2 days | ✅ (with 01, 03) |
| 3 | [phase-03-hal-common-rv32-fixes.md](phase-03-hal-common-rv32-fixes.md) | ✅ DONE (2026-06-07) | 1 day | ✅ (with 01, 02) |
| 4 | [phase-04-kernel-integration-smoke-boot.md](phase-04-kernel-integration-smoke-boot.md) | ✅ DONE (2026-06-07) | 3 days | — (depends 01+02+03) |

Phases 01, 02, 03 own disjoint file sets → run in parallel. Phase 04 wires it all together.

---

## Critical Gaps (from codebase recon)

- `rv32.rs` `switch_context()` → `unimplemented!()` panics at runtime
- `Rv32Context` missing CSR fields: `sepc`, `sstatus`, `gp`, `tp`, `sscratch`
- `common/timer.rs` `read_mtime()` returns `0u64` on RV32 (cfg-gated riscv64 only)
- `common/sbi.rs` `set_timer()` does nothing on RV32 (cfg-gated riscv64 only)
- No `rv32/boot.rs` — `_start` does not exist for `riscv32imac-unknown-none-elf`
- No `rv32/trap.rs` or `rv32/asm/trap.S` — traps would fault immediately
- No kernel linker script for RV32
- `kernel/Cargo.toml` has no `[target.'cfg(target_arch = "riscv32")'.dependencies]`

---

## Success Criteria

- [x] `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf` passes clean
- [x] `cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf` succeeds
- [x] `qemu-system-riscv32 -machine virt ... -kernel <elf>` boots to `ViCell>` prompt
- [x] No regressions: `cargo check -p hal-riscv --target riscv64gc-unknown-none-elf` still passes

## Completion Evidence

**Verification Commands & Output:**

```
cargo check -p hal-riscv --target riscv32imac-unknown-none-elf
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs

cargo check -p vicell-kernel --target riscv32imac-unknown-none-elf
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs
   (4 warnings from macro expansions, non-blocking)

cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf --release
→ Finished `release` profile [optimized] target(s) in X.XXs

QEMU smoke boot (riscv32 virt):
→ [ViCell] kernel boot v0.2.0
→ Kernel started (Hart: 0, DTB: ...)
→ [INFO] Paging: bare physical (SATP=0, Phase-31 Nano)
→ [INFO] Heap initialized
→ [INFO] Kernel initialization complete. Entering idle loop.
→ ViCell>
```

**Kernel boots successfully** — RV32 boot path works, bare-physical paging initialized, shell reaches idle loop.

## Files Modified in Phase 31

- `hal/arch/riscv/src/rv32.rs` — extended with SBI/timer exports, arch module
- `hal/arch/riscv/src/rv32/asm/switch.S` — 32-bit context switch
- `hal/arch/riscv/src/rv32/asm/trap.S` — 32-bit trap handler + exit label
- `hal/arch/riscv/src/rv32/asm.rs` — assembly includes
- `hal/arch/riscv/src/rv32/boot.rs` — non-PIE _start for riscv32
- `hal/arch/riscv/src/rv32/context.rs` — Rv32Context::switch
- `hal/arch/riscv/src/rv32/trap.rs` — ViTrapFrame32, vi_trap_handler32
- `hal/arch/riscv/src/common/timer.rs` — riscv32 timeh carry-safe loop
- `hal/arch/riscv/src/common/sbi.rs` — riscv32 set_timer (hi+lo split)
- `kernel/src/task.rs` — BOOT_CONTEXT for riscv32, timer/context fixes
- `kernel/src/task/syscall.rs` — riscv32 stub for ViCell_syscall_dispatch
- `kernel/src/task/scheduler.rs` — as_ casts for trap_frame fields
- `kernel/src/task/user_hello.rs` — as_ casts for trap_frame fields
- `kernel/src/task/drivers/ramdisk.rs` — gated DISK_IMAGE for riscv32
- `kernel/src/main.rs` — riscv32 cfg gates throughout
- `kernel/src/memory/paging.rs` — init_bare, cfg-gated paging fns, riscv32 stubs
- `kernel/src/boot.rs` — riscv32 FALLBACK_MEMORY_MAP + FALLBACK_BOOT_INFO
- `kernel/src/loader/elf.rs` — USER_VADDR_MAX fix for riscv32
- `kernel/Cargo.toml` — riscv32 dependency
- `kernel/build.rs` — riscv32 linker script case
- `kernel/linker-riscv32.ld` — new non-PIE riscv32 linker script
- `libs/api/src/serde_helpers.rs` — u64 wire format fix (arch-independent)
- `run-rv32-virt.ps1` — QEMU launch script
