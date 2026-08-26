# Plan: x86_32 và AArch32 HAL — Nano Profile

**Trạng thái:** In Progress  
**Tạo:** 2026-06-07  
**Mục tiêu:** Đưa x86_32 và AArch32 lên "Scheduler initialized" (cùng chuẩn nano như RV32 Phase 31). Không cần cells, disk, hay paging hardware — bare physical cho cả hai.

## Phạm vi (Nano Profile)

Giống RV32 Phase 31: boot → UART → BootInfo fallback → FrameAllocator → bare physical → Heap → Scheduler. Không có Limine, không có cells, không có disk.

## Dependency graph

```
Phase 01 (x86_32 HAL) ──┐
                         ├──► Phase 03 (kernel integration) ──► Phase 04 (tests)
Phase 02 (AArch32 HAL) ──┘
```

Phase 01 và 02 độc lập — có thể chạy song song.

## Phases

| # | Phase | Trạng thái | Priority |
|---|-------|-----------|---------|
| 01 | [x86_32 HAL — boot, GDT, IDT, UART, context](phase-01-x86-32-hal.md) | ☐ Todo | High |
| 02 | [AArch32 HAL — boot.rs, context, PL011, complete stubs](phase-02-aarch32-hal.md) | ☐ Todo | High |
| 03 | [Kernel integration — linker scripts, build.rs, boot.rs, main.rs](phase-03-kernel-integration.md) | ☐ Todo | High |
| 04 | [Handoff tests — QemuRunner + Phase 05/06 test cases](phase-04-handoff-tests.md) | ☐ Todo | Medium |

## Target triples

| Arch | Triple | `target_arch` cfg | Rust tier |
|------|--------|------------------|----------|
| x86_32 | `i686-unknown-none` (custom JSON nếu cần) | `"x86"` | Tier 3 / custom |
| AArch32 | `armv7a-none-eabi` | `"arm"` | Tier 3 |

## Boot flow mục tiêu

```
x86_32:  QEMU -kernel (multiboot1) → _start (32-bit PM) → GDT → IDT → kmain → UART → BootInfo → Frame alloc → bare physical → Heap → Scheduler
AArch32: QEMU -kernel (Linux ARM boot) → _start (SVC mode) → SP → BSS → kmain → PL011 → BootInfo → Frame alloc → bare physical → Heap → Scheduler
```

## Files sẽ tạo mới

| File | Phase |
|------|-------|
| `hal/arch/x86/src/x86_32.rs` | 01 |
| `hal/arch/x86/src/x86_32/boot.rs` | 01 |
| `hal/arch/x86/src/x86_32/gdt.rs` | 01 |
| `hal/arch/x86/src/x86_32/idt.rs` | 01 |
| `hal/arch/x86/src/x86_32/uart.rs` | 01 |
| `hal/arch/x86/src/x86_32/context.rs` | 01 |
| `hal/arch/arm/src/aarch32/boot.rs` | 02 |
| `hal/arch/arm/src/aarch32/context.rs` | 02 |
| `hal/arch/arm/src/aarch32/uart_pl011.rs` | 02 |
| `kernel/linker-x86-32.ld` | 03 |
| `kernel/linker-aarch32.ld` | 03 |

## Files sẽ chỉnh sửa

| File | Phase | Thay đổi |
|------|-------|---------|
| `hal/arch/x86/src/lib.rs` | 01 | Export `x86_32` + cfg gate |
| `hal/arch/arm/src/aarch32.rs` | 02 | Complete `switch_context`, `Arch` impl |
| `kernel/build.rs` | 03 | Add `"x86"` và `"arm"` linker cases |
| `kernel/src/boot.rs` | 03 | FALLBACK_BOOT_INFO cho x86 và arm |
| `kernel/src/main.rs` | 03 | UART, GDT/IDT, paging, interrupt, panic gates |
| `tests/integration/src/lib.rs` | 04 | boot_x86_32, boot_aarch32, qemu binaries |
| `tests/integration/tests/handoff.rs` | 04 | Phase 05 (x86_32) + Phase 06 (AArch32) tests |
