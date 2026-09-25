/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Internal port-platform syscall boundary.
 *
 * Every port-platform C adapter reaches the kernel through this one thunk, so
 * the register contract and the log/marker helpers live in exactly one place.
 * This header is porting-kit implementation, not part of the published C hook
 * boundary: a ported application includes cellos_platform.h, cellos_pthread.h,
 * or cellos_spawn.h and never this file.
 */
#ifndef CELLOS_SYSCALL_H
#define CELLOS_SYSCALL_H

#include <stddef.h>
#include <stdint.h>

/* Kernel ABI opcodes (libs/api/src/abi/syscall.rs). */
#define CELLOS_SYSCALL_LOG 11u

#define CELLOS_DENIED ((intptr_t)-1)

#if defined(__riscv)
static inline intptr_t cellos_syscall4(uintptr_t number, uintptr_t a0, uintptr_t a1,
                                       uintptr_t a2, uintptr_t a3) {
    register uintptr_t x10 __asm__("a0") = a0;
    register uintptr_t x11 __asm__("a1") = a1;
    register uintptr_t x12 __asm__("a2") = a2;
    register uintptr_t x13 __asm__("a3") = a3;
    register uintptr_t x17 __asm__("a7") = number;
    __asm__ volatile("ecall"
                     : "+r"(x10)
                     : "r"(x11), "r"(x12), "r"(x13), "r"(x17)
                     : "memory");
    return (intptr_t)x10;
}
#else
/* Host-only contract tests supply this backend; Cell builds require RISC-V. */
extern intptr_t cellos_test_syscall4(uintptr_t number, uintptr_t a0, uintptr_t a1,
                                     uintptr_t a2, uintptr_t a3);
static inline intptr_t cellos_syscall4(uintptr_t number, uintptr_t a0, uintptr_t a1,
                                       uintptr_t a2, uintptr_t a3) {
    return cellos_test_syscall4(number, a0, a1, a2, a3);
}
#endif

/* Emit one already-formatted line to the kernel's user log. */
static inline void cellos_log(const char *bytes, size_t length) {
    (void)cellos_syscall4(CELLOS_SYSCALL_LOG, (uintptr_t)bytes, length, 0, 0);
}

/* Emit a fixed marker line, so a witness marker is one call, not a strlen. */
static inline void cellos_log_text(const char *text) {
    size_t length = 0;
    while (text[length] != '\0') {
        length++;
    }
    cellos_log(text, length);
}

/* Emit `label` followed by one decimal value and a newline. */
static inline void cellos_log_value(const char *label, uintptr_t value) {
    char line[96];
    char digits[20];
    size_t count = 0;
    size_t at = 0;
    size_t index;

    while (label[at] != '\0' && at + 24 < sizeof line) {
        line[at] = label[at];
        at++;
    }
    if (value == 0) {
        digits[count++] = '0';
    } else {
        while (value > 0 && count < sizeof digits) {
            digits[count++] = (char)('0' + (char)(value % 10u));
            value /= 10u;
        }
    }
    for (index = count; index > 0; index--) {
        line[at++] = digits[index - 1];
    }
    line[at++] = '\n';
    cellos_log(line, at);
}

#endif
