/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Host stand-in for mlibc's vicell sysdeps: defines open/close/fstat/ioctl and
 * cellos_fd_provider_register in the test executable (taking precedence over
 * glibc's) and routes them through the same header-only core as
 * generic/generic.cpp. The kernel route is a fake VFS rather than Linux, so
 * the host's own /dev/dri never answers.
 */

#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdarg.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <vicell/fd_provider_core.h>

#include "harness.h"

#define KERNEL_FDS 16

static int kernel_open_fds[KERNEL_FDS];
static int kernel_close_calls;

/* VFS files: HOST_VFS_FILE opens as fds 3..; HOST_VFS_HIGH_FD comes back at
 * the provider base (a kernel fd the route must refuse); the rest ENOENT. */
static int kernel_open(const char *path, int flags, mode_t mode, int *fd) {
    (void)flags;
    (void)mode;
    if (strcmp(path, HOST_VFS_HIGH_FD) == 0) {
        *fd = CELLOS_FD_PROVIDER_BASE;
        return 0;
    }
    if (strcmp(path, HOST_VFS_FILE) != 0)
        return ENOENT;
    for (int i = 0; i < KERNEL_FDS; i++) {
        if (!kernel_open_fds[i]) {
            kernel_open_fds[i] = 1;
            *fd = 3 + i;
            return 0;
        }
    }
    return EMFILE;
}

static int kernel_close(int fd) {
    kernel_close_calls++;
    if (fd == CELLOS_FD_PROVIDER_BASE)
        return 0;
    if (fd < 3 || fd >= 3 + KERNEL_FDS || !kernel_open_fds[fd - 3])
        return EBADF;
    kernel_open_fds[fd - 3] = 0;
    return 0;
}

static int kernel_fstat(int fd, struct stat *st) {
    if (fd < 3 || fd >= 3 + KERNEL_FDS || !kernel_open_fds[fd - 3])
        return EBADF;
    memset(st, 0, sizeof(*st));
    st->st_mode = S_IFREG;
    return 0;
}

static void kernel_yield(void) {
    sched_yield();
}

static const struct cellos_fd_kernel host_kernel = {
    .open = kernel_open,
    .close = kernel_close,
    .fstat = kernel_fstat,
    .yield = kernel_yield,
};

struct cellos_fd_registry host_registry = {.kernel = &host_kernel};

int host_kernel_close_calls(void) {
    return kernel_close_calls;
}

int cellos_fd_provider_register(const char *prefix, const struct cellos_fd_provider_ops *ops,
                                void *ctx) {
    return cellos_fd_core_register(&host_registry, prefix, ops, ctx);
}

static int errno_result(int err) {
    if (!err)
        return 0;
    errno = err;
    return -1;
}

int open(const char *path, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    int fd = -1;
    int err = cellos_fd_core_open(&host_registry, path, flags, mode, &fd);
    return err ? errno_result(err) : fd;
}

int close(int fd) {
    return errno_result(cellos_fd_core_close(&host_registry, fd));
}

int fstat(int fd, struct stat *st) {
    return errno_result(cellos_fd_core_fstat(&host_registry, fd, st));
}

/* As libs/drm-cellos/c/ioctl.c on Cellos: the request passes through an int
 * (posix_devctl) before it reaches the core, so it arrives sign-extended. */
int ioctl(int fd, unsigned long request, ...) {
    va_list args;
    va_start(args, request);
    void *arg = va_arg(args, void *);
    va_end(args);
    int result = 0;
    int err = cellos_fd_core_ioctl(&host_registry, fd, (unsigned long)(long)(int)request, arg,
                                   &result);
    return err ? errno_result(err) : result;
}
