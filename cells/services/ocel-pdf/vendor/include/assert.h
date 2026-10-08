/* SPDX-License-Identifier: AGPL-3.0-or-later */
#ifndef OCEL_PDF_ASSERT_H
#define OCEL_PDF_ASSERT_H
#include <stdlib.h>
#ifdef NDEBUG
#define assert(x) ((void)0)
#else
#define assert(x) ((x)?(void)0:abort())
#endif
#endif
