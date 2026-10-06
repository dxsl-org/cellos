/* Cellos freestanding errno.h. Only included by upstream under CONFIG_ATOMICS,
 * which this build compiles out; the constants are here so the header resolves
 * for any future include. */
#ifndef QJS_VIOS_ERRNO_H
#define QJS_VIOS_ERRNO_H

extern int errno;

#define EPERM 1
#define ENOENT 2
#define EINTR 4
#define EIO 5
#define EAGAIN 11
#define ENOMEM 12
#define EINVAL 22
#define ENOSYS 38

#endif
