/* Cellos freestanding math declarations. The implementations are Rust `libm`
 * re-exported as C symbols by api::services::posix::math — never link -lm.
 *
 * These are real declarations, not macros: the engine's `Math` table takes the
 * addresses of `fabs`, `floor`, `ceil`, `sqrt`, `trunc`, … as function
 * pointers, which a macro definition would break. Only the classification
 * helpers stay macros, because a normal libc defines them as macros too and the
 * engine only calls them in expressions. */
#ifndef QJS_VIOS_MATH_H
#define QJS_VIOS_MATH_H

#define INFINITY (__builtin_inff())
#define NAN (__builtin_nanf(""))
#define HUGE_VAL (__builtin_huge_val())
#define HUGE_VALF (__builtin_huge_valf())

#define isnan(x) __builtin_isnan(x)
#define isinf(x) __builtin_isinf_sign(x)
#define isfinite(x) __builtin_isfinite(x)
#define signbit(x) __builtin_signbit(x)

double fabs(double x);
double floor(double x);
double ceil(double x);
double round(double x);
double trunc(double x);
double sqrt(double x);
double cbrt(double x);
double fmod(double x, double y);
double hypot(double x, double y);
double fmin(double x, double y);
double fmax(double x, double y);
double copysign(double x, double y);
double lrint(double x);
double modf(double x, double *iptr);
double pow(double x, double y);
double acos(double x);
double acosh(double x);
double asin(double x);
double asinh(double x);
double atan(double x);
double atan2(double y, double x);
double atanh(double x);
double cos(double x);
double cosh(double x);
double exp(double x);
double expm1(double x);
double log(double x);
double log10(double x);
double log1p(double x);
double log2(double x);
double sin(double x);
double sinh(double x);
double tan(double x);
double tanh(double x);

#endif
