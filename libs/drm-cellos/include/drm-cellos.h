/* SPDX-License-Identifier: MPL-2.0 */
/*
 * libdrm-lite, Cellos side: the device-enumeration backend that replaces
 * libdrm's sysfs scan (owner decision D-R9).
 *
 * drmGetDevices2/drmGetDevice2/drmGetDevice/drmFreeDevice/drmFreeDevices
 * describe exactly one PCI device:
 * - vendor 0x8086 (the iGPU driver Cell matches Intel PCI ids only);
 * - device id and revision from the provider's `xe` DEVICE_QUERY CONFIG
 *   (REV_AND_DEVICE_ID), after DRM_IOCTL_VERSION reports "xe";
 * - bus address from the Cell's launch configuration, set with
 *   drm_cellos_set_pci_bus (default 0000:00:02.0);
 * - one render node, DRM_CELLOS_RENDER_NODE; subsystem ids read 0.
 * The node is served by whatever fd provider claims DRM_CELLOS_NODE_PREFIX
 * (vicell/fd_provider.h): the IPC transport in production, the fake provider
 * (drm-cellos-fake.h) in tests.
 */
#ifndef DRM_CELLOS_H
#define DRM_CELLOS_H

#include <stdint.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DRM_CELLOS_NODE_PREFIX "/dev/dri/"
#define DRM_CELLOS_RENDER_NODE "/dev/dri/renderD128"
#define DRM_CELLOS_VENDOR_INTEL 0x8086
#define DRM_CELLOS_MAJOR 226
#define DRM_CELLOS_RENDER_MINOR 128

/*
 * Linux/glibc dev_t encoding (what Mesa's major()/minor() decode). mlibc's
 * posix option has no <sys/sysmacros.h>, so the encoding is spelled out.
 */
#define DRM_CELLOS_MAKEDEV(ma, mi)                                                       \
    ((((dev_t)(ma) & 0xfffff000u) << 32) | (((dev_t)(ma) & 0xfffu) << 8)                 \
     | (((dev_t)(mi) & 0xffffff00u) << 12) | ((dev_t)(mi) & 0xffu))
#define DRM_CELLOS_MAJOR_OF(dev) ((unsigned)((((dev) >> 32) & 0xfffff000u) | (((dev) >> 8) & 0xfffu)))
#define DRM_CELLOS_MINOR_OF(dev) ((unsigned)((((dev) >> 12) & 0xffffff00u) | ((dev) & 0xffu)))
#define DRM_CELLOS_RENDER_RDEV DRM_CELLOS_MAKEDEV(DRM_CELLOS_MAJOR, DRM_CELLOS_RENDER_MINOR)

/*
 * Sets the PCI address reported for the device. Call before the first
 * enumeration; not synchronised with concurrent drmGetDevice* calls.
 * Returns 0, or EINVAL when dev > 31 or func > 7.
 */
int drm_cellos_set_pci_bus(uint16_t domain, uint8_t bus, uint8_t dev, uint8_t func);

#ifdef __cplusplus
}
#endif

#endif /* DRM_CELLOS_H */
