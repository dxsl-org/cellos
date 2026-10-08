/* SPDX-License-Identifier: AGPL-3.0-or-later */
#ifndef OCEL_PDF_SETJMP_H
#define OCEL_PDF_SETJMP_H
#include <stdint.h>
typedef uint64_t jmp_buf[26];
int ocel_pdf_setjmp(jmp_buf) __attribute__((returns_twice));
void ocel_pdf_longjmp(jmp_buf, int) __attribute__((noreturn));
#define setjmp ocel_pdf_setjmp
#define longjmp ocel_pdf_longjmp
#endif
