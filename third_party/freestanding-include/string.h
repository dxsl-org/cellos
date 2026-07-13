/* Minimal freestanding <string.h> — DECLARATIONS ONLY.
 *
 * Used when cross-compiling vendored C (littlefs) with clang for bare-metal
 * targets that ship no libc headers (aarch64-unknown-none, x86_64-unknown-none).
 * riscv builds don't need this: riscv-none-elf-gcc bundles newlib headers.
 *
 * The IMPLEMENTATIONS come from the Rust side of the cell binary:
 *   - mem*  : rust compiler_builtins
 *   - str*  : libs/api/src/services/posix/strings.rs (POSIX shim, -zmuldefs)
 * so this header must only declare, never define.
 */
#ifndef _FREESTANDING_STRING_H
#define _FREESTANDING_STRING_H

#include <stddef.h> /* size_t — provided by clang's builtin headers */

#ifdef __cplusplus
extern "C" {
#endif

void  *memcpy(void *dest, const void *src, size_t n);
void  *memmove(void *dest, const void *src, size_t n);
void  *memset(void *s, int c, size_t n);
int    memcmp(const void *s1, const void *s2, size_t n);
void  *memchr(const void *s, int c, size_t n);

size_t strlen(const char *s);
int    strcmp(const char *s1, const char *s2);
int    strncmp(const char *s1, const char *s2, size_t n);
char  *strcpy(char *dest, const char *src);
char  *strncpy(char *dest, const char *src, size_t n);
char  *strchr(const char *s, int c);
char  *strrchr(const char *s, int c);
size_t strspn(const char *s, const char *accept);
size_t strcspn(const char *s, const char *reject);
char  *strcat(char *dest, const char *src);
char  *strstr(const char *haystack, const char *needle);

#ifdef __cplusplus
}
#endif

#endif /* _FREESTANDING_STRING_H */
