/* SPDX-License-Identifier: MPL-2.0 */
#ifndef _UNISTD_H
#define _UNISTD_H

#include <stddef.h>
#include <sys/types.h>

#define SEEK_SET    0
#define SEEK_CUR    1
#define SEEK_END    2

#ifdef __cplusplus
extern "C" {
#endif

int close(int fd);
ssize_t read(int fd, void *buf, size_t count);
ssize_t write(int fd, const void *buf, size_t count);
off_t lseek(int fd, off_t offset, int whence);
int unlink(const char *pathname);
unsigned int sleep(unsigned int seconds);
int usleep(unsigned long usec);
int isatty(int fd);

#ifdef __cplusplus
}
#endif

#endif /* _UNISTD_H */
