/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Test-only fd provider for DRM_CELLOS_NODE_PREFIX. It serves
 * DRM_CELLOS_RENDER_NODE (every other name under the prefix is ENOENT) and
 * answers, per open file:
 * - DRM_IOCTL_VERSION: "xe", by the two-call length protocol;
 * - DRM_IOCTL_GET_CAP: SYNCOBJ and SYNCOBJ_TIMELINE = 1, others EINVAL;
 * - DRM_IOCTL_XE_DEVICE_QUERY CONFIG: the identity below, VA_BITS 48,
 *   MIN_ALIGNMENT 4096, MAX_EXEC_QUEUE_PRIORITY 1, by the size protocol;
 * - DRM_IOCTL_SYNCOBJ_CREATE/DESTROY on a per-file handle table;
 * - every other request: EINVAL (as DRM does for an unknown ioctl);
 * - fstat: S_IFCHR, st_rdev = DRM_CELLOS_RENDER_RDEV;
 * - mmap/munmap: anonymous memory (not routed until runtime plan R3b).
 * The iGPU G3 transport replaces it in production.
 */
#ifndef DRM_CELLOS_FAKE_H
#define DRM_CELLOS_FAKE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct drm_cellos_fake_identity {
    uint16_t device_id;
    uint8_t revision;
};

/*
 * Registers the fake provider for DRM_CELLOS_NODE_PREFIX with `identity`
 * (copied). Returns cellos_fd_provider_register's result; a second call
 * returns EEXIST.
 */
int drm_cellos_fake_provider_register(const struct drm_cellos_fake_identity *identity);

#ifdef __cplusplus
}
#endif

#endif /* DRM_CELLOS_FAKE_H */
