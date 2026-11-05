/* SPDX-License-Identifier: MPL-2.0 */
/* `ioctl` for Cellos (compat/sys/ioctl.h); built only for Cellos targets. */

#include <devctl.h>
#include <errno.h>
#include <stdarg.h>
#include <sys/ioctl.h>

int ioctl(int fd, unsigned long request, ...) {
    va_list args;
    va_start(args, request);
    void *arg = va_arg(args, void *);
    va_end(args);

    /*
     * posix_devctl takes the request as an int; Linux request numbers are 32
     * bits and the vicell sysdep clears the sign extension. With no direct
     * PosixDevctl sysdep, mlibc ignores nbyte and stores the ioctl result in
     * dev_info.
     */
    int result = 0;
    int err = posix_devctl(fd, (int)request, arg, 0, &result);
    if (err) {
        errno = err;
        return -1;
    }
    return result;
}
