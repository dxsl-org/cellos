// SPDX-License-Identifier: MIT
// ViCell inline-asm syscall helper.
// Matches the ViCell ABI exactly — NOT Linux ABI.
//
// riscv64: a7=syscall_nr, a0-a3=args, a0=ret, ecall
// aarch64: x0=syscall_nr, x1-x4=args, x0=ret, svc #0  <-- CRITICAL: x0=nr (not x8)
// x86_64:  rax=syscall_nr, rdi/rsi/rdx/r10=args, rax=ret, syscall (rcx/r11 clobbered)
#pragma once

#include <stddef.h>

// ViCell syscall opcode constants (from libs/api/src/syscall.rs — must stay in sync)
#define VI_SYS_EXIT       60
#define VI_SYS_SPAWN       5
#define VI_SYS_SET_TLS_BASE 9
#define VI_SYS_FUTEX_WAIT 17
#define VI_SYS_FUTEX_WAKE 18
#define VI_SYS_YIELD     104
#define VI_SYS_GET_RANDOM 214
#define VI_SYS_MEM_INFO  243
#define VI_SYS_LOG        11
#define VI_SYS_OPEN      101
#define VI_SYS_READ      102
#define VI_SYS_CLOSE     103
#define VI_SYS_SEEK      106
#define VI_SYS_FILEOP    107
#define VI_SYS_WRITE     109
#define VI_SYS_GETTIME   120
#define VI_SYS_FSTAT     254

// GetTime op-selectors (kernel/src/task/syscall.rs handler)
#define VI_GETTIME_TICKS  0   // monotonic 10MHz ticks since boot
#define VI_GETTIME_NS     2   // epoch nanoseconds (RTC, wall clock)
#define VI_GETTIME_SECS   3   // epoch seconds (RTC, wall clock)

// FutexWait timeouts count 10 ms scheduler ticks; 0 means "wait forever".
#define VI_FUTEX_TICK_NS  10000000L
#define VI_FUTEX_WOKEN    0
#define VI_FUTEX_MISMATCH 1
#define VI_FUTEX_TIMEOUT  2

#if defined(__riscv) && __riscv_xlen == 64

static inline long vicell_syscall(long id, long a0, long a1, long a2, long a3) {
    register long _a7 __asm__("a7") = id;
    register long _a0 __asm__("a0") = a0;
    register long _a1 __asm__("a1") = a1;
    register long _a2 __asm__("a2") = a2;
    register long _a3 __asm__("a3") = a3;
    __asm__ volatile("ecall"
        : "+r"(_a0)
        : "r"(_a7), "r"(_a1), "r"(_a2), "r"(_a3)
        : "memory");
    return _a0;
}

#elif defined(__aarch64__)

// IMPORTANT: ViCell aarch64 ABI puts syscall_nr in x0, args in x1-x4.
// Linux puts nr in x8, args in x0-x5. Using Linux register layout here
// WILL silently misdispatch every call.
static inline long vicell_syscall(long id, long a0, long a1, long a2, long a3) {
    register long _x0 __asm__("x0") = id;
    register long _x1 __asm__("x1") = a0;
    register long _x2 __asm__("x2") = a1;
    register long _x3 __asm__("x3") = a2;
    register long _x4 __asm__("x4") = a3;
    __asm__ volatile("svc #0"
        : "+r"(_x0)
        : "r"(_x1), "r"(_x2), "r"(_x3), "r"(_x4)
        : "memory");
    return _x0;
}

#elif defined(__x86_64__)

// Mirrors libs/ostd/src/syscall.rs (x86_64 arm): R10, not RCX, carries the
// fourth argument because SYSCALL overwrites RCX with the user RIP.
static inline long vicell_syscall(long id, long a0, long a1, long a2, long a3) {
    long ret;
    register long _r10 __asm__("r10") = a3;
    __asm__ volatile("syscall"
        : "=a"(ret)
        : "a"(id), "D"(a0), "S"(a1), "d"(a2), "r"(_r10)
        : "rcx", "r11", "memory");
    return ret;
}

#else
#error "vicell/syscall.h: unsupported target architecture (riscv64, aarch64 or x86_64)"
#endif

// Sentinel for failed syscalls (matches usize::MAX from Rust side)
#define VI_SYSCALL_ERR  ((long)-1L)
