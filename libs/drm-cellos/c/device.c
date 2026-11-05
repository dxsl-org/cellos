/* SPDX-License-Identifier: MPL-2.0 */
/*
 * libdrm device enumeration for Cellos (drm-cellos.h): the libdrm entry points
 * that read sysfs upstream, rebuilt on the render node's own ioctls. Return
 * conventions match upstream xf86drm.c: negative errno values, and for
 * drmGetDevices2 a device count.
 */

#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "drm-cellos.h"
#include "xe_drm.h"
#include "xf86drm.h"

struct pci_address {
    uint16_t domain;
    uint8_t bus, dev, func;
};

/* [inference] the conventional Intel iGPU address; the launch config overrides it. */
static struct pci_address pci_address = {0, 0, 2, 0};

int drm_cellos_set_pci_bus(uint16_t domain, uint8_t bus, uint8_t dev, uint8_t func) {
    if (dev > 31 || func > 7)
        return EINVAL;
    pci_address.domain = domain;
    pci_address.bus = bus;
    pci_address.dev = dev;
    pci_address.func = func;
    return 0;
}

struct identity {
    uint16_t device_id;
    uint8_t revision;
};

/* The `xe` CONFIG item, by its two-call size protocol. */
static int query_config_id(int fd, uint64_t *rev_and_device_id) {
    struct drm_xe_device_query query;
    memset(&query, 0, sizeof(query));
    query.query = DRM_XE_DEVICE_QUERY_CONFIG;
    if (drmIoctl(fd, DRM_IOCTL_XE_DEVICE_QUERY, &query))
        return -errno;
    size_t minimum = sizeof(struct drm_xe_query_config)
                     + (DRM_XE_QUERY_CONFIG_REV_AND_DEVICE_ID + 1) * sizeof(uint64_t);
    if (query.size < minimum)
        return -ENODEV;

    struct drm_xe_query_config *config = calloc(1, query.size);
    if (!config)
        return -ENOMEM;
    query.data = (uintptr_t)config;
    int ret = 0;
    if (drmIoctl(fd, DRM_IOCTL_XE_DEVICE_QUERY, &query))
        ret = -errno;
    else if (config->num_params <= DRM_XE_QUERY_CONFIG_REV_AND_DEVICE_ID)
        ret = -ENODEV;
    else
        *rev_and_device_id = config->info[DRM_XE_QUERY_CONFIG_REV_AND_DEVICE_ID];
    free(config);
    return ret;
}

/* Only an `xe` device is enumerated; anything else reads as -ENODEV. */
static int query_identity(int fd, struct identity *id) {
    errno = 0;
    drmVersionPtr version = drmGetVersion(fd);
    if (!version)
        return errno ? -errno : -ENODEV;
    int is_xe = version->name && strcmp(version->name, "xe") == 0;
    drmFreeVersion(version);
    if (!is_xe)
        return -ENODEV;

    uint64_t value = 0;
    int ret = query_config_id(fd, &value);
    if (ret)
        return ret;
    /* xe_drm.h: device id in bits 0-15, revision in bits 16-23. */
    id->device_id = (uint16_t)(value & 0xffff);
    id->revision = (uint8_t)((value >> 16) & 0xff);
    return 0;
}

/* One allocation, freed by a plain free() as upstream's drmDeviceAlloc. */
static int build_device(const struct identity *id, uint32_t flags, drmDevicePtr *out) {
    size_t node_len = (sizeof(DRM_CELLOS_RENDER_NODE) + sizeof(void *) - 1) & ~(sizeof(void *) - 1);
    size_t size = sizeof(drmDevice) + DRM_NODE_MAX * (sizeof(char *) + node_len)
                  + sizeof(drmPciBusInfo) + sizeof(drmPciDeviceInfo);
    drmDevicePtr device = calloc(1, size);
    if (!device)
        return -ENOMEM;

    char *cursor = (char *)(device + 1);
    device->nodes = (char **)cursor;
    cursor += DRM_NODE_MAX * sizeof(char *);
    for (int i = 0; i < DRM_NODE_MAX; i++) {
        device->nodes[i] = cursor;
        cursor += node_len;
    }
    memcpy(device->nodes[DRM_NODE_RENDER], DRM_CELLOS_RENDER_NODE, sizeof(DRM_CELLOS_RENDER_NODE));
    device->available_nodes = 1 << DRM_NODE_RENDER;
    device->bustype = DRM_BUS_PCI;

    drmPciBusInfoPtr bus = (drmPciBusInfoPtr)cursor;
    bus->domain = pci_address.domain;
    bus->bus = pci_address.bus;
    bus->dev = pci_address.dev;
    bus->func = pci_address.func;
    device->businfo.pci = bus;

    drmPciDeviceInfoPtr info = (drmPciDeviceInfoPtr)(cursor + sizeof(drmPciBusInfo));
    info->vendor_id = DRM_CELLOS_VENDOR_INTEL;
    info->device_id = id->device_id;
    /* Upstream reports 0xff when the revision was not requested. */
    info->revision_id = (flags & DRM_DEVICE_GET_PCI_REVISION) ? id->revision : 0xff;
    device->deviceinfo.pci = info;

    *out = device;
    return 0;
}

static int valid_flags(uint32_t flags) {
    return (flags & ~(uint32_t)DRM_DEVICE_GET_PCI_REVISION) == 0;
}

int drmGetDevice2(int fd, uint32_t flags, drmDevicePtr *device) {
    if (!valid_flags(flags) || fd == -1 || !device)
        return -EINVAL;
    struct stat st;
    if (fstat(fd, &st))
        return -errno;
    if (!S_ISCHR(st.st_mode))
        return -EINVAL;
    if (DRM_CELLOS_MAJOR_OF(st.st_rdev) != DRM_CELLOS_MAJOR
        || DRM_CELLOS_MINOR_OF(st.st_rdev) != DRM_CELLOS_RENDER_MINOR)
        return -ENODEV;

    struct identity id;
    int ret = query_identity(fd, &id);
    if (ret)
        return ret;
    return build_device(&id, flags, device);
}

int drmGetDevice(int fd, drmDevicePtr *device) {
    return drmGetDevice2(fd, DRM_DEVICE_GET_PCI_REVISION, device);
}

int drmGetDevices2(uint32_t flags, drmDevicePtr devices[], int max_devices) {
    if (!valid_flags(flags))
        return -EINVAL;
    int fd = open(DRM_CELLOS_RENDER_NODE, O_RDWR | O_CLOEXEC);
    if (fd < 0)
        return -errno;

    struct identity id;
    int ret = query_identity(fd, &id);
    close(fd);
    /* As upstream skips a node it cannot identify, so does this backend. */
    if (ret)
        return ret == -ENOMEM ? ret : 0;
    if (!devices)
        return 1;
    if (max_devices < 1)
        return 0;
    ret = build_device(&id, flags, &devices[0]);
    return ret ? ret : 1;
}

void drmFreeDevice(drmDevicePtr *device) {
    if (!device)
        return;
    free(*device);
    *device = NULL;
}

void drmFreeDevices(drmDevicePtr devices[], int count) {
    if (!devices)
        return;
    for (int i = 0; i < count; i++)
        drmFreeDevice(&devices[i]);
}
