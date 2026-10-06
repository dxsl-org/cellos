/* Cellos freestanding setjmp.h. This QuickJS release includes the header in
 * dtoa.c but never jumps; the Tier-A ABI exports setjmp/longjmp anyway, so the
 * declarations are real rather than stubs. */
#ifndef QJS_VIOS_SETJMP_H
#define QJS_VIOS_SETJMP_H

#include <stdint.h>

typedef uintptr_t __jmp_buf[32];
typedef __jmp_buf jmp_buf[1];

int setjmp(jmp_buf env);
void longjmp(jmp_buf env, int val) __attribute__((noreturn));

#endif
