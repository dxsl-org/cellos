/* SPDX-License-Identifier: MPL-2.0 */
/* Test-only DRM render-node provider; behaviour in drm-cellos-fake.h. */

#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <vicell/fd_provider.h>

#include "drm-cellos-fake.h"
#include "drm-cellos.h"
#include "drm.h"
#include "xe_drm.h"

#define FAKE_SYNCOBJS 64
#define FAKE_CONFIG_PARAMS (DRM_XE_QUERY_CONFIG_MAX_EXEC_QUEUE_PRIORITY + 1)

struct fake_device {
    struct drm_cellos_fake_identity identity;
};

/* One per open: DRM handle namespaces are per file. */
struct fake_file {
    pthread_mutex_t lock;
    uint64_t syncobjs; /* bit i set: handle i + 1 is live */
};

/* Linux drm_copy_field: copy at most the caller's length, report the full one. */
static void copy_field(char *buf, __kernel_size_t *len, const char *value) {
    size_t full = strlen(value);
    size_t n = full < *len ? full : *len;
    if (n && buf)
        memcpy(buf, value, n);
    *len = full;
}

static int fake_version(struct drm_version *v) {
    v->version_major = 1;
    v->version_minor = 1;
    v->version_patchlevel = 0;
    copy_field(v->name, &v->name_len, "xe");
    copy_field(v->date, &v->date_len, "0");
    copy_field(v->desc, &v->desc_len, "Cellos fake xe provider");
    return 0;
}

static int fake_get_cap(struct drm_get_cap *cap) {
    if (cap->capability != DRM_CAP_SYNCOBJ && cap->capability != DRM_CAP_SYNCOBJ_TIMELINE)
        return EINVAL;
    cap->value = 1;
    return 0;
}

static int fake_query(const struct fake_device *dev, struct drm_xe_device_query *q) {
    if (q->extensions || q->reserved[0] || q->reserved[1])
        return EINVAL;
    if (q->query != DRM_XE_DEVICE_QUERY_CONFIG)
        return EINVAL;
    uint32_t size = sizeof(struct drm_xe_query_config) + FAKE_CONFIG_PARAMS * sizeof(uint64_t);
    if (q->size == 0) {
        q->size = size;
        return 0;
    }
    if (q->size != size || !q->data)
        return EINVAL;
    struct drm_xe_query_config *config = (struct drm_xe_query_config *)(uintptr_t)q->data;
    config->num_params = FAKE_CONFIG_PARAMS;
    config->pad = 0;
    config->info[DRM_XE_QUERY_CONFIG_REV_AND_DEVICE_ID] =
        ((uint64_t)dev->identity.revision << 16) | dev->identity.device_id;
    config->info[DRM_XE_QUERY_CONFIG_FLAGS] = 0;
    config->info[DRM_XE_QUERY_CONFIG_MIN_ALIGNMENT] = 4096;
    config->info[DRM_XE_QUERY_CONFIG_VA_BITS] = 48;
    config->info[DRM_XE_QUERY_CONFIG_MAX_EXEC_QUEUE_PRIORITY] = 1;
    return 0;
}

static int fake_syncobj_create(struct fake_file *file, struct drm_syncobj_create *args) {
    if (args->flags & ~(uint32_t)DRM_SYNCOBJ_CREATE_SIGNALED)
        return EINVAL;
    int err = ENOMEM;
    pthread_mutex_lock(&file->lock);
    for (unsigned i = 0; i < FAKE_SYNCOBJS; i++) {
        if (!(file->syncobjs & (1ull << i))) {
            file->syncobjs |= 1ull << i;
            args->handle = i + 1;
            err = 0;
            break;
        }
    }
    pthread_mutex_unlock(&file->lock);
    return err;
}

static int fake_syncobj_destroy(struct fake_file *file, const struct drm_syncobj_destroy *args) {
    if (args->pad || args->handle == 0 || args->handle > FAKE_SYNCOBJS)
        return EINVAL;
    uint64_t bit = 1ull << (args->handle - 1);
    int err = EINVAL;
    pthread_mutex_lock(&file->lock);
    if (file->syncobjs & bit) {
        file->syncobjs &= ~bit;
        err = 0;
    }
    pthread_mutex_unlock(&file->lock);
    return err;
}

static int fake_open(void *ctx, const char *path, int flags, void **handle) {
    (void)ctx;
    (void)flags;
    if (strcmp(path, DRM_CELLOS_RENDER_NODE) != 0)
        return ENOENT;
    struct fake_file *file = calloc(1, sizeof(*file));
    if (!file)
        return ENOMEM;
    pthread_mutex_init(&file->lock, NULL);
    *handle = file;
    return 0;
}

static int fake_close(void *ctx, void *handle) {
    (void)ctx;
    struct fake_file *file = handle;
    pthread_mutex_destroy(&file->lock);
    free(file);
    return 0;
}

static int fake_ioctl(void *ctx, void *handle, unsigned long request, void *arg, int *result) {
    (void)result;
    if (!arg)
        return EFAULT;
    switch (request) {
    case DRM_IOCTL_VERSION:
        return fake_version(arg);
    case DRM_IOCTL_GET_CAP:
        return fake_get_cap(arg);
    case DRM_IOCTL_XE_DEVICE_QUERY:
        return fake_query(ctx, arg);
    case DRM_IOCTL_SYNCOBJ_CREATE:
        return fake_syncobj_create(handle, arg);
    case DRM_IOCTL_SYNCOBJ_DESTROY:
        return fake_syncobj_destroy(handle, arg);
    default:
        return EINVAL;
    }
}

static int fake_fstat(void *ctx, void *handle, struct stat *st) {
    (void)ctx;
    (void)handle;
    memset(st, 0, sizeof(*st));
    st->st_mode = S_IFCHR | 0666;
    st->st_rdev = DRM_CELLOS_RENDER_RDEV;
    return 0;
}

static int fake_mmap(void *ctx, void *handle, void *hint, size_t size, int prot, int flags,
                     off_t offset, void **window) {
    (void)ctx;
    (void)handle;
    (void)offset;
    void *p = mmap(hint, size, prot, (flags & MAP_FIXED) | MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED)
        return errno;
    *window = p;
    return 0;
}

static int fake_munmap(void *ctx, void *handle, void *addr, size_t size) {
    (void)ctx;
    (void)handle;
    return munmap(addr, size) ? errno : 0;
}

static const struct cellos_fd_provider_ops fake_ops = {
    .abi = CELLOS_FD_PROVIDER_ABI,
    .open = fake_open,
    .close = fake_close,
    .ioctl = fake_ioctl,
    .fstat = fake_fstat,
    .mmap = fake_mmap,
    .munmap = fake_munmap,
};

static struct fake_device fake_device;
static int fake_claimed;

int drm_cellos_fake_provider_register(const struct drm_cellos_fake_identity *identity) {
    if (!identity)
        return EINVAL;
    /* The identity is written once, before the provider becomes reachable. */
    if (__atomic_exchange_n(&fake_claimed, 1, __ATOMIC_ACQ_REL))
        return EEXIST;
    fake_device.identity = *identity;
    return cellos_fd_provider_register(DRM_CELLOS_NODE_PREFIX, &fake_ops, &fake_device);
}
