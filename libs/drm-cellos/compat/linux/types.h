/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Cellos-only stand-in for <linux/types.h>: the fixed-width uAPI names that
 * drm.h and xe_drm.h use. Cellos serves the Linux DRM/xe uAPI unchanged, so
 * these are the x86_64 Linux sizes. Not on the include path of host builds.
 */
#ifndef CELLOS_COMPAT_LINUX_TYPES_H
#define CELLOS_COMPAT_LINUX_TYPES_H

#include <stddef.h>
#include <stdint.h>

typedef int8_t __s8;
typedef uint8_t __u8;
typedef int16_t __s16;
typedef uint16_t __u16;
typedef int32_t __s32;
typedef uint32_t __u32;
typedef int64_t __s64;
typedef uint64_t __u64;
typedef unsigned long __kernel_size_t;

#endif /* CELLOS_COMPAT_LINUX_TYPES_H */
