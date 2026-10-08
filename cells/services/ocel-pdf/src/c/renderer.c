/* SPDX-License-Identifier: AGPL-3.0-or-later */
#include "mupdf/fitz.h"
#include "mupdf/pdf.h"
#include <stdlib.h>
#include <string.h>
#include <math.h>

#define PDF_BYTES (2u * 1024u * 1024u)
#define PAGE_PIXELS (1024u * 1024u)
#define ALLOCATION_BYTES (10u * 1024u * 1024u)

typedef union { size_t size; max_align_t align; } allocation_header;
/* Include our header, POSIX malloc metadata and worst-case 16-byte rounding. */
#define ALLOCATION_OVERHEAD (sizeof(allocation_header) + 32u)
typedef struct {
    fz_context *ctx;
    pdf_document *doc;
    size_t allocated;
    fz_pixmap *raster;
    unsigned char *pixels;
    size_t pixel_bytes;
    unsigned int width, height, pages;
} cell_pdf;

static void *bounded_malloc(void *user, size_t size)
{
    cell_pdf *pdf = user;
    if (size > SIZE_MAX - ALLOCATION_OVERHEAD) return NULL;
    size_t charged = size + ALLOCATION_OVERHEAD;
    if (charged > ALLOCATION_BYTES - pdf->allocated) return NULL;
    allocation_header *h = malloc(size + sizeof(*h));
    if (!h) return NULL;
    h->size = charged;
    pdf->allocated += charged;
    return h + 1;
}
static void bounded_free(void *user, void *ptr)
{
    if (!ptr) return;
    cell_pdf *pdf = user;
    allocation_header *h = (allocation_header *)ptr - 1;
    pdf->allocated -= h->size;
    free(h);
}
static void *bounded_realloc(void *user, void *ptr, size_t size)
{
    if (!ptr) return bounded_malloc(user, size);
    if (!size) { bounded_free(user, ptr); return NULL; }
    cell_pdf *pdf = user;
    allocation_header *h = (allocation_header *)ptr - 1;
    size_t old = h->size;
    if (size > SIZE_MAX - ALLOCATION_OVERHEAD) return NULL;
    size_t charged = size + ALLOCATION_OVERHEAD;
    if (charged > ALLOCATION_BYTES - (pdf->allocated - old)) return NULL;
    allocation_header *next = realloc(h, size + sizeof(*h));
    if (!next) return NULL;
    next->size = charged;
    pdf->allocated = pdf->allocated - old + charged;
    return next + 1;
}
static void error_text(char *error, size_t capacity, const char *message)
{
    if (capacity) { strncpy(error, message, capacity - 1); error[capacity - 1] = 0; }
}

/* No Rust callback can throw. Every try/always/catch below begins and ends
 * inside this translation unit; longjmp can never unwind a Rust frame. */
void ocel_pdf_close(cell_pdf *pdf)
{
    if (!pdf) return;
    fz_drop_pixmap(pdf->ctx, pdf->raster);
    pdf_drop_document(pdf->ctx, pdf->doc);
    fz_drop_context(pdf->ctx);
    free(pdf);
}

/* The caller retains bytes until AFTER ocel_pdf_close: fz_open_memory does not
 * copy its input, and pdf_document retains the underlying stream. */
cell_pdf *ocel_pdf_open(const unsigned char *bytes, size_t length,
    unsigned int *pages, char *error, size_t capacity)
{
    if (!bytes || !length || length > PDF_BYTES) {
        error_text(error, capacity, "PDF input size exceeds limit"); return NULL;
    }
    cell_pdf *pdf = calloc(1, sizeof(*pdf));
    if (!pdf) { error_text(error, capacity, "PDF allocation failed"); return NULL; }
    fz_alloc_context allocator = {pdf, bounded_malloc, bounded_realloc, bounded_free};
    pdf->ctx = fz_new_context(&allocator, NULL, 2u * 1024u * 1024u);
    if (!pdf->ctx) { free(pdf); error_text(error, capacity, "MuPDF context allocation failed"); return NULL; }
    fz_stream *stream = NULL;
    int failed = 0;
    fz_var(stream);
    fz_var(failed);
    fz_try(pdf->ctx) {
        stream = fz_open_memory(pdf->ctx, bytes, length);
        pdf->doc = pdf_open_document_with_stream(pdf->ctx, stream);
        if (pdf_needs_password(pdf->ctx, pdf->doc))
            fz_throw(pdf->ctx, FZ_ERROR_ARGUMENT, "PDF requires a password");
        int count = pdf_count_pages(pdf->ctx, pdf->doc);
        if (count <= 0) fz_throw(pdf->ctx, FZ_ERROR_ARGUMENT, "PDF has no pages");
        pdf->pages = (unsigned int)count;
    }
    fz_always(pdf->ctx) { fz_drop_stream(pdf->ctx, stream); }
    fz_catch(pdf->ctx) { error_text(error, capacity, fz_caught_message(pdf->ctx)); failed = 1; }
    if (failed) { ocel_pdf_close(pdf); return NULL; }
    *pages = pdf->pages;
    return pdf;
}

int ocel_pdf_render(cell_pdf *pdf, unsigned int number, unsigned int max_width,
    unsigned int max_height, unsigned int *width, unsigned int *height,
    unsigned int *bytes, char *error, size_t capacity)
{
    if (!pdf || number >= pdf->pages || !max_width || !max_height ||
        max_width > PAGE_PIXELS || max_height > PAGE_PIXELS) {
        error_text(error, capacity, "Invalid PDF page or dimensions"); return -1;
    }
    /* A failed render invalidates the old cache, never exposes stale pixels. */
    fz_drop_pixmap(pdf->ctx, pdf->raster);
    pdf->raster = NULL;
    pdf->pixels = NULL; pdf->pixel_bytes = 0; pdf->width = pdf->height = 0;
    pdf_page *page = NULL;
    fz_pixmap *pixmap = NULL;
    fz_device *device = NULL;
    int failed = 0;
    fz_var(page); fz_var(pixmap); fz_var(device); fz_var(failed);
    fz_try(pdf->ctx) {
        page = pdf_load_page(pdf->ctx, pdf->doc, (int)number);
        fz_rect bounds = pdf_bound_page(pdf->ctx, page, FZ_CROP_BOX);
        double w = (double)bounds.x1 - bounds.x0, h = (double)bounds.y1 - bounds.y0;
        if (!isfinite(w) || !isfinite(h) || w <= 0 || h <= 0)
            fz_throw(pdf->ctx, FZ_ERROR_ARGUMENT, "Invalid PDF page bounds");
        double scale = fmin((double)max_width / w, (double)max_height / h);
        scale = fmin(scale, sqrt((double)PAGE_PIXELS / (w * h)));
        if (!isfinite(scale) || scale <= 0)
            fz_throw(pdf->ctx, FZ_ERROR_ARGUMENT, "Invalid PDF page scale");
        unsigned int pw = (unsigned int)fmin(ceil(w * scale), max_width);
        unsigned int ph = (unsigned int)fmin(ceil(h * scale), max_height);
        if (!pw) pw = 1;
        if (!ph) ph = 1;
        /* Ceil can add one row beyond the area limit; scale down before drawing. */
        if ((size_t)pw * ph > PAGE_PIXELS) {
            double adjust = sqrt((double)PAGE_PIXELS / ((double)pw * ph));
            scale *= adjust;
            pw = (unsigned int)fmax(1, floor(w * scale));
            ph = (unsigned int)fmax(1, floor(h * scale));
        }
        if ((size_t)pw * ph > PAGE_PIXELS)
            fz_throw(pdf->ctx, FZ_ERROR_LIMIT, "PDF raster exceeds pixel limit");
        fz_matrix matrix = fz_pre_translate(fz_scale((float)scale, (float)scale), -bounds.x0, -bounds.y0);
        pixmap = fz_new_pixmap_with_bbox(pdf->ctx, fz_device_rgb(pdf->ctx),
            fz_make_irect(0, 0, (int)pw, (int)ph), NULL, 1);
        fz_clear_pixmap_with_value(pdf->ctx, pixmap, 255);
        device = fz_new_draw_device(pdf->ctx, matrix, pixmap);
        fz_run_page(pdf->ctx, (fz_page *)page, device, fz_identity, NULL);
        fz_close_device(pdf->ctx, device);
        fz_drop_device(pdf->ctx, device);
        device = NULL;
        size_t size = (size_t)pw * ph * 4;
        unsigned char *rgba = fz_pixmap_samples(pdf->ctx, pixmap);
        int stride = fz_pixmap_stride(pdf->ctx, pixmap);
        if (fz_pixmap_components(pdf->ctx, pixmap) != 4 || stride != (int)(pw * 4))
            fz_throw(pdf->ctx, FZ_ERROR_ARGUMENT, "MuPDF returned non-packed RGBA raster");
        /* Rendering onto opaque white keeps alpha=255, so premultiplied and
         * straight RGB are identical. Swap channels in-place: no second full
         * image allocation/copy. After this point the pixmap is only a byte
         * cache and may only be dropped, never drawn or colorspace-converted. */
        for (size_t i = 0; i < size; i += 4) {
            if (rgba[i + 3] != 255)
                fz_throw(pdf->ctx, FZ_ERROR_ARGUMENT, "MuPDF returned non-opaque raster");
            unsigned char red = rgba[i];
            rgba[i] = rgba[i + 2];
            rgba[i + 2] = red;
        }
        pdf->raster = pixmap; pixmap = NULL;
        pdf->pixels = rgba;
        pdf->pixel_bytes = size; pdf->width = pw; pdf->height = ph;
    }
    fz_always(pdf->ctx) {
        fz_drop_device(pdf->ctx, device);
        fz_drop_pixmap(pdf->ctx, pixmap);
        fz_drop_page(pdf->ctx, (fz_page *)page);
    }
    fz_catch(pdf->ctx) { error_text(error, capacity, fz_caught_message(pdf->ctx)); failed = 1; }
    if (failed) return -1;
    *width = pdf->width; *height = pdf->height; *bytes = (unsigned int)pdf->pixel_bytes;
    return 0;
}

int ocel_pdf_read_pixels(cell_pdf *pdf, size_t offset, unsigned char *bytes, size_t length)
{
    if (!pdf || !pdf->pixels || !bytes || length > 3072 || offset > pdf->pixel_bytes ||
        length > pdf->pixel_bytes - offset) return -1;
    memcpy(bytes, pdf->pixels + offset, length);
    return 0;
}
