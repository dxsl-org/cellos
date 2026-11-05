// SPDX-License-Identifier: MIT
// fd provider routing core, header-only so that the vicell sysdeps
// (generic/generic.cpp) and the Linux host tests (libs/drm-cellos/tests/host)
// compile the same code. The kernel route is a callback table: vicell syscalls
// in libc, a fake VFS in the host tests. Uses no libc function, so it is safe
// inside mlibc's sysdeps.
//
// Slot life cycle, all transitions under `lock`:
//   FREE -> OPENING (open reserves the slot, provider open runs unlocked)
//   OPENING -> OPEN (provider open succeeded) | FREE (it failed)
//   OPEN -> CLOSING (close; ops in flight hold `refs`)
//   CLOSING -> FREE (after provider close, run by whoever drops refs to 0)
// Only OPEN slots accept new ops, so provider close runs once, after the last
// op on the fd has returned.
#ifndef VICELL_FD_PROVIDER_CORE_H
#define VICELL_FD_PROVIDER_CORE_H

#include <errno.h>
#include <vicell/fd_provider.h>

#ifdef __cplusplus
extern "C" {
#endif

// The non-provider route. Each returns 0 or a positive errno value.
struct cellos_fd_kernel {
    int (*open)(const char *path, int flags, mode_t mode, int *fd);
    int (*close)(int fd);
    int (*fstat)(int fd, struct stat *st);
    void (*yield)(void);
};

enum { CFP_FREE = 0, CFP_OPENING, CFP_OPEN, CFP_CLOSING };

struct cellos_fd_core_provider {
    char prefix[CELLOS_FD_PROVIDER_PREFIX_MAX];
    size_t prefix_len;
    const struct cellos_fd_provider_ops *ops;
    void *ctx;
};

struct cellos_fd_core_slot {
    unsigned state;
    unsigned refs;
    unsigned provider;
    void *handle;
};

// Static storage: `kernel` set, every other field zero (unlocked, no
// providers, all slots FREE).
struct cellos_fd_registry {
    const struct cellos_fd_kernel *kernel;
    int lock;
    unsigned providers_len;
    struct cellos_fd_core_provider providers[CELLOS_FD_PROVIDER_MAX];
    struct cellos_fd_core_slot slots[CELLOS_FD_PROVIDER_SLOTS];
};

static inline void cellos_fd_core_lock(struct cellos_fd_registry *r) {
    while (__atomic_exchange_n(&r->lock, 1, __ATOMIC_ACQUIRE))
        r->kernel->yield();
}

static inline void cellos_fd_core_unlock(struct cellos_fd_registry *r) {
    __atomic_store_n(&r->lock, 0, __ATOMIC_RELEASE);
}

static inline int cellos_fd_core_is_provider_fd(int fd) {
    return fd >= CELLOS_FD_PROVIDER_BASE && fd < CELLOS_FD_PROVIDER_BASE + CELLOS_FD_PROVIDER_SLOTS;
}

// 1 when `s` starts with the first `n` bytes of `prefix`.
static inline int cellos_fd_core_starts_with(const char *s, const char *prefix, size_t n) {
    for (size_t i = 0; i < n; i++)
        if (s[i] != prefix[i])
            return 0;
    return 1;
}

static inline int cellos_fd_core_register(struct cellos_fd_registry *r, const char *prefix,
                                          const struct cellos_fd_provider_ops *ops, void *ctx) {
    if (!prefix || !ops || ops->abi != CELLOS_FD_PROVIDER_ABI || !ops->open || !ops->close
        || !ops->ioctl || !ops->fstat)
        return EINVAL;
    size_t len = 0;
    while (len < CELLOS_FD_PROVIDER_PREFIX_MAX && prefix[len])
        len++;
    if (len < 2 || len >= CELLOS_FD_PROVIDER_PREFIX_MAX || prefix[0] != '/'
        || prefix[len - 1] != '/')
        return EINVAL;

    int err = 0;
    cellos_fd_core_lock(r);
    for (unsigned i = 0; i < r->providers_len && !err; i++) {
        const struct cellos_fd_core_provider *p = &r->providers[i];
        size_t shorter = p->prefix_len < len ? p->prefix_len : len;
        if (cellos_fd_core_starts_with(prefix, p->prefix, shorter))
            err = EEXIST;
    }
    if (!err && r->providers_len == CELLOS_FD_PROVIDER_MAX)
        err = ENOSPC;
    if (!err) {
        struct cellos_fd_core_provider *p = &r->providers[r->providers_len];
        for (size_t i = 0; i <= len; i++)
            p->prefix[i] = prefix[i];
        p->prefix_len = len;
        p->ops = ops;
        p->ctx = ctx;
        // Pairs with the acquire load in cellos_fd_core_match, which reads
        // the table without the lock.
        __atomic_store_n(&r->providers_len, r->providers_len + 1, __ATOMIC_RELEASE);
    }
    cellos_fd_core_unlock(r);
    return err;
}

// Index of the provider whose prefix starts `path`, or -1. Registered entries
// never change, so reading below a snapshot of providers_len is race-free.
static inline int cellos_fd_core_match(struct cellos_fd_registry *r, const char *path) {
    unsigned n = __atomic_load_n(&r->providers_len, __ATOMIC_ACQUIRE);
    for (unsigned i = 0; i < n; i++) {
        const struct cellos_fd_core_provider *p = &r->providers[i];
        size_t j = 0;
        while (j < p->prefix_len && path[j] == p->prefix[j])
            j++;
        if (j == p->prefix_len)
            return (int)i;
    }
    return -1;
}

static inline int cellos_fd_core_open(struct cellos_fd_registry *r, const char *path, int flags,
                                      mode_t mode, int *fd) {
    if (!path)
        return EFAULT;
    int provider = cellos_fd_core_match(r, path);
    if (provider < 0) {
        int kfd = -1;
        int err = r->kernel->open(path, flags, mode, &kfd);
        if (err)
            return err;
        if (kfd >= CELLOS_FD_PROVIDER_BASE) {
            r->kernel->close(kfd);
            return EMFILE;
        }
        *fd = kfd;
        return 0;
    }

    int slot = -1;
    cellos_fd_core_lock(r);
    for (int i = 0; i < CELLOS_FD_PROVIDER_SLOTS; i++) {
        if (r->slots[i].state == CFP_FREE) {
            r->slots[i].state = CFP_OPENING;
            r->slots[i].refs = 0;
            r->slots[i].provider = (unsigned)provider;
            slot = i;
            break;
        }
    }
    cellos_fd_core_unlock(r);
    if (slot < 0)
        return EMFILE;

    const struct cellos_fd_core_provider *p = &r->providers[provider];
    void *handle = 0;
    int err = p->ops->open(p->ctx, path, flags, &handle);
    cellos_fd_core_lock(r);
    if (err) {
        r->slots[slot].state = CFP_FREE;
    } else {
        r->slots[slot].handle = handle;
        r->slots[slot].state = CFP_OPEN;
    }
    cellos_fd_core_unlock(r);
    if (err)
        return err;
    *fd = CELLOS_FD_PROVIDER_BASE + slot;
    return 0;
}

// Pins an OPEN provider slot for one op; EBADF if `fd` is not open.
static inline int cellos_fd_core_get(struct cellos_fd_registry *r, int fd,
                                     struct cellos_fd_core_slot **out) {
    struct cellos_fd_core_slot *s = &r->slots[fd - CELLOS_FD_PROVIDER_BASE];
    int err = 0;
    cellos_fd_core_lock(r);
    if (s->state == CFP_OPEN)
        s->refs++;
    else
        err = EBADF;
    cellos_fd_core_unlock(r);
    *out = s;
    return err;
}

// Runs provider close for a CLOSING slot that nobody pins, then frees it.
static inline int cellos_fd_core_release(struct cellos_fd_registry *r,
                                         struct cellos_fd_core_slot *s) {
    const struct cellos_fd_core_provider *p = &r->providers[s->provider];
    int err = p->ops->close(p->ctx, s->handle);
    cellos_fd_core_lock(r);
    s->handle = 0;
    s->state = CFP_FREE;
    cellos_fd_core_unlock(r);
    return err;
}

static inline void cellos_fd_core_put(struct cellos_fd_registry *r,
                                      struct cellos_fd_core_slot *s) {
    cellos_fd_core_lock(r);
    int release = --s->refs == 0 && s->state == CFP_CLOSING;
    cellos_fd_core_unlock(r);
    if (release)
        cellos_fd_core_release(r, s);
}

// The fd number is released at once; a provider close error is reported
// only when no op was in flight (otherwise the close runs later).
static inline int cellos_fd_core_close(struct cellos_fd_registry *r, int fd) {
    if (!cellos_fd_core_is_provider_fd(fd))
        return r->kernel->close(fd);
    struct cellos_fd_core_slot *s = &r->slots[fd - CELLOS_FD_PROVIDER_BASE];
    int err = 0, release = 0;
    cellos_fd_core_lock(r);
    if (s->state != CFP_OPEN) {
        err = EBADF;
    } else {
        s->state = CFP_CLOSING;
        release = s->refs == 0;
    }
    cellos_fd_core_unlock(r);
    if (release)
        err = cellos_fd_core_release(r, s);
    return err;
}

static inline int cellos_fd_core_fstat(struct cellos_fd_registry *r, int fd, struct stat *st) {
    if (!cellos_fd_core_is_provider_fd(fd))
        return r->kernel->fstat(fd, st);
    struct cellos_fd_core_slot *s;
    int err = cellos_fd_core_get(r, fd, &s);
    if (err)
        return err;
    const struct cellos_fd_core_provider *p = &r->providers[s->provider];
    err = p->ops->fstat(p->ctx, s->handle, st);
    cellos_fd_core_put(r, s);
    return err;
}

// `request` is a Linux ioctl number: 32 bits, so callers that pass it through
// an `int` (posix_devctl) and sign-extend still match.
static inline int cellos_fd_core_ioctl(struct cellos_fd_registry *r, int fd,
                                       unsigned long request, void *arg, int *result) {
    if (!cellos_fd_core_is_provider_fd(fd))
        return ENOTTY;
    struct cellos_fd_core_slot *s;
    int err = cellos_fd_core_get(r, fd, &s);
    if (err)
        return err;
    const struct cellos_fd_core_provider *p = &r->providers[s->provider];
    int value = 0;
    err = p->ops->ioctl(p->ctx, s->handle, request & 0xffffffffUL, arg, &value);
    cellos_fd_core_put(r, s);
    if (!err && result)
        *result = value;
    return err;
}

#ifdef __cplusplus
}
#endif

#endif // VICELL_FD_PROVIDER_CORE_H
