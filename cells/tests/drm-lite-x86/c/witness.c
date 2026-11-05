// SPDX-License-Identifier: MPL-2.0
// libdrm-lite checks for x86_64 Tier-2 (runtime plan R6), against the fake
// provider. Each check prints "DRM-LITE: <name> OK|FAIL"; the summary line is
// "DRM-LITE: <p>/<t> pass". Provider-backed mmap/munmap joins after R3b.

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <vicell/fd_provider.h>

#include "drm-cellos-fake.h"
#include "drm-cellos.h"
#include "xf86drm.h"

// [inference] a Panther Lake device id; any id round-trips through CONFIG.
#define FAKE_DEVICE_ID 0xb080
#define FAKE_REVISION 4
// The witness's own image, served by the kernel VFS.
#define VFS_FILE "/bin/drm-lite-x86"

static int check(const char *name, int ok) {
    printf("DRM-LITE: %s %s\n", name, ok ? "OK" : "FAIL");
    return ok ? 1 : 0;
}

static int device_ok(drmDevicePtr d, uint8_t revision) {
    return d && d->bustype == DRM_BUS_PCI && d->available_nodes == 1 << DRM_NODE_RENDER
           && strcmp(d->nodes[DRM_NODE_RENDER], DRM_CELLOS_RENDER_NODE) == 0
           && d->deviceinfo.pci->vendor_id == 0x8086
           && d->deviceinfo.pci->device_id == FAKE_DEVICE_ID
           && d->deviceinfo.pci->revision_id == revision && d->businfo.pci->domain == 0
           && d->businfo.pci->bus == 0 && d->businfo.pci->dev == 2 && d->businfo.pci->func == 0;
}

int drm_lite_witness(void) {
    int pass = 0, total = 0;

    errno = 0;
    int fd = open(DRM_CELLOS_RENDER_NODE, O_RDWR);
    total++, pass += check("no-provider-enoent", fd == -1 && errno == ENOENT);

    struct drm_version version;
    memset(&version, 0, sizeof(version));
    int vfs = open(VFS_FILE, O_RDONLY);
    errno = 0;
    int rc = vfs >= 0 ? ioctl(vfs, DRM_IOCTL_VERSION, &version) : 0;
    total++, pass += check("vfs-ioctl-enotty", vfs >= 0 && vfs < CELLOS_FD_PROVIDER_BASE
                                                 && rc == -1 && errno == ENOTTY);
    if (vfs >= 0)
        close(vfs);

    struct drm_cellos_fake_identity identity = {FAKE_DEVICE_ID, FAKE_REVISION};
    total++, pass += check("register", drm_cellos_fake_provider_register(&identity) == 0);

    errno = 0;
    fd = open("/dev/dri/card0", O_RDWR);
    total++, pass += check("card0-enoent", fd == -1 && errno == ENOENT);

    drmDevicePtr devices[4] = {0};
    int count = drmGetDevices2(DRM_DEVICE_GET_PCI_REVISION, devices, 4);
    total++, pass += check("enumerate", count == 1 && device_ok(devices[0], FAKE_REVISION));
    drmFreeDevices(devices, count > 0 ? count : 0);

    fd = open(DRM_CELLOS_RENDER_NODE, O_RDWR | O_CLOEXEC);
    total++, pass += check("open-render", fd >= CELLOS_FD_PROVIDER_BASE);

    struct stat st;
    total++, pass += check("fstat", fstat(fd, &st) == 0 && S_ISCHR(st.st_mode)
                                        && st.st_rdev == DRM_CELLOS_RENDER_RDEV);

    drmVersionPtr v = drmGetVersion(fd);
    total++, pass += check("version-xe", v && v->name && strcmp(v->name, "xe") == 0);
    drmFreeVersion(v);

    uint64_t value = 0;
    total++, pass += check("cap-syncobj-timeline",
                           drmGetCap(fd, DRM_CAP_SYNCOBJ_TIMELINE, &value) == 0 && value == 1);

    uint32_t handle = 0;
    int created = drmSyncobjCreate(fd, 0, &handle) == 0 && handle != 0;
    int destroyed = created && drmSyncobjDestroy(fd, handle) == 0;
    errno = 0;
    int stale = drmSyncobjDestroy(fd, handle) == -1 && errno == EINVAL;
    total++, pass += check("syncobj", created && destroyed && stale);

    drmDevicePtr device = NULL;
    total++, pass += check("device-from-fd", drmGetDevice2(fd, DRM_DEVICE_GET_PCI_REVISION, &device) == 0
                                                 && device_ok(device, FAKE_REVISION));
    drmFreeDevice(&device);

    int closed = close(fd) == 0;
    errno = 0;
    total++, pass += check("close", closed && ioctl(fd, DRM_IOCTL_VERSION, &version) == -1
                                        && errno == EBADF);

    printf("DRM-LITE: %d/%d pass\n", pass, total);
    return pass == total ? 0 : 1;
}
