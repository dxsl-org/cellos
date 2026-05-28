#include <stddef.h>
#include <errno.h>

// We link with libc, so we don't need stdio/stdlib/string implementations.
// Only stubs for things possibly missing or custom.

#include <sys/reent.h>
#include <stdlib.h>
#include <string.h>

struct _reent * _impure_ptr;
    
void init_impure_ptr() {
    _impure_ptr = (struct _reent *)malloc(sizeof(struct _reent));
    if (_impure_ptr) {
        _REENT_INIT_PTR(_impure_ptr);
    }
    // Also set stdout/stdin if needed, but REENT_INIT_PTR does basic stuff
}

#include <stdio.h>
#include <unistd.h>

void init_stdio_files() {
    // Force no buffering
    if (stdout) {
        stdout->_file = 1;
        stdout->_flags |= __SWR; // Write mode
        setvbuf(stdout, NULL, _IONBF, 0);
    }
    
    if (stderr) {
        stderr->_file = 2;
        stderr->_flags |= __SWR; // Write mode
        setvbuf(stderr, NULL, _IONBF, 0);
    }
    if (stdin) {
        stdin->_file = 0;
        stdin->_flags |= __SRD; // Read mode
        setvbuf(stdin, NULL, _IONBF, 0);
    }
}
int system(const char *command) {
    (void)command;
    return -1;
}

char *getenv(const char *name) {
    (void)name;
    return NULL;
}

char *tmpnam(char *s) {
    (void)s;
    return NULL;
}

#include <stdio.h>
FILE *tmpfile(void) {
    return NULL;
}
