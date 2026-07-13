/* Minimal freestanding <inttypes.h> — PRI* format macros over <stdint.h>.
 *
 * clang's builtin inttypes.h does `#include_next <inttypes.h>` expecting a
 * libc copy; bare-metal targets have none, so this shim terminates the chain.
 * Only the printf/scanf FORMAT MACROS are provided (what vendored C like
 * littlefs actually uses) — no imaxdiv/strtoimax (freestanding, no libc).
 */
#ifndef _FREESTANDING_INTTYPES_H
#define _FREESTANDING_INTTYPES_H

#include <stdint.h> /* clang builtin — fixed-width types */

/* 64-bit ints are `long` on LP64 bare-metal targets (aarch64/x86_64-none). */
#if defined(__LP64__) || defined(_LP64)
#define __PRI64_PREFIX "l"
#else
#define __PRI64_PREFIX "ll"
#endif

#define PRId8  "d"
#define PRId16 "d"
#define PRId32 "d"
#define PRId64 __PRI64_PREFIX "d"

#define PRIi8  "i"
#define PRIi16 "i"
#define PRIi32 "i"
#define PRIi64 __PRI64_PREFIX "i"

#define PRIu8  "u"
#define PRIu16 "u"
#define PRIu32 "u"
#define PRIu64 __PRI64_PREFIX "u"

#define PRIx8  "x"
#define PRIx16 "x"
#define PRIx32 "x"
#define PRIx64 __PRI64_PREFIX "x"

#define PRIX8  "X"
#define PRIX16 "X"
#define PRIX32 "X"
#define PRIX64 __PRI64_PREFIX "X"

#define PRIdPTR __PRI64_PREFIX "d"
#define PRIuPTR __PRI64_PREFIX "u"
#define PRIxPTR __PRI64_PREFIX "x"

#endif /* _FREESTANDING_INTTYPES_H */
