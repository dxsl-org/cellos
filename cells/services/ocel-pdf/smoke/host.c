/* SPDX-License-Identifier: AGPL-3.0-or-later
 * Run against tests/fixtures/ocel-pdf-demo.pdf: real text/vector/image pages. */
#undef NDEBUG
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct cell_pdf cell_pdf;
cell_pdf *ocel_pdf_open(const unsigned char *, size_t, unsigned int *, char *, size_t);
void ocel_pdf_close(cell_pdf *);
int ocel_pdf_render(cell_pdf *, unsigned int, unsigned int, unsigned int,
    unsigned int *, unsigned int *, unsigned int *, char *, size_t);
int ocel_pdf_read_pixels(cell_pdf *, size_t, unsigned char *, size_t);
void ocel_pdf_set_clock(uint64_t (*)(void));
static uint64_t fake_ticks;
static uint64_t expiring_clock(void) { fake_ticks += 401; return fake_ticks; }
/* Reads are counted; the clock jumps past the 400-tick deadline at read `expire_at`. */
static uint64_t clock_reads, expire_at = UINT64_MAX;
static uint64_t counting_clock(void) { return ++clock_reads >= expire_at ? 1000 : 0; }

/* One page whose content is ~1.8 MiB of identity `cm` operators: no painting and
 * no allocation per operator, so only the stream checkpoints can observe time. */
static unsigned char *cm_flood_pdf(size_t *length)
{
    static const char op[] = "1 0 0 1 0 0 cm\n";
    const size_t ops = 120000, content = ops * (sizeof(op) - 1), cap = content + 1024;
    char *pdf = malloc(cap); assert(pdf);
    size_t offsets[5] = {0}, n = 0;
    n += (size_t)sprintf(pdf + n, "%%PDF-1.4\n");
    offsets[1] = n; n += (size_t)sprintf(pdf + n, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets[2] = n; n += (size_t)sprintf(pdf + n, "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    offsets[3] = n; n += (size_t)sprintf(pdf + n,
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>\nendobj\n");
    offsets[4] = n; n += (size_t)sprintf(pdf + n, "4 0 obj\n<< /Length %zu >>\nstream\n", content);
    for (size_t i = 0; i < ops; ++i) { memcpy(pdf + n, op, sizeof(op) - 1); n += sizeof(op) - 1; }
    n += (size_t)sprintf(pdf + n, "\nendstream\nendobj\n");
    size_t xref = n;
    n += (size_t)sprintf(pdf + n, "xref\n0 5\n0000000000 65535 f \n");
    for (int i = 1; i < 5; ++i) n += (size_t)sprintf(pdf + n, "%010zu 00000 n \n", offsets[i]);
    n += (size_t)sprintf(pdf + n, "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n%zu\n%%%%EOF\n", xref);
    assert(n < cap);
    *length = n;
    return (unsigned char *)pdf;
}
int main(int argc, char **argv)
{
    assert(argc == 2);
    FILE *file = fopen(argv[1], "rb"); assert(file);
    assert(fseek(file, 0, SEEK_END) == 0); long size = ftell(file); assert(size > 0);
    rewind(file);
    unsigned char *input = malloc((size_t)size); assert(input);
    assert(fread(input,1,(size_t)size,file) == (size_t)size); fclose(file);
    char error[256] = {0}; unsigned int pages = 0;
    /* This exercises MuPDF's exception stack, not an early wrapper rejection. */
    const unsigned char invalid[] = {0, 1, 2, 3};
    cell_pdf *bad = ocel_pdf_open(invalid,sizeof(invalid),&pages,error,sizeof(error));
    assert(!bad && error[0]);
    cell_pdf *doc = ocel_pdf_open(input,(size_t)size,&pages,error,sizeof(error));
    if (!doc) { fprintf(stderr,"open: %s\n",error); return 1; }
    assert(pages == 2);
    unsigned char chunk[3072]; unsigned int width,height,bytes;
    assert(ocel_pdf_read_pixels(doc,0,chunk,4) == -1);
    assert(ocel_pdf_render(doc,pages,320,240,&width,&height,&bytes,error,sizeof(error)) == -1);
    for (unsigned int page = 0; page < pages; ++page) {
        int status = ocel_pdf_render(doc,page,320,240,&width,&height,&bytes,error,sizeof(error));
        if (status) { fprintf(stderr,"render: %s\n",error); return 1; }
        assert(width == 320 && height == 240 && bytes == width*height*4);
        char raster_path[32];
        snprintf(raster_path, sizeof(raster_path), "pdf-page-%u.bgra", page);
        FILE *raster = fopen(raster_path, "wb"); assert(raster);
        unsigned int red = 0, blue = 0, black = 0, green = 0, magenta = 0;
        for (size_t offset = 0; offset < bytes;) {
            size_t length = bytes-offset < sizeof(chunk) ? bytes-offset : sizeof(chunk);
            assert(ocel_pdf_read_pixels(doc,offset,chunk,length) == 0);
            assert(fwrite(chunk, 1, length, raster) == length);
            for (size_t i = 0; i < length; i += 4) {
                assert(chunk[i+3] == 255);
                red += chunk[i+2] > 200 && chunk[i+1] < 40 && chunk[i] < 40;
                blue += chunk[i] > 200 && chunk[i+1] < 40 && chunk[i+2] < 40;
                black += chunk[i] < 80 && chunk[i+1] < 80 && chunk[i+2] < 80;
                green += chunk[i+1] > 200 && chunk[i+2] < 40 && chunk[i] < 40;
                magenta += chunk[i] > 200 && chunk[i+2] > 200 && chunk[i+1] < 40;
            }
            offset += length;
        }
        assert(fclose(raster) == 0);
        assert(black > 100);
        assert(page == 0 ? red > 100 : blue > 100);
        if (page == 1) assert(green > 100 && magenta > 100);
        assert(ocel_pdf_read_pixels(doc,bytes,chunk,1) == -1);
        printf("page %u: %ux%u, black=%u red=%u blue=%u opaque BGRA\n",page,width,height,black,red,blue);
    }
    /* Deadline failure clears the old raster and unwinds native allocations.
     * This is a cooperative checkpoint test, not a fake asynchronous cancel. */
    fake_ticks = 0; ocel_pdf_set_clock(expiring_clock);
    assert(ocel_pdf_render(doc,0,320,240,&width,&height,&bytes,error,sizeof(error)) == -1);
    assert(ocel_pdf_read_pixels(doc,0,chunk,4) == -1);
    ocel_pdf_close(doc);
    fake_ticks = 0;
    assert(ocel_pdf_open(input,(size_t)size,&pages,error,sizeof(error)) == NULL);
    ocel_pdf_set_clock(NULL);
    doc = ocel_pdf_open(input,(size_t)size,&pages,error,sizeof(error)); assert(doc);
    assert(ocel_pdf_render(doc,0,320,240,&width,&height,&bytes,error,sizeof(error)) == 0);
    ocel_pdf_close(doc); free(input);

    /* 13 S-7: an allocation-free content stream still meets the deadline. The
     * renderer reads the document in slices and checks the clock per slice, so
     * interpretation sees time pass; once it expires, rendering stops there. */
    size_t flood_length; unsigned char *flood = cm_flood_pdf(&flood_length);
    ocel_pdf_set_clock(counting_clock);
    doc = ocel_pdf_open(flood,flood_length,&pages,error,sizeof(error));
    if (!doc) { fprintf(stderr,"flood open: %s\n",error); return 1; }
    clock_reads = 0;
    assert(ocel_pdf_render(doc,0,64,64,&width,&height,&bytes,error,sizeof(error)) == 0);
    uint64_t full = clock_reads;
    assert(full >= flood_length / 1024 / 2);
    clock_reads = 0; expire_at = full / 2;
    assert(ocel_pdf_render(doc,0,64,64,&width,&height,&bytes,error,sizeof(error)) == -1);
    /* Stopped at the checkpoint instead of reading the rest of the content. */
    assert(clock_reads <= expire_at + 16);
    /* The cut-short stream is not sticky: a later render on the same document works. */
    clock_reads = 0; expire_at = UINT64_MAX;
    assert(ocel_pdf_render(doc,0,64,64,&width,&height,&bytes,error,sizeof(error)) == 0);
    ocel_pdf_close(doc); free(flood);
    ocel_pdf_set_clock(NULL);
    printf("allocation-free content: %llu clock reads per render, deadline stops it\n",
        (unsigned long long)full);
    puts("MuPDF host PDF raster and C exception smoke passed");
    return 0;
}
