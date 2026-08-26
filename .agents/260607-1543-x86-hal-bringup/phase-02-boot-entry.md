# Phase 02 — x86_64 Boot Entry + `_start`

**Status:** TODO  
**Priority:** Critical — `kernel/linker-x86-64.ld` declares `ENTRY(_start)` but no `_start` exists  
**Estimated effort:** Small (new file ~50 lines + 2-line facade change)

---

## Context Links

- `kernel/linker-x86-64.ld:2` — `ENTRY(_start)` 
- `hal/arch/x86/src/x86_64.rs` — module declarations (needs `pub mod boot;`)
- `kernel/src/main.rs:42` — `kmain(hartid: usize, dtb: usize)` — wrong signature for x86_64 Limine
- Research: Limine report §3 (asm stub), §5 (RSP alignment)

---

## Overview

Limine jumps to `_start` on x86_64 with:
- Long mode active, paging already set up by Limine (kernel at 0xFFFFFFFF80000000)
- All GPRs = 0 except RSP (Limine stack, ≥64 KiB)
- RSP = 8-byte aligned (Limine pre-pushes a 0 return address before jumping)
- IF = 0 (interrupts masked), IDT undefined

We need:
1. `_start` asm stub: align RSP to 16 bytes, switch to kernel `.stack`, call `kmain_x86`
2. `kmain_x86` bridge: a `no_mangle extern "C"` function that calls `kmain(0, 0)` — passing
   `hartid=0, dtb=0` is safe because `kmain` guards all RISC-V-specific paths with
   `#[cfg(target_arch = "riscv64")]` and `cpu_features::detect` is a no-op when `dtb=0`

---

## Requirements

- `_start` symbol is defined and placed in `.text.boot` section
- RSP is switched from Limine's bootloader-reclaimable stack to the kernel `.stack` section
  before any Rust code that could trigger stack-relative addressing
- `kmain_x86` is a valid `extern "C"` entry that bridges to `kmain`
- The stack switch happens BEFORE the call to `kmain_x86` so that the bootloader stack
  can be reclaimed later (Limine `BootloaderReclaimable` memory type)

---

## Architecture

### File layout

```
hal/arch/x86/src/x86_64/
  boot.rs    ← NEW
  ...
hal/arch/x86/src/x86_64.rs  ← add `pub mod boot;`
```

### `boot.rs` design

```rust
// hal/arch/x86/src/x86_64/boot.rs
use core::arch::global_asm;

// External symbols from linker-x86-64.ld
extern "C" {
    static __stack_top: u8;
}

global_asm!(
    ".section .text.boot, \"ax\"",
    ".global _start",
    "_start:",
    // Switch to kernel .stack (defined in linker-x86-64.ld) before any Rust code.
    // This moves us off the Limine-reclaimable bootloader stack onto kernel .bss.
    "lea rsp, [rip + {stack_top}]",
    // Align to 16 bytes for SysV ABI (required before any CALL that may use SSE).
    "and rsp, -16",
    // Clear the frame pointer for debuggability.
    "xor rbp, rbp",
    // Jump into Rust bridge (not CALL — so the return address is aligned correctly).
    "jmp {entry}",
    stack_top = sym __stack_top,
    entry = sym kmain_x86,
);

/// Rust entry bridge.
///
/// Called from _start with a valid kernel stack and 16-byte-aligned RSP.
/// Passes `hartid=0, dtb=0` — both are ignored on x86_64 by the guards in
/// `kmain` and `cpu_features::detect`.
#[no_mangle]
pub extern "C" fn kmain_x86() -> ! {
    extern "C" {
        fn kmain(hartid: usize, dtb: usize) -> !;
    }
    // SAFETY: kmain does not read hartid/dtb on x86_64 (all accesses are
    // gated on #[cfg(target_arch = "riscv64")]).
    unsafe { kmain(0, 0) }
}
```

### Linker note

`__stack_top` is already defined in `linker-x86-64.ld:33`. The `.stack` section is
`ALIGN(4096)` and 64 KiB; `__stack_top` points past the top. Using it as the initial
RSP gives us a clean, BSS-allocated stack that survives `reclaim_bootloader_memory`.

---

## Related Code Files

| Action | File |
|--------|------|
| Create | `hal/arch/x86/src/x86_64/boot.rs` |
| Modify | `hal/arch/x86/src/x86_64.rs` — add `#[cfg(target_arch = "x86_64")] pub mod boot;` |

---

## Implementation Steps

1. Create `hal/arch/x86/src/x86_64/boot.rs` with `global_asm!` stub + `kmain_x86` bridge
2. Add `#[cfg(target_arch = "x86_64")] pub mod boot;` to `hal/arch/x86/src/x86_64.rs` (line 16)
3. Run `cargo check -p hal-x86 --target x86_64-unknown-none`

---

## Success Criteria

- `cargo check -p hal-x86 --target x86_64-unknown-none` exits 0
- `_start` symbol present in the compiled ELF (`nm` or `objdump -t`)
- `.text.boot` section appears first in the x86_64 kernel binary

---

## Risk Assessment

- **LOW** — the asm stub is minimal; RSP alignment + `__stack_top` load pattern is canonical
- Risk: `__stack_top` symbol visibility. It is a linker symbol declared as `PROVIDE`-style; 
  Rust `extern "C"` can reference it via `sym __stack_top` in `global_asm!`. If it needs
  `#[no_mangle]` — it won't, linker symbols are always exported.
- Risk: `jmp` vs `call` for entry — using `jmp` means the return slot is never used, 
  which is correct since `kmain_x86 -> !` never returns.

---

## Security Considerations

- Stack is switched to kernel BSS (not bootloader memory) before any Rust execution.
  This ensures the stack is in a known, non-reclaimable region from the first Rust frame.
