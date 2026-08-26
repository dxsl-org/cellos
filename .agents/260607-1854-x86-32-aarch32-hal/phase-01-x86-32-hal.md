# Phase 01: x86_32 HAL

**Priority:** High  
**Status:** ☐ Todo  
**Estimate:** 3–4 giờ  
**Blocker cho:** Phase 03, Phase 04

## Context Links

- `hal/arch/x86/src/x86_64/boot.rs` — _start reference (adapt: long mode → 32-bit PM)
- `hal/arch/x86/src/x86_64/gdt.rs` — GDT pattern (simplify: no TSS, 32-bit selectors)
- `hal/arch/x86/src/x86_64/idt.rs` — IDT pattern (adapt: 32-bit gates)
- `hal/arch/x86/src/x86_64/context.rs` — CpuContext pattern (adapt: 32-bit regs)
- `hal/arch/riscv/src/rv32/boot.rs` — nano profile reference (SATP=0, bare physical)
- `hal/arch/x86/src/lib.rs` — nơi thêm x86_32 export

## Overview

x86_32 HAL từ đầu. Target triple: `i686-unknown-none` (hoặc custom JSON). QEMU dùng `-kernel` với multiboot1 header — đây là cách đơn giản nhất, không cần Limine hay ISO. QEMU load kernel ở địa chỉ `1M` (0x00100000), vào protected mode 32-bit trước khi gọi `_start`.

**Quan trọng:** `cfg(target_arch = "x86")` là gate cho 32-bit x86 trong Rust (NOT `"x86_32"`).

## Architecture Facts

### QEMU Multiboot1 Entry State
QEMU `-kernel` dùng multiboot1 protocol:
- EAX = `0x2BADB002` (multiboot magic)
- EBX = physical address của multiboot info struct
- CS = flat 32-bit code (base=0, limit=4GB)
- GDT = QEMU's temporary GDT (không trust, phải load lại)
- Interrupts: **disabled** (EFLAGS.IF = 0)
- PE bit set (protected mode)
- Paging: **OFF** (CR0.PG = 0) — bare physical từ đầu

### 32-bit GDT Layout
```
0x00: NULL
0x08: Kernel Code  (base=0, limit=4GB, DPL=0, executable, 32-bit)
0x10: Kernel Data  (base=0, limit=4GB, DPL=0, writable)
```
Selectors: CS=0x08, DS/ES/SS=0x10.

### 32-bit IDT
IDT entries 32-bit khác x86_64: `off_lo | sel | zero | attr | off_hi` (8 bytes/entry).
`attr` = `0x8E` (present, DPL=0, interrupt gate 32-bit).

Nano profile: chỉ cần stub entries cho tất cả 256 vectors. Không cần handler thật — khi có exception kernel sẽ reboot.

### COM1 UART (Port I/O)
COM1 = port `0x3F8`. Dùng `in`/`out` instructions (giống x86_64 uart_16550).
Init sequence:
```
0x3F8+1 = 0x00  // disable DLAB interrupts
0x3F8+3 = 0x80  // set DLAB
0x3F8+0 = 0x03  // divisor low (38400 baud)
0x3F8+1 = 0x00  // divisor high
0x3F8+3 = 0x03  // 8N1, clear DLAB
0x3F8+2 = 0xC7  // FIFO control
0x3F8+4 = 0x0B  // modem control
```

### CpuContext32 — Callee-saved registers (32-bit System V ABI)
Callee-saved: EBX, ESI, EDI, EBP. Plus ESP và EIP (saved/restored vào stack).
```rust
pub struct CpuContext32 {
    ebx: u32, esi: u32, edi: u32, ebp: u32,
    esp: u32,
    eip: u32,  // return address
}
```
Context switch: `push ebx/esi/edi/ebp; mov [old+16], esp; mov esp, [new+16]; pop ebp/edi/esi/ebx; jmp [new+20]`

### Target Triple
Thử `i686-unknown-none` (Rust tier 3). Nếu không có sẵn, tạo custom target JSON tại `targets/x86-unknown-none.json` (copy từ `x86_64-unknown-none.json` rồi sửa: `"arch": "x86"`, `"target-pointer-width": "32"`, `"data-layout"`, `"llvm-target": "i686-unknown-none"`, `"cpu": "i686"`).

## Requirements

### Functional
- [ ] `_start` nhận multiboot1 entry state (32-bit PM), setup stack, call `kmain`
- [ ] Multiboot1 header embedded trong `.multiboot` section (first in binary)
- [ ] GDT load: null + kCode + kData descriptors, `ljmp` để reload CS
- [ ] IDT load: 256 stub entries, `lidt`
- [ ] COM1 UART init và `putchar` via port I/O
- [ ] `CpuContext32` với context switch assembly
- [ ] `Arch` impl cho `X86_32Arch` (impl `ViArch` trait)

### Non-functional
- [ ] `#![forbid(unsafe_code)]` KHÔNG áp dụng cho HAL — unsafe có SAFETY comment
- [ ] Không dùng `mod.rs` — dùng `x86_32.rs` parallel với `x86_32/` directory
- [ ] `cargo check --target i686-unknown-none` (hoặc custom) passes

## Architecture

```
hal/arch/x86/src/
├── lib.rs              (thêm x86_32 export)
├── x86_32.rs           (Arch impl, re-export submodules)
└── x86_32/
    ├── boot.rs         (_start, multiboot header, stack)
    ├── gdt.rs          (32-bit GDT)
    ├── idt.rs          (32-bit IDT stubs)
    ├── uart.rs         (COM1 port I/O)
    └── context.rs      (CpuContext32)
```

## Implementation Steps

### Step 1 — `hal/arch/x86/src/x86_32/boot.rs`

```rust
// SAFETY: multiboot header phải là byte sequence chính xác
// SAFETY: _start cần raw asm để setup stack trước khi Rust code chạy

use core::arch::global_asm;

global_asm!(
    // Multiboot1 header — PHẢI là 4 bytes đầu tiên của .multiboot section
    ".section .multiboot, \"a\"",
    ".align 4",
    ".long 0x1BADB002",           // magic
    ".long 0x00000002",           // flags: bit1 = memory map
    ".long -(0x1BADB002 + 0x00000002)",  // checksum

    // Boot entry point
    ".section .text.boot, \"ax\"",
    ".global _start",
    "_start:",
    "cli",
    // EAX = multiboot magic (0x2BADB002), EBX = multiboot_info ptr
    // Setup kernel stack
    "mov esp, {stack_top}",
    "and esp, -16",     // 16-byte align
    "xor ebp, ebp",
    // Call kmain(0, 0) — x86_32 dùng FALLBACK_BOOT_INFO, không parse multiboot
    "push 0",           // dtb = 0
    "push 0",           // hartid = 0
    "call {entry}",
    "1: hlt",
    "jmp 1b",
    stack_top = sym __stack_top,
    entry = sym kmain_x86_32,
);

#[no_mangle]
pub extern "C" fn kmain_x86_32() -> ! {
    extern "C" { fn kmain(hartid: usize, dtb: usize) -> !; }
    unsafe { kmain(0, 0) }
}

extern "C" { pub static __stack_top: u8; }
```

**Lưu ý:** `i686-unknown-none` dùng `extern "C"` là 32-bit cdecl (args trên stack, không phải registers). `_start` push args lên stack trước `call`.

### Step 2 — `hal/arch/x86/src/x86_32/gdt.rs`

```rust
// 32-bit GDT: NULL + kCode (0x08) + kData (0x10)
// Segment descriptor format 32-bit:
//   [15:0]  limit_lo, [31:16] base_lo
//   [7:0]   base_mid, [11:8] type, [12] S, [14:13] DPL, [15] P
//   [19:16] limit_hi, [20] AVL, [21] 0, [22] DB (1=32-bit), [23] G, [31:24] base_hi

static mut GDT: [u64; 3] = [
    0,                    // null
    0x00CF9A000000FFFF,   // kernel code 32-bit (base=0, limit=4G, DPL=0, exec)
    0x00CF92000000FFFF,   // kernel data 32-bit (base=0, limit=4G, DPL=0, data)
];

#[repr(C, packed)]
struct GdtPtr { limit: u16, base: u32 }

pub fn init() {
    // ...load GDT, far jump to reload CS (0x08), reload data segments (0x10)
}
```

`G` bit (granularity) = 1 → limit unit = 4KB → `0xFFFFF * 4KB = 4GB`.

### Step 3 — `hal/arch/x86/src/x86_32/idt.rs`

32-bit interrupt gate = 8 bytes:
```
[15:0]  offset_lo
[31:16] segment selector (0x08)
[39:32] zero
[47:40] type|attr: 0x8E = present|DPL=0|32-bit interrupt gate
[63:48] offset_hi
```

256 stubs: `hlt; jmp .-1` hoặc đơn giản là `iret`.

### Step 4 — `hal/arch/x86/src/x86_32/uart.rs`

Port I/O với `in al, dx` / `out dx, al`.
```rust
pub fn init() { /* COM1 init sequence */ }
pub fn putchar(c: u8) { /* wait LSR.THRE, out dx, al */ }
```

### Step 5 — `hal/arch/x86/src/x86_32/context.rs`

`CpuContext32` struct + `switch` function với inline asm.

### Step 6 — `hal/arch/x86/src/x86_32.rs`

```rust
#[cfg(target_arch = "x86")] pub mod boot;
#[cfg(target_arch = "x86")] pub mod gdt;
#[cfg(target_arch = "x86")] pub mod idt;
pub mod uart;
pub mod context;

pub struct X86_32Arch;

#[cfg(target_arch = "x86")]
impl Arch for X86_32Arch {
    type Context = context::CpuContext32;
    fn init(&self) { gdt::init(); idt::init(); }
    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
        context::switch(old, new);
    }
    fn enable_interrupts(&self) { unsafe { core::arch::asm!("sti"); } }
    fn disable_interrupts(&self) { unsafe { core::arch::asm!("cli"); } }
    fn interrupts_enabled(&self) -> bool {
        let flags: u32;
        unsafe { core::arch::asm!("pushfd; pop {}", out(reg) flags); }
        flags & (1 << 9) != 0
    }
    fn wait_for_interrupt(&self) { unsafe { core::arch::asm!("hlt"); } }
}

#[cfg(not(target_arch = "x86"))]
impl Arch for X86_32Arch {
    type Context = usize;
    fn init(&self) {}
    unsafe fn switch_context(&self, _: *mut usize, _: *const usize) {}
    fn enable_interrupts(&self) {}
    fn disable_interrupts(&self) {}
    fn interrupts_enabled(&self) -> bool { false }
    fn wait_for_interrupt(&self) {}
}
```

### Step 7 — `hal/arch/x86/src/lib.rs`

Thêm:
```rust
pub mod x86_32;
#[cfg(target_arch = "x86")]
pub use x86_32::*;
```

## Todo List

- [ ] Tạo `hal/arch/x86/src/x86_32/boot.rs`
- [ ] Tạo `hal/arch/x86/src/x86_32/gdt.rs`
- [ ] Tạo `hal/arch/x86/src/x86_32/idt.rs`
- [ ] Tạo `hal/arch/x86/src/x86_32/uart.rs`
- [ ] Tạo `hal/arch/x86/src/x86_32/context.rs`
- [ ] Tạo `hal/arch/x86/src/x86_32.rs`
- [ ] Sửa `hal/arch/x86/src/lib.rs` — thêm x86_32 export
- [ ] Verify `cargo check --target i686-unknown-none` (hoặc custom JSON) passes

## Success Criteria

- `cargo check --target i686-unknown-none -p hal-x86` compiles không lỗi
- Tất cả unsafe blocks có `// SAFETY:` comment
- Không dùng `mod.rs`
- x86_32 module structure đúng: `x86_32.rs` + `x86_32/` directory

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `i686-unknown-none` không tồn tại trong rustup | Tạo custom target JSON (xem step 0) |
| Multiboot header alignment sai → QEMU không boot | `.align 4` + checksum = -(magic+flags) |
| 32-bit cdecl vs 64-bit calling convention | x86 args trên stack, không phải registers |
| `abi_x86_interrupt` feature không có cho 32-bit | Dùng naked functions hoặc global_asm cho IDT stubs |

## Security Considerations

- IDT stubs: DPL=0 (không cho user code trigger trực tiếp)
- Stack guard: chưa cần cho nano profile
