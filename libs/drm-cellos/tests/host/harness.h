/* SPDX-License-Identifier: MPL-2.0 */
#ifndef DRM_CELLOS_HOST_HARNESS_H
#define DRM_CELLOS_HOST_HARNESS_H

#include <vicell/fd_provider_core.h>

#define HOST_VFS_FILE "/etc/cellos-test"
#define HOST_VFS_HIGH_FD "/etc/high-fd"

extern struct cellos_fd_registry host_registry;

/* Kernel-route close calls so far. */
int host_kernel_close_calls(void);

#endif /* DRM_CELLOS_HOST_HARNESS_H */
