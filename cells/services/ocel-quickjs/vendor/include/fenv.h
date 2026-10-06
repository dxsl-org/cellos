/* Cellos freestanding fenv.h. The engine only needs the names to exist; the
 * default rounding mode is what the hardware already uses. */
#ifndef QJS_VIOS_FENV_H
#define QJS_VIOS_FENV_H

#define FE_TONEAREST 0
#define FE_DOWNWARD 1
#define FE_UPWARD 2
#define FE_TOWARDZERO 3

int fegetround(void);
int fesetround(int mode);

#endif
