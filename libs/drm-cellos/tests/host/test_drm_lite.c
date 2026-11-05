/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Host tests for libdrm-lite: the pinned libdrm subset, the Cellos
 * enumeration backend and the fake provider, routed by the vicell fd provider
 * core (harness.c). Cases run in order on one process-wide registry.
 */

#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

#include "drm-cellos-fake.h"
#include "drm-cellos.h"
#include "harness.h"
#include "xe_drm.h"
#include "xf86drm.h"

#define FAKE_DEVICE_ID 0xb080
#define FAKE_REVISION 4

static int checks, failures;

#define EXPECT(cond)                                                                     \
    do {                                                                                 \
        checks++;                                                                        \
        if (!(cond)) {                                                                   \
            fprintf(stderr, "FAIL %s:%d: %s (errno %d)\n", __FILE__, __LINE__, #cond, errno); \
            failures++;                                                                  \
        }                                                                                \
    } while (0)

static int slots_in_state(unsigned state) {
    int n = 0;
    for (int i = 0; i < CELLOS_FD_PROVIDER_SLOTS; i++)
        n += host_registry.slots[i].state == state;
    return n;
}

static void test_no_provider(void) {
    errno = 0;
    EXPECT(open(DRM_CELLOS_RENDER_NODE, O_RDWR) == -1 && errno == ENOENT);
    errno = 0;
    EXPECT(open("/dev/dri/card0", O_RDWR) == -1 && errno == ENOENT);
    EXPECT(drmGetDevices2(0, NULL, 0) == -ENOENT);
}

static void test_vfs_fd(void) {
    int fd = open(HOST_VFS_FILE, O_RDONLY);
    EXPECT(fd >= 3 && fd < CELLOS_FD_PROVIDER_BASE);
    struct drm_version version;
    memset(&version, 0, sizeof(version));
    errno = 0;
    EXPECT(ioctl(fd, DRM_IOCTL_VERSION, &version) == -1 && errno == ENOTTY);
    errno = 0;
    EXPECT(drmIoctl(fd, DRM_IOCTL_VERSION, &version) == -1 && errno == ENOTTY);
    drmDevicePtr device = NULL;
    EXPECT(drmGetDevice2(fd, 0, &device) == -EINVAL && device == NULL);
    EXPECT(close(fd) == 0);
}

static void test_kernel_fd_in_provider_range(void) {
    int before = host_kernel_close_calls();
    errno = 0;
    EXPECT(open(HOST_VFS_HIGH_FD, O_RDONLY) == -1 && errno == EMFILE);
    EXPECT(host_kernel_close_calls() == before + 1);
}

static int stub_open(void *ctx, const char *path, int flags, void **handle) {
    (void)ctx, (void)path, (void)flags, (void)handle;
    return ENOENT;
}
static int stub_close(void *ctx, void *handle) {
    (void)ctx, (void)handle;
    return 0;
}
static int stub_ioctl(void *ctx, void *handle, unsigned long request, void *arg, int *result) {
    (void)ctx, (void)handle, (void)request, (void)arg, (void)result;
    return EINVAL;
}
static int stub_fstat(void *ctx, void *handle, struct stat *st) {
    (void)ctx, (void)handle, (void)st;
    return EINVAL;
}

static const struct cellos_fd_provider_ops stub_ops = {
    .abi = CELLOS_FD_PROVIDER_ABI,
    .open = stub_open,
    .close = stub_close,
    .ioctl = stub_ioctl,
    .fstat = stub_fstat,
};

static void test_register_validation(void) {
    struct cellos_fd_provider_ops wrong_abi = stub_ops;
    wrong_abi.abi = CELLOS_FD_PROVIDER_ABI + 1;
    struct cellos_fd_provider_ops no_ioctl = stub_ops;
    no_ioctl.ioctl = NULL;
    char long_prefix[CELLOS_FD_PROVIDER_PREFIX_MAX + 1];
    memset(long_prefix, 'a', sizeof(long_prefix));
    long_prefix[0] = '/';
    long_prefix[CELLOS_FD_PROVIDER_PREFIX_MAX - 1] = '/';
    long_prefix[CELLOS_FD_PROVIDER_PREFIX_MAX] = '\0';

    EXPECT(cellos_fd_provider_register("dev/dri/", &stub_ops, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register("/dev/dri", &stub_ops, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register("/", &stub_ops, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register(long_prefix, &stub_ops, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register(NULL, &stub_ops, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register("/x/", NULL, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register("/x/", &wrong_abi, NULL) == EINVAL);
    EXPECT(cellos_fd_provider_register("/x/", &no_ioctl, NULL) == EINVAL);
    EXPECT(host_registry.providers_len == 0);
}

static void test_register_fake(void) {
    struct drm_cellos_fake_identity identity = {FAKE_DEVICE_ID, FAKE_REVISION};
    EXPECT(drm_cellos_fake_provider_register(&identity) == 0);
    EXPECT(drm_cellos_fake_provider_register(&identity) == EEXIST);
    EXPECT(cellos_fd_provider_register("/dev/dri/", &stub_ops, NULL) == EEXIST);
    EXPECT(cellos_fd_provider_register("/dev/", &stub_ops, NULL) == EEXIST);
    EXPECT(cellos_fd_provider_register("/dev/dri/by-path/", &stub_ops, NULL) == EEXIST);
    EXPECT(host_registry.providers_len == 1);
}

static void test_card0_enoent(void) {
    errno = 0;
    EXPECT(open("/dev/dri/card0", O_RDWR) == -1 && errno == ENOENT);
    errno = 0;
    EXPECT(open("/dev/dri/", O_RDWR) == -1 && errno == ENOENT);
    EXPECT(slots_in_state(CFP_FREE) == CELLOS_FD_PROVIDER_SLOTS);
    /* Outside the prefix, the kernel route still answers. */
    int fd = open(HOST_VFS_FILE, O_RDONLY);
    EXPECT(fd >= 3 && fd < CELLOS_FD_PROVIDER_BASE);
    EXPECT(close(fd) == 0);
}

static void test_render_node(int fd) {
    EXPECT(fd >= CELLOS_FD_PROVIDER_BASE && fd < CELLOS_FD_PROVIDER_BASE + CELLOS_FD_PROVIDER_SLOTS);
    struct stat st;
    EXPECT(fstat(fd, &st) == 0);
    EXPECT(S_ISCHR(st.st_mode));
    EXPECT(st.st_rdev == makedev(226, 128));
    EXPECT(DRM_CELLOS_RENDER_RDEV == makedev(226, 128));
    struct drm_gem_close gem_close = {0};
    errno = 0;
    EXPECT(drmIoctl(fd, DRM_IOCTL_GEM_CLOSE, &gem_close) == -1 && errno == EINVAL);
}

static void test_version_and_caps(int fd) {
    drmVersionPtr version = drmGetVersion(fd);
    EXPECT(version != NULL);
    if (version) {
        EXPECT(version->name_len == 2 && strcmp(version->name, "xe") == 0);
        EXPECT(version->desc && (size_t)version->desc_len == strlen(version->desc));
        drmFreeVersion(version);
    }
    uint64_t value = 0;
    EXPECT(drmGetCap(fd, DRM_CAP_SYNCOBJ_TIMELINE, &value) == 0 && value == 1);
    value = 0;
    EXPECT(drmGetCap(fd, DRM_CAP_SYNCOBJ, &value) == 0 && value == 1);
    errno = 0;
    EXPECT(drmGetCap(fd, DRM_CAP_DUMB_BUFFER, &value) == -1 && errno == EINVAL);
}

static void test_syncobj(int fd) {
    uint32_t a = 0, b = 0;
    EXPECT(drmSyncobjCreate(fd, 0, &a) == 0 && a != 0);
    EXPECT(drmSyncobjCreate(fd, DRM_SYNCOBJ_CREATE_SIGNALED, &b) == 0 && b != 0 && b != a);
    EXPECT(drmSyncobjDestroy(fd, a) == 0);
    EXPECT(drmSyncobjDestroy(fd, b) == 0);
    errno = 0;
    EXPECT(drmSyncobjDestroy(fd, a) == -1 && errno == EINVAL);
    errno = 0;
    EXPECT(drmSyncobjCreate(fd, 0x80, &a) == -1 && errno == EINVAL);
}

/* Handles are per open file, as DRM's are per drm_file. */
static void test_syncobj_namespaces(int fd) {
    int other = open(DRM_CELLOS_RENDER_NODE, O_RDWR);
    EXPECT(other >= CELLOS_FD_PROVIDER_BASE && other != fd);
    uint32_t handle = 0;
    EXPECT(drmSyncobjCreate(other, 0, &handle) == 0);
    errno = 0;
    EXPECT(drmSyncobjDestroy(fd, handle) == -1 && errno == EINVAL);
    EXPECT(drmSyncobjDestroy(other, handle) == 0);
    EXPECT(close(other) == 0);
}

static void expect_device(drmDevicePtr device, uint8_t revision, uint16_t domain, uint8_t bus,
                          uint8_t dev, uint8_t func) {
    EXPECT(device != NULL);
    if (!device)
        return;
    EXPECT(device->bustype == DRM_BUS_PCI);
    EXPECT(device->available_nodes == 1 << DRM_NODE_RENDER);
    EXPECT(strcmp(device->nodes[DRM_NODE_RENDER], "/dev/dri/renderD128") == 0);
    EXPECT(device->nodes[DRM_NODE_PRIMARY][0] == '\0');
    EXPECT(device->deviceinfo.pci->vendor_id == 0x8086);
    EXPECT(device->deviceinfo.pci->device_id == FAKE_DEVICE_ID);
    EXPECT(device->deviceinfo.pci->revision_id == revision);
    EXPECT(device->businfo.pci->domain == domain);
    EXPECT(device->businfo.pci->bus == bus);
    EXPECT(device->businfo.pci->dev == dev);
    EXPECT(device->businfo.pci->func == func);
}

static void test_enumeration(int fd) {
    EXPECT(drmGetDevices2(0, NULL, 0) == 1);
    EXPECT(drmGetDevices2(0x80, NULL, 0) == -EINVAL);

    drmDevicePtr devices[4] = {0};
    EXPECT(drmGetDevices2(DRM_DEVICE_GET_PCI_REVISION, devices, 4) == 1);
    expect_device(devices[0], FAKE_REVISION, 0, 0, 2, 0);
    EXPECT(devices[1] == NULL);
    drmFreeDevices(devices, 4);
    EXPECT(devices[0] == NULL);

    EXPECT(drmGetDevices2(0, devices, 4) == 1);
    expect_device(devices[0], 0xff, 0, 0, 2, 0);
    drmFreeDevices(devices, 1);
    EXPECT(drmGetDevices2(0, devices, 0) == 0 && devices[0] == NULL);
    EXPECT(slots_in_state(CFP_OPEN) == 1); /* only the caller's fd */

    EXPECT(drm_cellos_set_pci_bus(0, 0, 32, 0) == EINVAL);
    EXPECT(drm_cellos_set_pci_bus(0, 0, 0, 8) == EINVAL);
    EXPECT(drm_cellos_set_pci_bus(1, 0x3a, 0, 1) == 0);
    drmDevicePtr device = NULL;
    EXPECT(drmGetDevice2(fd, DRM_DEVICE_GET_PCI_REVISION, &device) == 0);
    expect_device(device, FAKE_REVISION, 1, 0x3a, 0, 1);
    drmFreeDevice(&device);
    EXPECT(device == NULL);
    EXPECT(drmGetDevice(fd, &device) == 0);
    expect_device(device, FAKE_REVISION, 1, 0x3a, 0, 1);
    drmFreeDevice(&device);
    EXPECT(drmGetDevice2(fd, 0x80, &device) == -EINVAL);
    EXPECT(drm_cellos_set_pci_bus(0, 0, 2, 0) == 0);
}

static void test_fake_mmap_op(void) {
    const struct cellos_fd_core_provider *fake = &host_registry.providers[0];
    void *window = NULL;
    EXPECT(fake->ops->mmap(fake->ctx, NULL, NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, 0,
                           &window) == 0);
    if (window) {
        unsigned char *bytes = window;
        EXPECT(bytes[0] == 0 && bytes[8191] == 0);
        bytes[100] = 0x5a;
        EXPECT(fake->ops->munmap(fake->ctx, NULL, window, 8192) == 0);
    }
}

static void test_close(int fd) {
    EXPECT(close(fd) == 0);
    errno = 0;
    EXPECT(close(fd) == -1 && errno == EBADF);
    struct drm_version version;
    memset(&version, 0, sizeof(version));
    errno = 0;
    EXPECT(ioctl(fd, DRM_IOCTL_VERSION, &version) == -1 && errno == EBADF);
    struct stat st;
    errno = 0;
    EXPECT(fstat(fd, &st) == -1 && errno == EBADF);
    errno = 0;
    EXPECT(close(CELLOS_FD_PROVIDER_BASE + CELLOS_FD_PROVIDER_SLOTS - 1) == -1 && errno == EBADF);
    EXPECT(slots_in_state(CFP_FREE) == CELLOS_FD_PROVIDER_SLOTS);
}

static void test_slot_exhaustion(void) {
    int fds[CELLOS_FD_PROVIDER_SLOTS];
    int opened = 0;
    for (int i = 0; i < CELLOS_FD_PROVIDER_SLOTS; i++) {
        fds[i] = open(DRM_CELLOS_RENDER_NODE, O_RDWR);
        opened += fds[i] >= CELLOS_FD_PROVIDER_BASE;
    }
    EXPECT(opened == CELLOS_FD_PROVIDER_SLOTS);
    errno = 0;
    EXPECT(open(DRM_CELLOS_RENDER_NODE, O_RDWR) == -1 && errno == EMFILE);
    for (int i = 0; i < CELLOS_FD_PROVIDER_SLOTS; i++)
        EXPECT(close(fds[i]) == 0);
    EXPECT(slots_in_state(CFP_FREE) == CELLOS_FD_PROVIDER_SLOTS);
}

/* A provider whose ioctl 1 blocks until released, to close an fd mid-op. */
#define BLOCKING_REQUEST 1ul

static struct {
    pthread_mutex_t lock;
    pthread_cond_t cond;
    int inside, release, closes;
    unsigned long last_request;
} blocker = {PTHREAD_MUTEX_INITIALIZER, PTHREAD_COND_INITIALIZER, 0, 0, 0, 0};

static int blocker_open(void *ctx, const char *path, int flags, void **handle) {
    (void)ctx, (void)path, (void)flags;
    *handle = &blocker;
    return 0;
}
static int blocker_close(void *ctx, void *handle) {
    (void)ctx, (void)handle;
    pthread_mutex_lock(&blocker.lock);
    blocker.closes++;
    pthread_mutex_unlock(&blocker.lock);
    return 0;
}
static int blocker_ioctl(void *ctx, void *handle, unsigned long request, void *arg, int *result) {
    (void)ctx, (void)handle;
    pthread_mutex_lock(&blocker.lock);
    blocker.last_request = request;
    if (request == BLOCKING_REQUEST) {
        blocker.inside = 1;
        pthread_cond_broadcast(&blocker.cond);
        while (!blocker.release)
            pthread_cond_wait(&blocker.cond, &blocker.lock);
    }
    pthread_mutex_unlock(&blocker.lock);
    if (request == (unsigned long)DRM_IOCTL_VERSION) {
        /* An identity that is not `xe`. */
        struct drm_version *v = arg;
        size_t n = v->name_len < 4 ? v->name_len : 4;
        if (v->name && n)
            memcpy(v->name, "i915", n);
        v->name_len = 4;
        v->date_len = 0;
        v->desc_len = 0;
    }
    *result = 7;
    return 0;
}
static int blocker_fstat(void *ctx, void *handle, struct stat *st) {
    (void)ctx, (void)handle;
    memset(st, 0, sizeof(*st));
    st->st_mode = S_IFCHR;
    st->st_rdev = DRM_CELLOS_RENDER_RDEV;
    return 0;
}

static const struct cellos_fd_provider_ops blocker_ops = {
    .abi = CELLOS_FD_PROVIDER_ABI,
    .open = blocker_open,
    .close = blocker_close,
    .ioctl = blocker_ioctl,
    .fstat = blocker_fstat,
};

static void *blocking_call(void *arg) {
    int fd = *(int *)arg;
    long ret = ioctl(fd, BLOCKING_REQUEST, &blocker);
    return (void *)ret;
}

static void test_close_during_ioctl(void) {
    EXPECT(cellos_fd_provider_register("/test/", &blocker_ops, NULL) == 0);
    int fd = open("/test/node", O_RDWR);
    EXPECT(fd >= CELLOS_FD_PROVIDER_BASE);

    /* The request reaches the provider as 32 bits after posix_devctl's int. */
    int dummy = 0;
    EXPECT(ioctl(fd, 0xc0106400ul, &dummy) == 7);
    EXPECT(blocker.last_request == 0xc0106400ul);

    /* A non-`xe` render node is not a device. */
    drmDevicePtr device = NULL;
    EXPECT(drmGetDevice2(fd, 0, &device) == -ENODEV && device == NULL);

    pthread_t thread;
    EXPECT(pthread_create(&thread, NULL, blocking_call, &fd) == 0);
    pthread_mutex_lock(&blocker.lock);
    while (!blocker.inside)
        pthread_cond_wait(&blocker.cond, &blocker.lock);
    pthread_mutex_unlock(&blocker.lock);

    EXPECT(close(fd) == 0);
    EXPECT(blocker.closes == 0);
    EXPECT(host_registry.slots[fd - CELLOS_FD_PROVIDER_BASE].state == CFP_CLOSING);
    errno = 0;
    EXPECT(ioctl(fd, 2ul, &dummy) == -1 && errno == EBADF);

    pthread_mutex_lock(&blocker.lock);
    blocker.release = 1;
    pthread_cond_broadcast(&blocker.cond);
    pthread_mutex_unlock(&blocker.lock);
    void *ret = NULL;
    EXPECT(pthread_join(thread, &ret) == 0 && (long)ret == 7);
    EXPECT(blocker.closes == 1);
    EXPECT(slots_in_state(CFP_FREE) == CELLOS_FD_PROVIDER_SLOTS);
}

int main(void) {
    test_no_provider();
    test_vfs_fd();
    test_kernel_fd_in_provider_range();
    test_register_validation();
    test_register_fake();
    test_card0_enoent();

    int fd = open(DRM_CELLOS_RENDER_NODE, O_RDWR | O_CLOEXEC);
    test_render_node(fd);
    test_version_and_caps(fd);
    test_syncobj(fd);
    test_syncobj_namespaces(fd);
    test_enumeration(fd);
    test_fake_mmap_op();
    test_close(fd);

    test_slot_exhaustion();
    test_close_during_ioctl();

    if (failures) {
        printf("DRM-LITE-HOST: %d of %d checks failed\n", failures, checks);
        return 1;
    }
    printf("DRM-LITE-HOST: %d/%d checks pass\n", checks, checks);
    return 0;
}
