/* Cellos freestanding math for the vendored engine — never link -lm.
 *
 * No `double` crosses between this C and the cell's Rust (rule H1,
 * docs/guides/tier1b-c-zig.md "x86_64 C and the float boundary"): x86_64 cells
 * compile this C hard-float SSE2 while their Rust is soft-float, so a `double`
 * argument would arrive in the wrong registers. Each function the engine calls
 * is a `static inline` C wrapper here that passes the IEEE-754 bit pattern to
 * the cell's integer-only `cellos_qjs_<name>_bits` export (src/float_abi.rs,
 * over Rust `libm`) and reinterprets the bits it gets back. The same path runs
 * on every architecture, so every architecture computes the same result.
 *
 * The standard names are object-like macros naming those wrappers: the
 * engine's `Math` table takes the addresses of `fabs`, `floor`, `sqrt`, …,
 * which a function-like macro would break. Only the classification helpers are
 * function-like, because a normal libc defines them as macros too and the
 * engine only calls them in expressions.
 *
 * Layout: every boundary declaration comes before the first wrapper body, so
 * scripts/check-float-ffi-boundary.py reads exactly the crossing prototypes.
 */
#ifndef QJS_VIOS_MATH_H
#define QJS_VIOS_MATH_H

#include <stdint.h>

#define INFINITY (__builtin_inff())
#define NAN (__builtin_nanf(""))
#define HUGE_VAL (__builtin_huge_val())
#define HUGE_VALF (__builtin_huge_valf())

#define isnan(x) __builtin_isnan(x)
#define isinf(x) __builtin_isinf_sign(x)
#define isfinite(x) __builtin_isfinite(x)
#define signbit(x) __builtin_signbit(x)

/* The boundary: implemented in src/float_abi.rs. */
uint64_t cellos_qjs_fabs_bits(uint64_t x);
uint64_t cellos_qjs_floor_bits(uint64_t x);
uint64_t cellos_qjs_ceil_bits(uint64_t x);
uint64_t cellos_qjs_round_bits(uint64_t x);
uint64_t cellos_qjs_trunc_bits(uint64_t x);
uint64_t cellos_qjs_sqrt_bits(uint64_t x);
uint64_t cellos_qjs_cbrt_bits(uint64_t x);
uint64_t cellos_qjs_fmod_bits(uint64_t x, uint64_t y);
uint64_t cellos_qjs_hypot_bits(uint64_t x, uint64_t y);
uint64_t cellos_qjs_fmin_bits(uint64_t x, uint64_t y);
uint64_t cellos_qjs_fmax_bits(uint64_t x, uint64_t y);
uint64_t cellos_qjs_copysign_bits(uint64_t x, uint64_t y);
int64_t cellos_qjs_lrint_bits(uint64_t x);
uint64_t cellos_qjs_modf_bits(uint64_t x);
uint64_t cellos_qjs_pow_bits(uint64_t x, uint64_t y);
uint64_t cellos_qjs_acos_bits(uint64_t x);
uint64_t cellos_qjs_acosh_bits(uint64_t x);
uint64_t cellos_qjs_asin_bits(uint64_t x);
uint64_t cellos_qjs_asinh_bits(uint64_t x);
uint64_t cellos_qjs_atan_bits(uint64_t x);
uint64_t cellos_qjs_atan2_bits(uint64_t y, uint64_t x);
uint64_t cellos_qjs_atanh_bits(uint64_t x);
uint64_t cellos_qjs_cos_bits(uint64_t x);
uint64_t cellos_qjs_cosh_bits(uint64_t x);
uint64_t cellos_qjs_exp_bits(uint64_t x);
uint64_t cellos_qjs_expm1_bits(uint64_t x);
uint64_t cellos_qjs_log_bits(uint64_t x);
uint64_t cellos_qjs_log10_bits(uint64_t x);
uint64_t cellos_qjs_log1p_bits(uint64_t x);
uint64_t cellos_qjs_log2_bits(uint64_t x);
uint64_t cellos_qjs_sin_bits(uint64_t x);
uint64_t cellos_qjs_sinh_bits(uint64_t x);
uint64_t cellos_qjs_tan_bits(uint64_t x);
uint64_t cellos_qjs_tanh_bits(uint64_t x);

/* C-only from here on: the wrappers never leave this translation unit. */
static inline uint64_t cellos_qjs_u64(double x) { union { double d; uint64_t u; } v; v.d = x; return v.u; }
static inline double cellos_qjs_f64(uint64_t x) { union { double d; uint64_t u; } v; v.u = x; return v.d; }

#define CELLOS_QJS_1(n) cellos_qjs_f64(cellos_qjs_##n##_bits(cellos_qjs_u64(x)))
#define CELLOS_QJS_2(n) cellos_qjs_f64(cellos_qjs_##n##_bits(cellos_qjs_u64(x), cellos_qjs_u64(y)))

static inline double cellos_qjs_fabs(double x) { return CELLOS_QJS_1(fabs); }
static inline double cellos_qjs_floor(double x) { return CELLOS_QJS_1(floor); }
static inline double cellos_qjs_ceil(double x) { return CELLOS_QJS_1(ceil); }
static inline double cellos_qjs_round(double x) { return CELLOS_QJS_1(round); }
static inline double cellos_qjs_trunc(double x) { return CELLOS_QJS_1(trunc); }
static inline double cellos_qjs_sqrt(double x) { return CELLOS_QJS_1(sqrt); }
static inline double cellos_qjs_cbrt(double x) { return CELLOS_QJS_1(cbrt); }
static inline double cellos_qjs_fmod(double x, double y) { return CELLOS_QJS_2(fmod); }
static inline double cellos_qjs_hypot(double x, double y) { return CELLOS_QJS_2(hypot); }
static inline double cellos_qjs_fmin(double x, double y) { return CELLOS_QJS_2(fmin); }
static inline double cellos_qjs_fmax(double x, double y) { return CELLOS_QJS_2(fmax); }
static inline double cellos_qjs_copysign(double x, double y) { return CELLOS_QJS_2(copysign); }
static inline long cellos_qjs_lrint(double x) { return (long)cellos_qjs_lrint_bits(cellos_qjs_u64(x)); }
static inline double cellos_qjs_modf(double x, double *iptr) { *iptr = cellos_qjs_trunc(x); return CELLOS_QJS_1(modf); }
static inline double cellos_qjs_pow(double x, double y) { return CELLOS_QJS_2(pow); }
static inline double cellos_qjs_acos(double x) { return CELLOS_QJS_1(acos); }
static inline double cellos_qjs_acosh(double x) { return CELLOS_QJS_1(acosh); }
static inline double cellos_qjs_asin(double x) { return CELLOS_QJS_1(asin); }
static inline double cellos_qjs_asinh(double x) { return CELLOS_QJS_1(asinh); }
static inline double cellos_qjs_atan(double x) { return CELLOS_QJS_1(atan); }
static inline double cellos_qjs_atan2(double x, double y) { return CELLOS_QJS_2(atan2); }
static inline double cellos_qjs_atanh(double x) { return CELLOS_QJS_1(atanh); }
static inline double cellos_qjs_cos(double x) { return CELLOS_QJS_1(cos); }
static inline double cellos_qjs_cosh(double x) { return CELLOS_QJS_1(cosh); }
static inline double cellos_qjs_exp(double x) { return CELLOS_QJS_1(exp); }
static inline double cellos_qjs_expm1(double x) { return CELLOS_QJS_1(expm1); }
static inline double cellos_qjs_log(double x) { return CELLOS_QJS_1(log); }
static inline double cellos_qjs_log10(double x) { return CELLOS_QJS_1(log10); }
static inline double cellos_qjs_log1p(double x) { return CELLOS_QJS_1(log1p); }
static inline double cellos_qjs_log2(double x) { return CELLOS_QJS_1(log2); }
static inline double cellos_qjs_sin(double x) { return CELLOS_QJS_1(sin); }
static inline double cellos_qjs_sinh(double x) { return CELLOS_QJS_1(sinh); }
static inline double cellos_qjs_tan(double x) { return CELLOS_QJS_1(tan); }
static inline double cellos_qjs_tanh(double x) { return CELLOS_QJS_1(tanh); }

#undef CELLOS_QJS_1
#undef CELLOS_QJS_2

#define fabs cellos_qjs_fabs
#define floor cellos_qjs_floor
#define ceil cellos_qjs_ceil
#define round cellos_qjs_round
#define trunc cellos_qjs_trunc
#define sqrt cellos_qjs_sqrt
#define cbrt cellos_qjs_cbrt
#define fmod cellos_qjs_fmod
#define hypot cellos_qjs_hypot
#define fmin cellos_qjs_fmin
#define fmax cellos_qjs_fmax
#define copysign cellos_qjs_copysign
#define lrint cellos_qjs_lrint
#define modf cellos_qjs_modf
#define pow cellos_qjs_pow
#define acos cellos_qjs_acos
#define acosh cellos_qjs_acosh
#define asin cellos_qjs_asin
#define asinh cellos_qjs_asinh
#define atan cellos_qjs_atan
#define atan2 cellos_qjs_atan2
#define atanh cellos_qjs_atanh
#define cos cellos_qjs_cos
#define cosh cellos_qjs_cosh
#define exp cellos_qjs_exp
#define expm1 cellos_qjs_expm1
#define log cellos_qjs_log
#define log10 cellos_qjs_log10
#define log1p cellos_qjs_log1p
#define log2 cellos_qjs_log2
#define sin cellos_qjs_sin
#define sinh cellos_qjs_sinh
#define tan cellos_qjs_tan
#define tanh cellos_qjs_tanh

#endif
