/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Cellos-only <sys/ioctl.h>. mlibc builds `ioctl` with its glibc option,
 * which Cellos leaves off (owner decision D-R8); libdrm-lite provides it on
 * top of POSIX posix_devctl (c/ioctl.c), which reaches the vicell `Ioctl`
 * sysdep. Not on the include path of host builds.
 */
#ifndef CELLOS_COMPAT_SYS_IOCTL_H
#define CELLOS_COMPAT_SYS_IOCTL_H

#include <asm/ioctl.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * `request` is a Linux ioctl number (32 bits); `arg` is the one pointer
 * argument. Returns the provider's result, or -1 with errno (ENOTTY on a
 * kernel fd, EBADF on a closed provider fd).
 */
int ioctl(int fd, unsigned long request, ...);

#ifdef __cplusplus
}
#endif

#endif /* CELLOS_COMPAT_SYS_IOCTL_H */
