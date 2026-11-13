/*
 * setjmp/longjmp round-trip probe for the c-math-smoke cell.
 *
 * Why this lives in C: setjmp returns twice, and only a C compiler models
 * that (the returns_twice attribute pins locals and forbids caching values
 * across the call). rustc has no returns_twice concept, so calling setjmp from
 * Rust is undefined behaviour — the second return may observe miscompiled
 * state. Rust calls c_math_smoke_setjmp_probe() and only sees an ordinary
 * function returning a status.
 *
 * Freestanding build: there is no <setjmp.h>, so the posix shim's symbols
 * (libs/posix-shim/src/setjmp.rs) are declared here. The buffer is
 * sized for the largest shim layout (aarch64: 22 u64 words; riscv64 uses 16,
 * x86_64 9) so one definition fits every target.
 */

#define C_MATH_SMOKE_JMP_BUF_WORDS 22

typedef unsigned long long c_math_smoke_jmp_buf[C_MATH_SMOKE_JMP_BUF_WORDS];

extern int setjmp(c_math_smoke_jmp_buf env) __attribute__((returns_twice));
extern void longjmp(c_math_smoke_jmp_buf env, int val) __attribute__((noreturn));

/* Value carried by the longjmp; any non-zero value distinguishes the second
 * return from the first. */
#define PROBE_JUMP_VALUE 42

/*
 * Runs one setjmp → longjmp(42) → setjmp-returns-42 round trip.
 *
 * Returns 0 when setjmp first returns 0, longjmp transfers control back
 * exactly once, and the second return carries PROBE_JUMP_VALUE; -1 otherwise.
 */
int c_math_smoke_setjmp_probe(void)
{
    c_math_smoke_jmp_buf env;
    /* volatile: modified between setjmp and longjmp, so C11 7.13.2.1p3 makes a
     * non-volatile copy indeterminate after the jump. */
    volatile int jumps = 0;

    /* setjmp as the whole controlling expression of a selection statement:
     * one of the only contexts C11 7.13.1.1p4 defines for it. */
    switch (setjmp(env)) {
    case 0:
        jumps = jumps + 1;
        longjmp(env, PROBE_JUMP_VALUE);
    case PROBE_JUMP_VALUE:
        return jumps == 1 ? 0 : -1;
    default:
        return -1;
    }
}
