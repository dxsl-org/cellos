// SPDX-License-Identifier: MIT
// Cellos fd providers: user-space device nodes served inside the Cell.
//
// A provider claims a path prefix. `open` of a path under that prefix calls the
// provider instead of the kernel VFS and returns an fd from the reserved range
// [CELLOS_FD_PROVIDER_BASE, CELLOS_FD_PROVIDER_BASE + CELLOS_FD_PROVIDER_SLOTS).
// `close`, `fstat` and `ioctl` (reached through `posix_devctl`; libdrm-lite
// supplies `ioctl` on top of it) on such an fd go to the same provider. Every
// other path and fd keeps the kernel route; `ioctl` on a kernel fd is ENOTTY,
// because the Cellos kernel has no ioctl.
//
// Contract for providers:
// - `ops` and `ctx` stay valid for the life of the process; registration is
//   permanent.
// - Every op returns 0 or a positive errno value, never -1.
// - `open` receives the full path and returns a provider handle; `close` is
//   called exactly once per successful `open`, after the last in-flight
//   `ioctl`/`fstat` on that fd has returned, even if the application closed
//   the fd while another thread was still inside an op.
// - `ioctl` receives the 32-bit Linux request number (upper bits cleared) and
//   may set `*result` (pre-set to 0) as the ioctl return value.
// - Ops run without registry locks held and may block (for example on IPC);
//   the provider serialises its own state.
// - `mmap`/`munmap` are part of the v1 table but are not routed yet: they
//   wait for anonymous-memory support (runtime plan R3b). May be NULL.
//
// This header is not installed into the mlibc sysroot (meson.build installs
// only abi-bits); consumers add third_party/mlibc/sysdeps/vicell/include to
// their include path.
#ifndef VICELL_FD_PROVIDER_H
#define VICELL_FD_PROVIDER_H

#include <stddef.h>
#include <sys/stat.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CELLOS_FD_PROVIDER_ABI 1

// The kernel numbers fds from 3 as "highest open + 1" with no upper bound
// (kernel/src/task.rs `file_open`); a kernel fd at or above the base would
// alias a provider fd, so the kernel route refuses it with EMFILE.
#define CELLOS_FD_PROVIDER_BASE 0x100000
#define CELLOS_FD_PROVIDER_SLOTS 64
#define CELLOS_FD_PROVIDER_MAX 4
// Including the terminating NUL.
#define CELLOS_FD_PROVIDER_PREFIX_MAX 64

struct cellos_fd_provider_ops {
    unsigned abi; // CELLOS_FD_PROVIDER_ABI
    int (*open)(void *ctx, const char *path, int flags, void **handle);
    int (*close)(void *ctx, void *handle);
    int (*ioctl)(void *ctx, void *handle, unsigned long request, void *arg, int *result);
    int (*fstat)(void *ctx, void *handle, struct stat *st);
    int (*mmap)(void *ctx, void *handle, void *hint, size_t size, int prot, int flags,
                off_t offset, void **window);
    int (*munmap)(void *ctx, void *handle, void *addr, size_t size);
};

// Claims `prefix`: absolute, ending in '/', shorter than
// CELLOS_FD_PROVIDER_PREFIX_MAX. Returns 0, EINVAL (bad prefix or ops),
// EEXIST (a registered prefix equals, contains or is contained in `prefix`)
// or ENOSPC (CELLOS_FD_PROVIDER_MAX providers already registered).
int cellos_fd_provider_register(const char *prefix, const struct cellos_fd_provider_ops *ops,
                                void *ctx);

#ifdef __cplusplus
}
#endif

#endif // VICELL_FD_PROVIDER_H
