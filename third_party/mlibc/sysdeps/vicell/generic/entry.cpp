// SPDX-License-Identifier: MIT
// ViCell mlibc entry point shim.
//
// ViCell uses ostd's _start (in libs/ostd), NOT mlibc's crt1.  This file
// exists so mlibc's build system has a named entry translation unit, but it
// must not emit a conflicting _start or main wrapper.
//
// __mlibc_do_entry is called by mlibc's rtld stubs; it is a no-op here because
// ViCell does not use dynamic linking in G2.  If dynamic linking is added in G3,
// this should perform ELF relocation and call the user's main via __libc_start_main.
extern "C" {

// Called by mlibc headers as part of the libc startup path.
// No-op: ostd's _start already initialises the heap and calls main.
void __mlibc_do_entry() {}

// Provide a minimal __libc_start_main so statically-linked C programs that
// call it directly work as expected.  argc/argv are passed by ostd's _start
// on the stack; this trampoline just invokes main and exits.
int __libc_start_main(int (*main_fn)(int, char **, char **),
                      int argc, char **argv) {
    int ret = main_fn(argc, argv, nullptr);
    // sys_exit declared in generic.cpp / sysdeps.hpp
    extern void sys_exit(int) __attribute__((__noreturn__));
    sys_exit(ret);
}

} // extern "C"
