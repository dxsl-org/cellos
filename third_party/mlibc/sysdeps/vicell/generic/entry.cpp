// SPDX-License-Identifier: MIT
// Cellos (vicell) mlibc entry point shim.
//
// Cellos uses ostd's _start (in libs/ostd), NOT mlibc's crt1. This file exists
// so mlibc's build system has a named entry translation unit, but it must not
// emit a conflicting _start or main wrapper.
//
// __mlibc_do_entry is a no-op: Cellos does not use dynamic linking.
#include <mlibc/all-sysdeps.hpp>

extern "C" {

// No-op: ostd's _start already initialises the heap and calls main.
void __mlibc_do_entry() {}

// Minimal __libc_start_main so statically linked C programs that call it work:
// invoke main, then exit through the Exit sysdep. It does not run mlibc's
// static initialisation (__dlapi_enter), so TLS blocks, environ and auxv are
// not set up here.
int __libc_start_main(int (*main_fn)(int, char **, char **), int argc, char **argv) {
    int ret = main_fn(argc, argv, nullptr);
    // Tags live at global scope at some mlibc revisions and in `mlibc` at others.
    using namespace mlibc;
    sysdep<Exit>(ret);
}

} // extern "C"
