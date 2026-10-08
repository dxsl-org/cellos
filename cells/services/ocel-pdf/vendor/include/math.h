/* SPDX-License-Identifier: AGPL-3.0-or-later */
#ifndef OCEL_PDF_MATH_H
#define OCEL_PDF_MATH_H
#define INFINITY (__builtin_inff())
#define NAN (__builtin_nanf(""))
#define HUGE_VAL (__builtin_huge_val())
#define HUGE_VALF (__builtin_huge_valf())
#define M_PI 3.14159265358979323846
#define isnan(x) __builtin_isnan(x)
#define isinf(x) __builtin_isinf(x)
#define isfinite(x) __builtin_isfinite(x)
#define signbit(x) __builtin_signbit(x)
#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4
#define fpclassify(x) __builtin_fpclassify(FP_NAN,FP_INFINITE,FP_NORMAL,FP_SUBNORMAL,FP_ZERO,(x))
#define lrintf ocel_pdf_lrintf
long lrintf(float);
double sin(double); float sinf(float);
double cos(double); float cosf(float);
double tan(double); float tanf(float);
double asin(double); float asinf(float);
double acos(double); float acosf(float);
double atan(double); float atanf(float);
double sinh(double); float sinhf(float);
double cosh(double); float coshf(float);
double tanh(double); float tanhf(float);
double exp(double); float expf(float);
double exp2(double); float exp2f(float);
double log(double); float logf(float);
double log2(double); float log2f(float);
double log10(double); float log10f(float);
double sqrt(double); float sqrtf(float);
double floor(double); float floorf(float);
double ceil(double); float ceilf(float);
double round(double); float roundf(float);
double trunc(double); float truncf(float);
double fabs(double); float fabsf(float);
double rint(double); float rintf(float);
double atan2(double,double); float atan2f(float,float);
double pow(double,double); float powf(float,float);
double hypot(double,double); float hypotf(float,float);
double fmod(double,double); float fmodf(float,float);
double fmin(double,double); float fminf(float,float);
double fmax(double,double); float fmaxf(float,float);
double copysign(double,double); float copysignf(float,float);
double frexp(double,int*); double ldexp(double,int); float frexpf(float,int*); float ldexpf(float,int);
#endif
