# Hardware Abstraction Layer (HAL) - RISC-V Implementation Plan

## Overview
This document outlines the implementation of a real Hardware Abstraction Layer for RISC-V architecture, enabling ViOS to run on bare metal.

## Target Platform
**Primary**: QEMU RISC-V `virt` machine
- Architecture: `riscv64gc-unknown-none-elf`
- Memory: 128MB RAM
- UART: NS16550A at 0x10000000
- PLIC: Platform-Level Interrupt Controller
- CLINT: Core-Local Interruptor

## Implementation Phases

### Phase 1: Boot & Memory
- [ ] Create linker script (`kernel.ld`)
- [ ] Implement `_start` assembly entry point
- [ ] Set up stack pointer
- [ ] Initialize `.bss` and `.data` sections
- [ ] Jump to Rust `kmain()`

### Phase 2: Interrupts & Exceptions
- [ ] Implement trap handler in assembly
- [ ] Set up `stvec` (trap vector)
- [ ] Handle timer interrupts (CLINT)
- [ ] Handle external interrupts (PLIC)
- [ ] Implement context switching

### Phase 3: HAL Traits Implementation
- [ ] `hal-riscv` crate with platform-specific code
- [ ] Implement `SerialPort` for NS16550A (already done in `hal-uart`)
- [ ] Implement timer interface
- [ ] Implement interrupt controller interface

### Phase 4: Testing
- [ ] Boot in QEMU
- [ ] Verify UART output
- [ ] Test timer interrupts
- [ ] Verify scheduler context switching

## File Structure
```
hal/
  hal-riscv/
    Cargo.toml
    src/
      lib.rs           # Platform initialization
      boot.s           # Assembly entry point
      trap.s           # Trap handler assembly
      linker.ld        # Linker script
      timer.rs         # CLINT timer
      plic.rs          # Interrupt controller
```

## Next Steps
1. Create `hal-riscv` crate
2. Write linker script
3. Implement boot sequence
4. Test in QEMU
