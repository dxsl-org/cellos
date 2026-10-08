/* SPDX-License-Identifier: AGPL-3.0-or-later */
#ifndef OCEL_PDF_UNISTD_H
#define OCEL_PDF_UNISTD_H
#include "cellos-libc.h"
int unlink(const char *);
int ftruncate(int, int64_t);
int close(int);
ssize_t read(int, void *, size_t);
ssize_t write(int, const void *, size_t);
int access(const char *, int);
#define F_OK 0
#endif
