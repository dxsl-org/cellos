/* Cellos freestanding assert. The engine is compiled with -DNDEBUG (release
 * cell), so assertions vanish; the include is kept so upstream sources that
 * `#include <assert.h>` compile unchanged. */
#ifndef QJS_VIOS_ASSERT_H
#define QJS_VIOS_ASSERT_H

#ifdef NDEBUG
#define assert(expr) ((void)0)
#else
void __assert_fail(const char *expr, const char *file, unsigned line, const char *func)
    __attribute__((noreturn));
#define assert(expr) ((expr) ? (void)0 : __assert_fail(#expr, __FILE__, __LINE__, __func__))
#endif

#endif
