/* T-2 reproducer driver (host, x86_64 SysV, hard-float like the cell's C).
 *
 * Runs JavaScript in the ocel-quickjs engine archive exactly as the x86_64 cell
 * build compiles it (SSE2), with every math symbol resolved by soft-float Rust
 * built for x86_64-unknown-none (lib.rs). Only allocation, strings, stdio and
 * the clock come from the host libc; none of those carry a float.
 *
 * Each case prints the engine's own String(value) — formatted by QuickJS's C
 * dtoa, which never crosses the boundary — against the expected text. The
 * expected values are Rust libm 0.2.16's, which is what every architecture
 * computes: its exp(1) is 0x4005bf0a8b14576a, one ulp above e, on a hard-float
 * host as well.
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "quickjs.h"

/* lib.rs: how the cell's Rust reads a float JSValue, as IEEE-754 bits. */
uint64_t t2_rust_reads_float(JSContext *ctx, JSValue v);

static const struct {
    const char *js;
    const char *expect;
} CASES[] = {
    {"Math.sqrt(2)", "1.4142135623730951"},
    {"2 ** 0.5", "1.4142135623730951"},
    {"Math.pow(2.5, 2)", "6.25"},
    {"Math.sin(0.5)", "0.479425538604203"},
    {"Math.exp(1)", "2.7182818284590455"},
    {"Math.atan2(1, 1)", "0.7853981633974483"},
    {"Math.hypot(3, 4)", "5"},
    {"Math.cbrt(27)", "3"},
    {"Math.log2(8)", "3"},
    {"Math.floor(-2.5)", "-3"},
    {"Math.round(-2.5)", "-2"},
    {"5.5 % 2", "1.5"},
    {"Array.from(new Uint8ClampedArray([1.5, 2.5, 254.5])).join()", "2,2,254"},
};

int main(void)
{
    JSRuntime *rt = JS_NewRuntime();
    JSContext *ctx = JS_NewContext(rt);
    int bad = 0;

    for (size_t i = 0; i < sizeof CASES / sizeof CASES[0]; i++) {
        char src[256];
        snprintf(src, sizeof src, "String(%s)", CASES[i].js);
        JSValue v = JS_Eval(ctx, src, strlen(src), "<t2>", JS_EVAL_TYPE_GLOBAL);
        const char *got = JS_IsException(v) ? NULL : JS_ToCString(ctx, v);
        int ok = got && strcmp(got, CASES[i].expect) == 0;
        bad += !ok;
        printf("%-4s C->Rust  %-62s = %-22s (expect %s)\n", ok ? "OK" : "BAD", CASES[i].js,
               got ? got : "<exception>", CASES[i].expect);
        if (got)
            JS_FreeCString(ctx, got);
        JS_FreeValue(ctx, v);
    }

    /* Rust->C: the engine returns a float JSValue; Rust extracts the double. */
    const char *expr = "0.1 + 0.2";
    JSValue v = JS_Eval(ctx, expr, strlen(expr), "<t2>", JS_EVAL_TYPE_GLOBAL);
    uint64_t bits = t2_rust_reads_float(ctx, v);
    double d;
    memcpy(&d, &bits, sizeof d);
    int ok = bits == UINT64_C(0x3fd3333333333334);
    bad += !ok;
    printf("%-4s Rust->C  %-62s = %.17g bits=0x%016llx (expect 0x3fd3333333333334)\n",
           ok ? "OK" : "BAD", expr, d, (unsigned long long)bits);
    JS_FreeValue(ctx, v);

    JS_FreeContext(ctx);
    JS_FreeRuntime(rt);
    printf("t2-quickjs-float-repro: %d of %zu cases wrong\n", bad,
           sizeof CASES / sizeof CASES[0] + 1);
    return bad != 0;
}
