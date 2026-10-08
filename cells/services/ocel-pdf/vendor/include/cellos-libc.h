/* SPDX-License-Identifier: AGPL-3.0-or-later
 * Owned declarations for the cell's LP64 C ABI; no hosted libc headers. */
#ifndef OCEL_PDF_LIBC_H
#define OCEL_PDF_LIBC_H
#include <stddef.h>
#include <stdint.h>
#include <stdarg.h>
#define EOF (-1)
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define RAND_MAX 2147483647
#define BUFSIZ 4096
#define L_tmpnam 64
#define INT_MAX 2147483647
#define INT_MIN (-INT_MAX-1)
#define UINT_MAX 4294967295U
#define LONG_MAX 9223372036854775807L
#define LONG_MIN (-LONG_MAX-1)
#define ULONG_MAX 18446744073709551615UL
#define LLONG_MAX LONG_MAX
#define LLONG_MIN LONG_MIN
#define ULLONG_MAX ULONG_MAX
#define CHAR_BIT 8
#define UCHAR_MAX 255
#define CHAR_MAX 127
#define CHAR_MIN (-128)
#define SHRT_MAX 32767
#define SHRT_MIN (-32768)
#define USHRT_MAX 65535
#define PATH_MAX 4096
#define ERANGE 34
#define EINVAL 22
#define ENOMEM 12
#define ENOENT 2
#define EIO 5
#define EACCES 13
#define EINTR 4
#define EEXIST 17
#define ENOSYS 38
extern int ocel_pdf_errno;
#define errno ocel_pdf_errno
/* Prefix missing libc facilities, so no incompatible global shims are exported. */
#define qsort ocel_pdf_qsort
#define bsearch ocel_pdf_bsearch
#define strtol ocel_pdf_strtol
#define strtoul ocel_pdf_strtoul
#define strtoll ocel_pdf_strtoll
#define strtoull ocel_pdf_strtoull
#define atoi ocel_pdf_atoi
#define atol ocel_pdf_atol
#define atoll ocel_pdf_atoll
#define strerror ocel_pdf_strerror
#define strstr ocel_pdf_strstr
#define strpbrk ocel_pdf_strpbrk
#define strspn ocel_pdf_strspn
#define strcspn ocel_pdf_strcspn
#define strchrnul ocel_pdf_strchrnul
#define strdup ocel_pdf_strdup
#define abs ocel_pdf_abs
#define labs ocel_pdf_labs
#define llabs ocel_pdf_llabs
void *malloc(size_t);
void *calloc(size_t, size_t);
void *realloc(void *, size_t);
void free(void *);
void abort(void) __attribute__((noreturn));
void *memcpy(void *, const void *, size_t);
void *memmove(void *, const void *, size_t);
void *memset(void *, int, size_t);
int memcmp(const void *, const void *, size_t);
void *memchr(const void *, int, size_t);
size_t strlen(const char *);
char *strcpy(char *, const char *);
char *strncpy(char *, const char *, size_t);
char *strcat(char *, const char *);
int strcmp(const char *, const char *);
int strncmp(const char *, const char *, size_t);
char *strchr(const char *, int);
char *strrchr(const char *, int);
char *strstr(const char *, const char *);
char *strpbrk(const char *, const char *);
char *strchrnul(const char *, int);
size_t strspn(const char *, const char *);
size_t strcspn(const char *, const char *);
char *strdup(const char *);
char *strerror(int);
long strtol(const char *, char **, int);
unsigned long strtoul(const char *, char **, int);
long long strtoll(const char *, char **, int);
unsigned long long strtoull(const char *, char **, int);
int atoi(const char *);
long atol(const char *);
long long atoll(const char *);
int abs(int);
long labs(long);
long long llabs(long long);
void qsort(void *, size_t, size_t, int (*)(const void *, const void *));
void *bsearch(const void *, const void *, size_t, size_t, int (*)(const void *, const void *));
/* FILE is opaque and owned by api::services::posix. */
typedef struct ocel_pdf_FILE FILE;
extern FILE *stdin, *stdout, *stderr;
FILE *fopen(const char *, const char *);
int fclose(FILE *);
size_t fread(void *, size_t, size_t, FILE *);
size_t fwrite(const void *, size_t, size_t, FILE *);
int fputs(const char *, FILE *);
int fputc(int, FILE *);
int fgetc(FILE *);
char *fgets(char *, int, FILE *);
int puts(const char *);
int putchar(int);
int fprintf(FILE *, const char *, ...);
int printf(const char *, ...);
int sprintf(char *, const char *, ...);
int snprintf(char *, size_t, const char *, ...);
int vsprintf(char *, const char *, va_list);
int vsnprintf(char *, size_t, const char *, va_list);
int vfprintf(FILE *, const char *, va_list);
int fflush(FILE *);
int ferror(FILE *);
int feof(FILE *);
void clearerr(FILE *);
int fseek(FILE *, long, int);
long ftell(FILE *);
int fseeko(FILE *, int64_t, int);
int64_t ftello(FILE *);
int fileno(FILE *);
/* This passive renderer has no file mutation authority. */
#define remove ocel_pdf_remove
int remove(const char *);
typedef long ssize_t;
typedef long off_t;
#define getenv ocel_pdf_getenv
char *getenv(const char *);
char *strncat(char *, const char *, size_t);
double atof(const char *);
int sscanf(const char *, const char *, ...);
#define _IONBF 2
#define getc fgetc
#define putc fputc
int setvbuf(FILE *, char *, int, size_t);
int mkstemp(char *);
FILE *fdopen(int, const char *);
#define exit ocel_pdf_exit
void exit(int) __attribute__((noreturn));
#endif
