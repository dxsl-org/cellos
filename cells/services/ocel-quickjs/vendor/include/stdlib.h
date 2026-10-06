/* Cellos freestanding declarations for the C ABI that libs/api exports. */
#ifndef QJS_VIOS_STDLIB_H
#define QJS_VIOS_STDLIB_H

#include <stddef.h>

void *malloc(size_t size);
void free(void *ptr);
void *realloc(void *ptr, size_t size);
void *calloc(size_t nmemb, size_t size);
void abort(void) __attribute__((noreturn));

/* Upstream calls `abs` as a function (dtoa.c); the cell's C glue defines it,
 * because the Tier-A C ABI exports the libm-style math set but not this. */
int abs(int v);
long labs(long v);

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1

/* A normal libc header spells this as the compiler builtin; the engine's
 * parser uses `alloca` for small temporary buffers. */
#ifndef alloca
#define alloca(size) __builtin_alloca(size)
#endif

#endif
