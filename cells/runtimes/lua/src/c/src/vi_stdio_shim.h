#ifndef VI_STDIO_SHIM_H
#define VI_STDIO_SHIM_H

#include <stddef.h>
#include <stdarg.h>

// Prevent newlib stdio.h from being included
#define _STDIO_H_

// Typedefs (Lua checks pointers to FILE)
typedef void FILE;

// Constants
#define EOF (-1)
#define BUFSIZ 1024
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

// Global streams (redirect to our symbols)
#define stdin  ((FILE*)vi_stdin)
#define stdout ((FILE*)vi_stdout)
#define stderr ((FILE*)vi_stderr)

extern void *vi_stdin;
extern void *vi_stdout;
extern void *vi_stderr;

// Function prototypes used by Lua (must match our vi_shim.c and posix.rs imports)
// posix.rs exports C functions, so we can declare them here.
int fprintf(FILE *stream, const char *format, ...);
int printf(const char *format, ...);
int snprintf(char *str, size_t size, const char *format, ...);
int sprintf(char *str, const char *format, ...);
int setvbuf(FILE *stream, char *buf, int modes, size_t n);
FILE *fopen(const char *filename, const char *mode);
FILE *freopen(const char *filename, const char *mode, FILE *stream);
int fclose(FILE *stream);
int fseek(FILE *stream, long offset, int whence);
long ftell(FILE *stream);
size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);
int feof(FILE *stream);
int ferror(FILE *stream);
int getc(FILE *stream);
int ungetc(int c, FILE *stream);
int fflush(FILE *stream);
int fputc(int c, FILE *stream);
int remove(const char *pathname);
int rename(const char *oldpath, const char *newpath);
FILE *tmpfile(void);
char *tmpnam(char *s);

// _IONBF for setvbuf (if used)
#define _IONBF 2

#endif
