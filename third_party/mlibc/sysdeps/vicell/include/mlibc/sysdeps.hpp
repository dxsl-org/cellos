// SPDX-License-Identifier: MIT
// ViCell OS-specific sysdeps header for mlibc.
// Consumed by mlibc's generic headers to know what primitives we provide.
#pragma once

#include <abi-bits/errno.h>
#include <abi-bits/fcntl.h>
#include <abi-bits/seek-whence.h>
#include <abi-bits/signal.h>
#include <abi-bits/stat.h>
#include <abi-bits/vm-flags.h>
#include <stddef.h>
#include <stdint.h>

// Tells mlibc we are a hosted-ish bare-metal target:
// - We provide file I/O, clock, and anonymous memory
// - We do NOT provide networking, sockets, or pthreads (-Dposix_option=disabled)
// - CRT is provided by ostd (_start), not by mlibc's crt1.o
#define __MLIBC_ANSI_OPTION    1
#define __MLIBC_LINUX_OPTION   0
#define __MLIBC_POSIX_OPTION   0

// Optional sysdep: isatty — we implement it to control stdio buffering
#define __MLIBC_HAVE_SYS_ISATTY 1

namespace mlibc {

// ─── Mandatory sysdeps ─────────────────────────────────────────────────────

// Logging and panic (required; called by mlibc internals on assertion failure)
void sys_libc_log(const char *message);
[[noreturn]] void sys_libc_panic();

// Process exit
[[noreturn]] void sys_exit(int status);

// Thread Control Block — sets the TCB pointer register used by TLS
int sys_tcb_set(void *pointer);

// Futex primitives — spin-loop stubs in G2 (pthreads disabled)
int sys_futex_wait(int *pointer, int expected, const struct timespec *time);
int sys_futex_wake(int *pointer);

// File I/O
int sys_open(const char *pathname, int flags, mode_t mode, int *fd);
int sys_read(int fd, void *buf, size_t count, ssize_t *bytes_read);
int sys_write(int fd, const void *buf, size_t count, ssize_t *bytes_written);
int sys_close(int fd);
int sys_seek(int fd, off_t offset, int whence, off_t *new_offset);

// Clock
int sys_clock_get(int clock, time_t *secs, long *nanos);

// Anonymous memory (backing mlibc's frg::slab_allocator)
int sys_anon_allocate(size_t size, void **pointer);
int sys_anon_free(void *pointer, size_t size);

// VM mapping (MAP_ANONYMOUS → anon_allocate; everything else → EINVAL)
int sys_vm_map(void *hint, size_t size, int prot, int flags, int fd,
               off_t offset, void **window);
int sys_vm_unmap(void *pointer, size_t size);

// ─── Optional sysdeps ──────────────────────────────────────────────────────

// isatty — fd 0-2 are always considered terminals so stdio is unbuffered
int sys_isatty(int fd);

} // namespace mlibc
