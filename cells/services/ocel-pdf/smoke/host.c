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
    ocel_pdf_close(doc); free(input);
    puts("MuPDF host PDF raster and C exception smoke passed");
    return 0;
}
