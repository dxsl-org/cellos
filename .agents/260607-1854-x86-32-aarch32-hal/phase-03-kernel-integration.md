# Phase 03: Kernel Integration

**Priority:** High  
**Status:** ☐ Todo  
**Blocked by:** Phase 01, Phase 02  
**Estimate:** 3–4 giờ

## Context Links

- `kernel/build.rs` — linker script selection (thêm `"x86"` và `"arm"`)
- `kernel/linker-x86-64.ld` — x86 linker reference
- `kernel/linker-riscv32.ld` — nano profile linker reference (bare physical)
- `kernel/linker-aarch64.ld` — AArch64 linker reference
- `kernel/src/boot.rs` — FALLBACK_BOOT_INFO definitions
- `kernel/src/main.rs` — tất cả `#[cfg(target_arch)]` gates

## Overview

Wiring kernel để compile và boot cho `target_arch = "x86"` và `target_arch = "arm"`. Cả hai đều dùng nano profile (bare physical, FALLBACK_BOOT_INFO, không cells, không disk).

**Mapping `target_arch` value → action trong main.rs:**

| Thay đổi cần thiết | x86_32 | AArch32 |
|-------------------|--------|---------|
| Linker script | `kernel/linker-x86-32.ld` | `kernel/linker-aarch32.ld` |
| UART init | `hal::uart_16550::init()` (port I/O) | `hal::uart_pl011::init()` (MMIO) |
| HAL init | GDT + IDT (`x86_32::gdt::init()`, `x86_32::idt::init()`) | `hal::ARCH.init()` (noop) |
| UART putchar | `uart_16550::putchar` | `uart_pl011::putchar` |
| Paging | `bare physical` (giống RV32) | `bare physical` |
| BootInfo | `FALLBACK_BOOT_INFO` | `FALLBACK_BOOT_INFO` |
| Interrupt enable | `sti` (via `hal::ARCH.enable_interrupts()`) | `cpsie i` (via `hal::ARCH.enable_interrupts()`) |
| Halt (wfi/hlt) | `hlt` | `wfi` |
| Embedded ELF | KHÔNG | KHÔNG |

## Requirements

### Functional
- [ ] `kernel/build.rs` route đúng linker script cho `"x86"` và `"arm"`
- [ ] `kernel/src/boot.rs`: `FALLBACK_BOOT_INFO` cho x86 (base=0x00100000, ram=128MB) và arm (base=0x40080000, ram=256MB)
- [ ] `kernel/src/main.rs`: UART init, putchar, paging path, interrupt enable gate cho cả hai arch
- [ ] Linker scripts tạo binary boot được trên QEMU `-kernel`
- [ ] `cargo check --target i686-unknown-none -p vicell-kernel` passes
- [ ] `cargo check --target armv7a-none-eabi -p vicell-kernel` passes

### Non-functional
- [ ] Không thêm code không cần thiết (YAGNI — nano only)
- [ ] Không break các arch hiện có (RV64, AArch64, RV32, x86_64)

## Architecture: Linker Scripts

### `kernel/linker-x86-32.ld`

```ld
OUTPUT_ARCH(i386)
ENTRY(_start)

SECTIONS {
    . = 0x00100000;  /* 1 MB — standard multiboot load address */

    .multiboot : {
        KEEP(*(.multiboot))  /* multiboot header PHẢI đứng đầu */
    }

    .text.boot : { KEEP(*(.text.boot)) }
    .text       : { *(.text .text.*) }
    .rodata     : ALIGN(4096) { *(.rodata .rodata.*) }
    .data       : ALIGN(4096) {
        *(.data .data.*)
        *(.got .got.plt)
    }
    .bss        : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }
    .stack      : ALIGN(4096) NOLOAD {
        . = . + 0x8000;   /* 32 KiB kernel stack */
        __stack_top = .;
    }

    /DISCARD/ : { *(.comment) *(.eh_frame) *(.note.*) }
}
```

**Lưu ý:** `0x00100000` = 1MB. Vùng 0x0–0xFFFFF bị BIOS/QEMU dùng. Multiboot load address mặc định.

### `kernel/linker-aarch32.ld`

```ld
OUTPUT_ARCH(arm)
ENTRY(_start)

MEMORY {
    RAM (rwx) : ORIGIN = 0x40080000, LENGTH = 256M
}

SECTIONS {
    . = 0x40080000;  /* ARM virt load address, giống AArch64 */

    .text.boot  : { KEEP(*(.text.boot)) }
    .text        : ALIGN(4096) { *(.text .text.*) }
    .rodata      : ALIGN(4096) { *(.rodata .rodata.*) }
    .data        : ALIGN(4096) {
        *(.data .data.*)
        *(.got .got.plt)
    }
    .bss         : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }
    .stack       : ALIGN(4096) NOLOAD {
        . = . + 0x10000;   /* 64 KiB kernel stack */
        __stack_top = .;
    }

    /DISCARD/ : { *(.comment) *(.eh_frame) *(.note.*) }
}
```

## Architecture: `kernel/build.rs` Changes

Thêm 2 cases vào match arm:

```rust
let (ld_script, rerun_path) = match target_arch.as_str() {
    "aarch64" => ("kernel/linker-aarch64.ld", "kernel/linker-aarch64.ld"),
    "x86_64"  => ("kernel/linker-x86-64.ld",  "kernel/linker-x86-64.ld"),
    "x86"     => ("kernel/linker-x86-32.ld",   "kernel/linker-x86-32.ld"),  // NEW
    "arm"     => ("kernel/linker-aarch32.ld",  "kernel/linker-aarch32.ld"), // NEW
    "riscv32" => ("kernel/linker-riscv32.ld",  "kernel/linker-riscv32.ld"),
    _         => ("kernel/linker.ld",          "kernel/linker.ld"),
};
```

## Architecture: `kernel/src/boot.rs` Changes

Thêm FALLBACK_BOOT_INFO cho 2 arch mới. Pattern giống RV32:

```rust
// Hiện tại có:
// #[cfg(any(target_arch = "riscv32", ...))] static FALLBACK_BOOT_INFO: ...

// Thêm cho x86_32:
#[cfg(target_arch = "x86")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    phys_base: 0x0000_0000_0010_0000,  // 1MB load address
    memory_size: 128 * 1024 * 1024,    // 128MB (QEMU -m 128M)
    hhdm_offset: 0,                    // bare physical — không có HHDM
    memory_map: [MemoryEntry { base: 0, len: 0, typ: MemoryType::Reserved }; 64],
    memory_map_len: 1,
};

// Thêm cho AArch32:
#[cfg(target_arch = "arm")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    phys_base: 0x0000_0000_4008_0000,  // ARM virt load address
    memory_size: 256 * 1024 * 1024,   // 256MB (QEMU -m 256M)
    hhdm_offset: 0,                   // bare physical
    memory_map: [MemoryEntry { base: 0, len: 0, typ: MemoryType::Reserved }; 64],
    memory_map_len: 1,
};
```

**Kiểm tra:** Xem signature của `SimpleBootInfo` trong `kernel/src/boot.rs` để biết exact field names. Có thể cần adjust nếu struct khác với assumption.

## Architecture: `kernel/src/main.rs` Changes

### 1. UART init gate (~line 57-60)

Hiện tại:
```rust
#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
task::drivers::uart::init();
#[cfg(target_arch = "aarch64")]
crate::hal::uart_pl011::init();
#[cfg(target_arch = "x86_64")]
crate::hal::uart_16550::init();
```

Thêm:
```rust
#[cfg(target_arch = "arm")]
crate::hal::uart_pl011::init();     // cùng PL011, cùng base 0x09000000
#[cfg(target_arch = "x86")]
crate::hal::uart_16550::init();     // COM1 port I/O, cùng 16550A
```

### 2. HHDM/phys_offset setup (~line 65-70)

Gate hiện tại là `#[cfg(target_arch = "x86_64")]` — **không cần thêm** cho x86_32 (bare physical, không có HHDM).

### 3. HAL/GDT/IDT init (~line 75-83)

Hiện tại:
```rust
#[cfg(target_arch = "x86_64")]
{ /* GDT, IDT, SYSCALL deferred init */ }
#[cfg(not(target_arch = "x86_64"))]
hal::ARCH.init();
```

x86_32: muốn GDT/IDT init qua `hal::ARCH.init()` (vì `X86_32Arch::init()` calls gdt::init() + idt::init()). Vậy `not(x86_64)` sẽ tự include `x86` — **không cần thay đổi gate này**.

Nhưng phải verify: `X86_32Arch::init()` được called, không phải `X86_64Arch::init()`. Điều này phụ thuộc vào `hal::ARCH` static trong x86 HAL. Xem `hal/arch/x86/src/lib.rs` — cần export đúng `ARCH` static cho `x86` target.

### 4. UART putchar dispatch (~line 88-94)

Thêm arms cho `arm` và `x86`:
```rust
#[cfg(target_arch = "arm")]
fn __putchar(c: u8) { crate::hal::uart_pl011::putchar(c); }
#[cfg(target_arch = "x86")]
fn __putchar(c: u8) { crate::hal::uart_16550::putchar(c); }
```

### 5. Paging gate (~line 170-186)

Hiện tại:
```rust
#[cfg(not(any(target_arch = "x86_64", target_arch = "riscv32")))]
{ /* build + activate page tables (RV64, AArch64) */ }
#[cfg(target_arch = "x86_64")]
{ println!("Paging: using Limine PML4"); }
#[cfg(target_arch = "riscv32")]
{ println!("Paging: bare physical"); }
```

x86_32 và arm cả hai dùng bare physical — thêm vào condition:

```rust
// Đổi condition để exclude cả x86 và arm khỏi "build page tables" path
#[cfg(not(any(target_arch = "x86_64", target_arch = "riscv32", target_arch = "x86", target_arch = "arm")))]
{ /* build + activate page tables */ }

// Giữ x86_64 gate
#[cfg(target_arch = "x86_64")]
{ println!("Paging: using Limine PML4"); }

// Expand bare physical condition
#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
{ println!("Paging: bare physical"); }
```

### 6. Interrupt enable (~line 334-344)

Hiện tại có 3 branches. Cần verify x86 và arm được cover:
- `#[cfg(target_arch = "x86_64")]` → `sti` 
- `#[cfg(target_arch = "aarch64")]` → `daifclr #2`
- `#[cfg(any(risc-v arches))]` → `csrsi sstatus, 2`

Thêm:
```rust
#[cfg(target_arch = "x86")]
unsafe { core::arch::asm!("sti"); }
#[cfg(target_arch = "arm")]
{ hal::ARCH.enable_interrupts(); }  // calls cpsie i
```

### 7. Panic putchar (~line 381-387)

Thêm:
```rust
#[cfg(target_arch = "arm")]
fn panic_putchar(c: u8) { unsafe { crate::hal::uart_pl011::putchar(c); } }
#[cfg(target_arch = "x86")]
fn panic_putchar(c: u8) { unsafe { crate::hal::uart_16550::putchar(c); } }
```

### 8. Panic halt (~line 372-376)

Hiện tại:
```rust
#[cfg(target_arch = "x86_64")]
unsafe { core::arch::asm!("cli; hlt"); }
#[cfg(not(target_arch = "x86_64"))]
unsafe { core::arch::asm!("wfi"); }
```

Thêm x86_32 vào `cli; hlt` path:
```rust
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
unsafe { core::arch::asm!("cli; hlt"); }
#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
unsafe { core::arch::asm!("wfi"); }
```

(AArch32 `wfi` là instruction hợp lệ trong ARMv7-A SVC mode)

## Implementation Steps (thứ tự thực hiện)

1. Tạo `kernel/linker-x86-32.ld`
2. Tạo `kernel/linker-aarch32.ld`
3. Edit `kernel/build.rs` — thêm 2 cases
4. Edit `kernel/src/boot.rs` — thêm 2 FALLBACK_BOOT_INFO
5. Edit `kernel/src/main.rs` — từng gate theo thứ tự (UART → HAL → paging → interrupt → panic)
6. Verify `cargo check --target i686-unknown-none -p vicell-kernel`
7. Verify `cargo check --target armv7a-none-eabi -p vicell-kernel`
8. Verify không break existing arches: `cargo check -p vicell-kernel` (RV64 default)

## Todo List

- [ ] Tạo `kernel/linker-x86-32.ld`
- [ ] Tạo `kernel/linker-aarch32.ld`
- [ ] Edit `kernel/build.rs` — `"x86"` và `"arm"` cases
- [ ] Edit `kernel/src/boot.rs` — 2 FALLBACK_BOOT_INFO statics
- [ ] Edit `kernel/src/main.rs` — UART init gates
- [ ] Edit `kernel/src/main.rs` — putchar dispatch
- [ ] Edit `kernel/src/main.rs` — paging path (bare physical cho arm + x86)
- [ ] Edit `kernel/src/main.rs` — interrupt enable
- [ ] Edit `kernel/src/main.rs` — panic halt/putchar
- [ ] Verify `cargo check` cho cả 6 arches không lỗi

## Success Criteria

- `cargo check --target i686-unknown-none -p vicell-kernel` → OK
- `cargo check --target armv7a-none-eabi -p vicell-kernel` → OK
- `cargo check -p vicell-kernel` (RV64 default) → OK (no regression)
- `cargo check --target x86_64-unknown-none -p vicell-kernel` → OK (no regression)
- `cargo check --target aarch64-unknown-none -p vicell-kernel` → OK (no regression)

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `SimpleBootInfo` field names khác assumption | Đọc `boot.rs` trước khi implement |
| `hal::uart_16550` module path khác cho x86_32 | Kiểm tra `hal/arch/x86/src/x86_32.rs` exports |
| main.rs gate conflicts (e.g., `not(x86_64)` bây giờ include x86_32 nhưng cần exclude) | Enumerate tất cả `not(...)` gates, verify không có unintended matches |
| `cargo check` đúng nhưng QEMU không boot (multiboot header offset sai) | Verify với `objdump -h` rằng `.multiboot` section đứng đầu binary |
| linker orphan sections warning | Thêm `/DISCARD/` rule cho các sections không mong muốn |

## Security Considerations

- FALLBACK_BOOT_INFO: `memory_size` phải conservative (không vượt quá RAM thật của QEMU)
- Paging bare physical: stack overflow và buffer overruns không có hardware protection — chấp nhận được cho nano bringup
