# Thread Spawning Debug Report

## Issue
Thread được spawn thành công (Task ID 3 created) nhưng `thread_entry` không được gọi.

## Evidence
```
[INFO] Thread 'thread' (ID 3): Stack 0x800837C0-0x800A37C0, Entry 0x80023C78, Arg 0x800630A0
[INFO] USER: OSTd: sys_spawn called (Entry: 0x80023C44, Arg: 0x800631C0)
```

- Syscall được gọi: Entry = 0x80023C44 (thread_entry address)
- Thread được tạo với Entry = 0x80023C78 (trampoline address)
- Argument = 0x800631C0 (boxed closure pointer)

## Problem
`thread_entry` không in log "Entered thread_entry", nghĩa là trampoline không nhảy đến `thread_entry`.

## Root Cause Analysis

### Trampoline Code
```rust
pub static THREAD_TRAMPOLINE: [u32; 2] = [
    0x00840513,  // mv a0, s0
    0x00048067,  // jr s1
];
```

### Context Initialization
```rust
task.context.s0 = arg;   // Argument
task.context.s1 = entry; // Real Entry Point (thread_entry)
task.context.ra = trampoline; // Jump to Trampoline
```

### Expected Flow
1. Context switch loads `ra` = trampoline address
2. `ret` instruction jumps to trampoline
3. Trampoline executes:
   - `mv a0, s0` → Move arg to a0
   - `jr s1` → Jump to thread_entry
4. thread_entry runs with arg in a0

### Actual Problem
Trampoline instructions are stored as **data** (static array), not executable code!

## Solution
Use `global_asm!` properly or write trampoline to executable section.

### Option 1: Fix global_asm syntax
```rust
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    ".section .text",
    ".global thread_trampoline",
    ".align 4",
    "thread_trampoline:",
    "    mv a0, s0",
    "    jr s1"
);
```

### Option 2: Use naked function
```rust
#[naked]
#[no_mangle]
pub unsafe extern "C" fn thread_trampoline() {
    core::arch::asm!(
        "mv a0, s0",
        "jr s1",
        options(noreturn)
    );
}
```

## Next Steps
1. Implement proper trampoline using global_asm
2. Verify trampoline address is in .text section
3. Test thread execution
