/* SPDX-License-Identifier: AGPL-3.0-or-later */
#ifndef OCEL_PDF_CONFIG_H
#define OCEL_PDF_CONFIG_H
#define FZ_ENABLE_PDF 1
#define FZ_ENABLE_XPS 0
#define FZ_ENABLE_SVG 0
#define FZ_ENABLE_CBZ 0
#define FZ_ENABLE_IMG 0
#define FZ_ENABLE_HTML 0
#define FZ_ENABLE_HTML_ENGINE 0
#define FZ_ENABLE_FB2 0
#define FZ_ENABLE_MOBI 0
#define FZ_ENABLE_EPUB 0
#define FZ_ENABLE_OFFICE 0
#define FZ_ENABLE_TXT 0
#define FZ_ENABLE_JS 0
#define FZ_ENABLE_BROTLI 0
#define FZ_ENABLE_BARCODE 0
#define FZ_ENABLE_OCR_OUTPUT 0
#define FZ_ENABLE_DOCX_OUTPUT 0
#define FZ_ENABLE_ODT_OUTPUT 0
#define FZ_ENABLE_JPX 1
#define FZ_ENABLE_ICC 1
#define HAVE_SIGSETJMP 0
#define TOFU
/* The VIFS1 loader cannot stage multi-megabyte fallback font payloads.
 * Embedded CJK fonts still render; non-embedded CJK substitution is absent. */
#define TOFU_CJK
#define NDEBUG
#define HAVE_STDINT_H
#define HAVE_INTTYPES_H
#define HAVE_STDARG_H
#define OPJ_STATIC
#define OPJ_HAVE_STDINT_H
#define OPJ_HAVE_INTTYPES_H
#define MUTEX_pthread 0
#define HAVE_LCMS2MT
#define LCMS2MT_PREFIX lcms2mt_
#define JBIG_EXTERNAL_MEMENTO_H "mupdf/memento.h"
#define FT_CONFIG_MODULES_H "slimftmodules.h"
#define CMS_NO_PTHREADS
#define FT_CONFIG_OPTIONS_H "slimftoptions.h"
/* PDF Flate streams use fz_zlib_alloc/free; no hosted gzip-file API is needed. */
#define Z_SOLO
/* Cells have no process environment; use IJG's documented switch to omit
 * JPEGMEM parsing (and its hosted sscanf dependency), not a fake parser. */
#define NO_GETENV
#define FT2_BUILD_LIBRARY
#endif
