# Phase 04 — Kernel RV32 Integration + QEMU Smoke Boot

**Status**: 📋 PLANNED
**Priority**: P3
**Effort**: 3 days
**Depends on**: Phase 01 + Phase 02 + Phase 03

---

## Context Links

- Kernel entry: `kernel/src/main.rs` line 41 (`kmain`)
- Kernel Cargo.toml: `kernel/Cargo.toml` lines 19-24 (target-specific deps)
- Existing linker scripts: `kernel/linker.ld` (RV64), `kernel/linker-aarch64.ld`
- Existing run script: `run.ps1` (RV64)
- Kernel memory init: `kernel/src/memory/paging.rs`

---

## Overview

Wires the completed RV32 HAL into a kernel binary that compiles, links, and boots on
QEMU RV32 virt. The ViCell-Nano profile is implemented via `#[cfg(target_arch = "riscv32")]`
gates — no separate Cargo feature flag needed for Phase 31.

Key scope reductions for the Nano boot:
- **No SV32 paging** — SATP=0 (bare physical addressing in S-mode, valid on QEMU RV32 virt)
- **No VirtIO GPU / compositor**
- **No snapshot module** (no disk on MCU targets)
- **No smoltcp network** (defer to Phase 32)
- **Same NS16550A UART** as RV64 (QEMU RV32 virt uses the same UART address 0x10000000)
- **Same shell + init cells** — these compile cleanly for RV32 since they're `#![forbid(unsafe_code)]`

`kmain(hartid: usize, dtb: usize)` already uses `usize` — automatically 32-bit on RV32. No
signature change needed.

---

## Key Insights

**Linker script for RV32:**
- ORIGIN = 0x80200000 (same as RV64 with OpenSBI)
- No `.rela.dyn` section (non-PIE)
- Stack: 32KB (down from 64KB — Nano budget)
- Guard page at stack bottom (`__stack_guard_page`)
- Manifest section `.ViCell_manifest` must still be present (kernel reads it for self-manifest)
- Memory: 128MB RAM (same QEMU virt allocation)
- Output arch: `OUTPUT_ARCH(riscv)` — same for both RV32 and RV64

**kernel/Cargo.toml:**
```toml
[target.'cfg(target_arch = "riscv32")'.dependencies]
hal = { path = "../hal/core", package = "hal-core",
        default-features = false, features = ["riscv32"] }
```
No `riscv = "0.16.0"` dep for RV32 — the RV32 HAL uses inline ASM, not the riscv crate.

**kernel/src/main.rs changes:**
- `#[cfg(target_arch = "riscv64")]` → add parallel `#[cfg(target_arch = "riscv32")]` blocks
- UART init: `task::drivers::uart::init()` — works for both (same NS16550A driver)
  - Change to `#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]`
- `_putchar` (used in `puts` closure): same pattern — riscv32 also links to `_putchar`
- Skip `task::drivers::init()` on RV32 (no VirtIO GPU/net/keyboard)
- Skip `try_restore()` / snapshot on RV32 (`#[cfg(not(target_arch = "riscv32"))]`)
- Skip PLIC init on RV32 (no PLIC on QEMU RV32 virt in Nano mode — use SBI for timer only)
- Memory init: RV32 uses `memory::init_bare()` (new function, SATP=0) instead of `memory::init()`
  - `init_bare()` is a new fn that initializes the frame allocator but does NOT enable the MMU

**kernel/src/memory/paging.rs:**
- Add `#[cfg(target_arch = "riscv32")] pub fn init_bare()` — frame allocator init only, no SATP write
- This function does NOT set `satp` — kernel runs in bare physical mode

**Syscall dispatch stub for RV32:**
- `vi_trap_handler32` calls `ViCell_syscall_dispatch32`
- Need a stub in `kernel/src/task/syscall.rs`:
  ```rust
  #[cfg(target_arch = "riscv32")]
  #[no_mangle]
  pub extern "Rust" fn ViCell_syscall_dispatch32(frame: &mut hal::rv32::trap::ViTrapFrame32) {
      // TODO Phase 32: full syscall dispatch; for Nano boot just handle basic IPC
      let _ = frame;
  }
  ```

**Build command:**
```powershell
cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf `
  -Z build-std=core,alloc `
  --config "build.rustflags=['-C', 'link-arg=-Tlinker-riscv32.ld', '-C', 'link-arg=-Lkernel']"
```
Or add a `.cargo/config-rv32.toml` for convenience.

---

## Related Code Files

| File | Action |
|------|--------|
| `kernel/linker-riscv32.ld` | CREATE — non-PIE RV32 linker script |
| `kernel/Cargo.toml` | MODIFY — add `[target.riscv32.dependencies]` |
| `kernel/src/main.rs` | MODIFY — add `#[cfg(target_arch = "riscv32")]` branches |
| `kernel/src/memory/paging.rs` | MODIFY — add `init_bare()` for RV32 (no SATP) |
| `kernel/src/task/syscall.rs` | MODIFY — add `ViCell_syscall_dispatch32` stub |
| `run-rv32-virt.ps1` | CREATE — QEMU launch script for RV32 |
| `docs/specs/04-hardware.md` | MODIFY — add RV32 section |
| `docs/project-roadmap.md` | MODIFY — Phase 31 status update on completion |

---

## Implementation Steps

1. **Create `kernel/linker-riscv32.ld`** (base from `kernel/linker.ld`, stripped of PIE sections):
   ```ld
   OUTPUT_ARCH(riscv)
   ENTRY(_start)

   MEMORY {
       ram (wxa) : ORIGIN = 0x80200000, LENGTH = 128M
   }

   SECTIONS {
       .text : {
           KEEP(*(.text.boot))
           *(.text .text.*)
       } >ram

       .rodata : ALIGN(4096) {
           *(.rodata .rodata.*)
           *(.srodata .srodata.*)
       } >ram

       .data : ALIGN(4096) {
           . = ALIGN(16);
           __global_pointer$ = . + 0x800;
           *(.data .data.*)
           *(.sdata .sdata.*)
       } >ram

       .bss : ALIGN(4096) {
           __bss_start = .;
           *(.bss .bss.*)
           *(.sbss .sbss.*)
           *(COMMON)
           __bss_end = .;
       } >ram

       /* Kernel stack: 32 KB (Nano profile — half of RV64) */
       .kernel_stack (NOLOAD) : ALIGN(4096) {
           __stack_guard_page = .;
           . = . + 0x1000;     /* 4 KB guard page */
           __stack_bottom = .;
           . = . + 0x8000;     /* 32 KB stack */
           __stack_top = .;
       } >ram

       /* ViCell cell manifest — must be present even in Nano kernel */
       __ViCell_manifest : {
           KEEP(*(__ViCell_manifest))
       } >ram

       /DISCARD/ : {
           *(.eh_frame)
           *(.rela .rela.*)
       }
   }
   ```

2. **Add `[target.riscv32.dependencies]`** in `kernel/Cargo.toml`:
   ```toml
   [target.'cfg(target_arch = "riscv32")'.dependencies]
   hal = { path = "../hal/core", package = "hal-core",
           default-features = false, features = ["riscv32"] }
   ```

3. **Update `kernel/src/main.rs`**:

   a. UART init: change `#[cfg(target_arch = "riscv64")]` to cover riscv32 too:
   ```rust
   #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
   task::drivers::uart::init();
   ```

   b. `_putchar` import: add riscv32 variant:
   ```rust
   #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
   use api::posix::_putchar;
   ```

   c. `puts` closure: same `_putchar` call for both RISC-V arches:
   ```rust
   #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
   unsafe { _putchar(c as u8); }
   ```

   d. VirtIO driver init — wrap in `#[cfg(not(target_arch = "riscv32"))]`

   e. Snapshot call — wrap in `#[cfg(not(target_arch = "riscv32"))]`

   f. Memory init — add RV32 path:
   ```rust
   #[cfg(target_arch = "riscv32")]
   memory::paging::init_bare();
   #[cfg(not(target_arch = "riscv32"))]
   memory::init();  // existing call
   ```

4. **Add `init_bare()` to `kernel/src/memory/paging.rs`**:
   ```rust
   #[cfg(target_arch = "riscv32")]
   pub fn init_bare() {
       // Nano profile: run in bare physical address mode (SATP=0).
       // Initialize frame allocator only — do NOT write satp.
       // This is valid in S-mode on QEMU RV32 virt with OpenSBI.
       use crate::memory::frame;
       frame::init();
   }
   ```

5. **Add `ViCell_syscall_dispatch32` stub** in `kernel/src/task/syscall.rs`:
   ```rust
   #[cfg(target_arch = "riscv32")]
   #[no_mangle]
   pub extern "Rust" fn ViCell_syscall_dispatch32(
       _frame: &mut hal::rv32::trap::ViTrapFrame32,
   ) {
       // Phase 32: implement full syscall dispatch for RV32.
   }
   ```

6. **Create `run-rv32-virt.ps1`**:
   ```powershell
   #!/usr/bin/env pwsh
   $kernel = "target/riscv32imac-unknown-none-elf/release/vicell-kernel"
   if (-not (Test-Path $kernel)) {
       Write-Error "Kernel not found. Run: cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf --release"
       exit 1
   }
   qemu-system-riscv32 `
     -machine virt `
     -cpu rv32 `
     -m 128M `
     -bios default `
     -kernel $kernel `
     -nographic `
     -no-reboot
   ```

7. **Build and smoke-boot**:
   ```powershell
   cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf `
     -Z build-std=core,alloc
   # Then run:
   ./run-rv32-virt.ps1
   # Expected: ViCell banner + "ViCell>" prompt
   ```

8. **Update docs** (`docs/specs/04-hardware.md` — add RV32 target table):
   Document that Phase 31 targets `riscv32imac-unknown-none-elf` on QEMU RV32 virt,
   S-mode + OpenSBI, SATP=0 (no virtual memory), 128MB RAM. CHERIoT-IBEX deferred.

---

## Todo List

- [ ] Create `kernel/linker-riscv32.ld`
- [ ] Add riscv32 target dep in `kernel/Cargo.toml`
- [ ] Fix UART + _putchar cfg gates in `main.rs`
- [ ] Wrap VirtIO init in `#[cfg(not(target_arch = "riscv32"))]`
- [ ] Wrap snapshot calls in `#[cfg(not(target_arch = "riscv32"))]`
- [ ] Add `memory::paging::init_bare()` for RV32
- [ ] Add `ViCell_syscall_dispatch32` stub in syscall.rs
- [ ] Create `run-rv32-virt.ps1`
- [ ] `cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf` succeeds
- [ ] QEMU RV32 virt boots to `ViCell>` prompt (serial output)
- [ ] RV64 build still clean: `cargo check -p vicell-kernel` passes
- [ ] Update `docs/specs/04-hardware.md`
- [ ] Update `docs/project-roadmap.md` Phase 31 status

---

## Success Criteria

- [ ] `cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf` produces an ELF
- [ ] QEMU RV32 virt serial output includes `ViCell>` prompt
- [ ] Serial log shows ViCell banner (`[INFO] ViCell kernel starting`)
- [ ] No RV64 regressions: existing `cargo check` passes
- [ ] `run-rv32-virt.ps1` exists and documents the QEMU invocation

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `-Z build-std=core,alloc` requires nightly | ViCell already requires nightly (`#![feature(alloc_error_handler)]`) |
| Cell ELFs embedded as `include_bytes!` won't cross-compile cleanly | cfg-gate `INIT_ELF` embed under `#[cfg(not(target_arch = "riscv32"))]` for Phase 31; Nano init is minimal |
| `snapshot` module unconditionally included in main.rs | Move `pub mod snapshot` under `#[cfg(not(target_arch = "riscv32"))]` |
| `virtio-drivers` crate may fail on RV32 | Gate `use virtio_drivers::...` behind `#[cfg(not(target_arch = "riscv32"))]` |
| `fatfs` crate may have u64-only APIs that break on RV32 | Gate or stub — FAT32 not needed for Nano boot MVP |

---

## Security Considerations

- SATP=0 (bare mode): kernel and cells share the same physical address space — Rust type
  system (LBI) is the ONLY isolation mechanism on RV32 Nano. This is intentional for Phase 31.
  PMP-based hardware isolation is Phase 32+.
- Cell `#![forbid(unsafe_code)]` still applies — cells cannot perform MMIO or write arbitrary
  memory even without hardware PMP.
