# Phase 02: AArch32 HAL

**Priority:** High  
**Status:** ☐ Todo  
**Estimate:** 3–4 giờ  
**Blocker cho:** Phase 03, Phase 04

## Context Links

- `hal/arch/arm/src/aarch64/boot.rs` — EL2→EL1 pattern (adapt: AArch32 modes)
- `hal/arch/arm/src/aarch64/context.rs` — CpuContext pattern (adapt: 32-bit ARM registers)
- `hal/arch/arm/src/aarch32.rs` — stub hiện tại (cần complete)
- `hal/arch/arm/src/aarch64/uart_pl011.rs` — PL011 UART (reuse: same MMIO base 0x09000000)
- `kernel/linker-aarch64.ld` — linker reference (adapt: 32-bit, same load base)

## Overview

Hoàn thiện AArch32 từ stub hiện tại. Điểm mấu chốt còn thiếu: `boot.rs` (không có), `context.rs` (không có), `uart_pl011.rs` (không có). `aarch32.rs` có `cpsie i`/`cpsid i` asm nhưng `switch_context` là `unimplemented!()`.

Target: `armv7a-none-eabi`. QEMU `-kernel` load kernel vào `0x40080000` trên `virt` machine, vào **SVC mode** (Supervisor = kernel mode trong ARM32).

**Quan trọng:** `cfg(target_arch = "arm")` là gate cho ARM 32-bit (AArch32/ARMv7).

## Architecture Facts

### ARM32 Privilege Modes (CPSR bits [4:0])
```
0x10: User
0x11: FIQ
0x12: IRQ  
0x13: SVC  ← entry mode từ QEMU -kernel
0x17: Abort
0x1B: Undefined
0x1F: System
```

### QEMU ARM virt `-kernel` Entry State
- CPU: `cortex-a15` (ARMv7-A, có VFPv4 và NEON)
- Mode: SVC (CPSR bits [4:0] = 0x13)
- Interrupts: **disabled** (CPSR.I = 1, CPSR.F = 1)
- MMU: **OFF** (SCTLR.M = 0) — bare physical
- Load address: `0x40080000` (giống AArch64 virt)
- Entry: `_start` symbol (địa chỉ thấp nhất trong ELF)
- r0 = 0, r1 = machine type (0xFFFFFFFF cho DTB), r2 = DTB physical address

### ARM32 Register Layout (ABI: AAPCS)
```
Callee-saved: r4, r5, r6, r7, r8, r9, r10 (sl), r11 (fp)
Caller-saved: r0-r3, r12 (ip)
Special:      r13 (sp), r14 (lr), r15 (pc)
```

### CpuContext32 cho cooperative switch
Chỉ save callee-saved + sp + lr:
```rust
pub struct Arm32Context {
    r4: u32, r5: u32, r6: u32, r7: u32,
    r8: u32, r9: u32, r10: u32, r11: u32,
    sp: u32,
    lr: u32,    // resume address (= "return address")
    cpsr: u32,  // flags (cần cho interrupt enable state)
}
```
Offset layout: r4=0, r5=4, ..., r11=28, sp=32, lr=36, cpsr=40.

### PL011 UART (ARM virt machine)
Base address: `0x09000000` (giống AArch64 virt machine).
Registers (32-bit MMIO):
```
+0x000: UARTDR   (Data Register — read: RX, write: TX)
+0x018: UARTFR   (Flag Register — bit 5: TXFF=TX full, bit 7: TXFE=TX empty)
+0x024: UARTIBRD (Integer Baud Rate)
+0x028: UARTFBRD (Fractional Baud Rate)
+0x02C: UARTLCR_H (Line Control)
+0x030: UARTCR   (Control Register)
```

Khác AArch64: MMIO access dùng `u32` volatile reads/writes — ARM32 không có `LDR X0, [X1]` (64-bit register), chỉ có `LDR R0, [R1]`.

### VFP Enable (nếu cần cho context switch)
Để dùng VFP/NEON instructions (nếu compiler tạo FP code):
```asm
MRC p15, 0, r0, c1, c0, 2  // read CPACR
ORR r0, r0, #(0xF << 20)   // enable CP10 + CP11 (full access)
MCR p15, 0, r0, c1, c0, 2  // write CPACR
MOV r0, #0x40000000         // FPEXC.EN bit
FMXR FPEXC, r0              // enable VFP
```
Nano profile: chỉ cần enable nếu `armv7a-none-eabi` compiler dùng soft-float hay hard-float.

**Kiểm tra:** `armv7a-none-eabi` mặc định là soft-float (không emit FP instructions), nên không cần enable VFP cho nano profile.

## Requirements

### Functional
- [ ] `_start` vào từ SVC mode, setup SP, clear BSS, call `kmain(0, dtb_r2)`
- [ ] PL011 UART init và `putchar` qua MMIO `u32` writes
- [ ] `Arm32Context` struct với `switch` function (inline asm, ARM32 syntax)
- [ ] `switch_context` trong `AArch32Arch` hoàn chỉnh (không còn `unimplemented!()`)
- [ ] `#[cfg(target_arch = "arm")]` gate đúng chỗ

### Non-functional
- [ ] Không dùng `mod.rs`
- [ ] Unsafe có SAFETY comment
- [ ] `cargo check --target armv7a-none-eabi -p hal-arm` passes

## Architecture (file structure)

```
hal/arch/arm/src/
├── aarch32.rs          (complete Arch impl, re-export submodules)
└── aarch32/
    ├── boot.rs         (_start asm, entry sequence)
    ├── context.rs      (Arm32Context, switch fn)
    └── uart_pl011.rs   (32-bit MMIO PL011)
```

`aarch32/` directory đã tồn tại (empty). Chỉ cần tạo 3 files bên trong.

## Implementation Steps

### Step 1 — `hal/arch/arm/src/aarch32/boot.rs`

```rust
use core::arch::global_asm;

global_asm!(
    ".section .text.boot, \"ax\"",
    ".global _start",
    "_start:",
    // Disable IRQ và FIQ (CPSR.I=1, CPSR.F=1)
    "cpsid if",
    // Save DTB pointer (r2 từ QEMU) — r2 bị clear khi BSS clear
    "mov r8, r2",
    // Setup stack
    "ldr sp, ={stack_top}",
    // Clear BSS
    "ldr r4, =__bss_start",
    "ldr r5, =__bss_end",
    "1: cmp r4, r5",
    "bge 2f",
    "str r6, [r4], #4",     // r6 = 0 (không cần init, SVC mode entry)
    "b 1b",                  // Thực ra r6 chưa clear — dùng "mov r6, #0" trước
    "2:",
    // Call kmain(hartid=0, dtb=r8)
    "mov r0, #0",            // hartid = 0
    "mov r1, r8",            // dtb ptr (saved r2)
    "bl {entry}",
    "3: wfi",
    "b 3b",
    stack_top = sym __stack_top,
    entry = sym kmain_arm32,
);

// Thực tế inline asm phức tạp hơn — xem implementation steps chi tiết

#[no_mangle]
pub extern "C" fn kmain_arm32(hartid: usize, dtb: usize) -> ! {
    extern "C" { fn kmain(hartid: usize, dtb: usize) -> !; }
    unsafe { kmain(hartid, dtb) }
}

extern "C" {
    pub static __stack_top: u8;
    pub static mut __bss_start: u8;
    pub static __bss_end: u8;
}
```

**Lưu ý quan trọng về BSS clear:**
- Trong ARM32, `global_asm!` không hỗ trợ `sym` cho static mut (linker symbols)
- Dùng `.extern __bss_start` trong asm, rồi `ldr r4, =__bss_start` (PC-relative literal pool)
- `mov r6, #0` trước vòng lặp BSS clear

**Lưu ý về `ldr sp, =symbol`:**
ARM32 assembler tạo literal pool cho `ldr rd, =value`. Trong `global_asm!`, dùng `{stack_top}` như một immediate value thì không hoạt động — phải dùng `.word` ở cuối literal pool hoặc dùng cú pháp `adr`/`ldr` đặc biệt. Pattern được recommend:
```asm
ldr sp, .Lstack_top_ptr
...
.Lstack_top_ptr: .word __stack_top
```

### Step 2 — `hal/arch/arm/src/aarch32/context.rs`

```rust
#[repr(C)]
pub struct Arm32Context {
    pub r4:   u32,  // offset 0
    pub r5:   u32,  // offset 4
    pub r6:   u32,  // offset 8
    pub r7:   u32,  // offset 12
    pub r8:   u32,  // offset 16
    pub r9:   u32,  // offset 20
    pub r10:  u32,  // offset 24
    pub r11:  u32,  // offset 28
    pub sp:   u32,  // offset 32 — kernel stack pointer
    pub lr:   u32,  // offset 36 — resume address (= saved LR = return PC)
    pub cpsr: u32,  // offset 40
}

pub unsafe fn switch(old: *mut Arm32Context, new: *const Arm32Context) {
    core::arch::asm!(
        // Save old context (r4-r11 + sp + lr + cpsr)
        "stmia {old}, {{r4-r11}}",   // save r4..r11 at old[0..7]
        "mrs {tmp}, cpsr",
        "str sp, [{{old}}, #32]",
        "str lr, [{{old}}, #36]",
        "str {tmp}, [{{old}}, #40]",
        // Restore new context
        "ldmia {new}, {{r4-r11}}",   // restore r4..r11
        "ldr sp, [{{new}}, #32]",
        "ldr lr, [{{new}}, #36]",
        "ldr {tmp}, [{{new}}, #40]",
        "msr cpsr_cxsf, {tmp}",
        "bx lr",                      // return to new task's resume address
        old = in(reg) old,
        new = in(reg) new,
        tmp = out(reg) _,
    )
}
```

**Lưu ý:** ARM `stmia` / `ldmia` với register list syntax trong Rust inline asm có thể cần `global_asm!` thay vì `asm!` vì brace escaping. Kiểm tra khi implement.

### Step 3 — `hal/arch/arm/src/aarch32/uart_pl011.rs`

```rust
const PL011_BASE: u32 = 0x09000000;  // ARM virt machine
const UARTDR:   u32 = PL011_BASE + 0x000;
const UARTFR:   u32 = PL011_BASE + 0x018;
const UARTIBRD: u32 = PL011_BASE + 0x024;
const UARTFBRD: u32 = PL011_BASE + 0x028;
const UARTLCR:  u32 = PL011_BASE + 0x02C;
const UARTCR:   u32 = PL011_BASE + 0x030;

unsafe fn mmio_write(addr: u32, val: u32) {
    core::ptr::write_volatile(addr as *mut u32, val);
}
unsafe fn mmio_read(addr: u32) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}

pub fn init() {
    unsafe {
        mmio_write(UARTCR, 0);          // disable UART
        mmio_write(UARTIBRD, 26);       // 115200 baud @ 48MHz refclk
        mmio_write(UARTFBRD, 3);
        mmio_write(UARTLCR, 0x70);      // 8N1, FIFO enable
        mmio_write(UARTCR, 0x301);      // TX+RX enable, UART enable
    }
}

pub fn putchar(c: u8) {
    unsafe {
        while mmio_read(UARTFR) & (1 << 5) != 0 {}  // wait TXFF clear
        mmio_write(UARTDR, c as u32);
    }
}
```

### Step 4 — Complete `hal/arch/arm/src/aarch32.rs`

Thay `unimplemented!()` context switch bằng real implementation:

```rust
#[cfg(target_arch = "arm")] pub mod boot;
#[cfg(target_arch = "arm")] pub mod context;
pub mod uart_pl011;

pub struct AArch32Arch;

#[cfg(target_arch = "arm")]
impl Arch for AArch32Arch {
    type Context = context::Arm32Context;
    fn init(&self) { /* ARM virt có GIC nhưng nano profile skip */ }
    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
        context::switch(old, new);
    }
    fn enable_interrupts(&self) { unsafe { core::arch::asm!("cpsie i", options(nomem, nostack)); } }
    fn disable_interrupts(&self) { unsafe { core::arch::asm!("cpsid i", options(nomem, nostack)); } }
    fn interrupts_enabled(&self) -> bool {
        let cpsr: u32;
        unsafe { core::arch::asm!("mrs {}, cpsr", out(reg) cpsr); }
        cpsr & (1 << 7) == 0  // CPSR.I=0 → interrupts enabled
    }
    fn wait_for_interrupt(&self) { unsafe { core::arch::asm!("wfi"); } }
}
```

## Todo List

- [ ] Tạo `hal/arch/arm/src/aarch32/boot.rs`
- [ ] Tạo `hal/arch/arm/src/aarch32/context.rs`  
- [ ] Tạo `hal/arch/arm/src/aarch32/uart_pl011.rs`
- [ ] Update `hal/arch/arm/src/aarch32.rs` — add submodule exports, complete Arch impl
- [ ] Verify `cargo check --target armv7a-none-eabi -p hal-arm` passes

## Success Criteria

- `cargo check --target armv7a-none-eabi -p hal-arm` passes không lỗi
- Không còn `unimplemented!()` trong `aarch32.rs`
- 3 files trong `aarch32/` tồn tại và compile đúng
- `switch_context` có real implementation

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `armv7a-none-eabi` chưa install | `rustup target add armv7a-none-eabi` |
| ARM32 `global_asm!` literal pool syntax phức tạp | Dùng `.Lptr: .word symbol` pattern thay vì `sym` operand |
| `stmia {reg}, {r4-r11}` brace escaping trong Rust | Dùng `{{r4-r11}}` hoặc `global_asm!` |
| CPSR manipulation cần MRS/MSR | Đây là ARMv7-A SVC mode — MRS/MSR available |
| VFP not enabled → compiler FP crash | `armv7a-none-eabi` mặc định soft-float → không cần |

## Security Considerations

- SVC mode = kernel privilege — không cần thêm privilege escalation
- BSS clear trước khi dùng bất kỳ global variables
