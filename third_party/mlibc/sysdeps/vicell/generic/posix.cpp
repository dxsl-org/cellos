// SPDX-License-Identifier: MIT
// Cellos (vicell) mlibc sysdeps for the `posix` option: sleep, process
// identity, sysconf and entropy. Anything not listed in include/mlibc/sysdeps.hpp
// reaches callers as ENOSYS through mlibc::sysdep_or_enosys.

#include <errno.h>
#include <mlibc/all-sysdeps.hpp>
#include <unistd.h>
#include <vicell/syscall.h>

namespace {

// ViMemInfoV1 (libs/api/src/abi/syscall.rs): global frame-allocator snapshot.
struct vi_mem_info_v1 {
    uint64_t total_frames;
    uint64_t used_frames;
    uint64_t free_frames;
    uint64_t page_size;
};

long long monotonic_ns() {
    time_t secs;
    long nanos;
    if (mlibc::sysdep<ClockGet>(1, &secs, &nanos))
        return -1;
    return (long long)secs * 1000000000LL + nanos;
}

} // namespace

namespace mlibc {

// Sleeps by a timed FutexWait on a word nobody wakes, re-arming until the
// monotonic clock has passed the deadline: never returns early, may return
// up to one 10 ms tick late. Not interruptible (no signals), so the remaining
// time is always 0.
int Sysdeps<Sleep>::operator()(time_t *secs, long *nanos) {
    if (*secs < 0 || *nanos < 0 || *nanos >= 1000000000L)
        return EINVAL;
    long long start = monotonic_ns();
    if (start < 0)
        return EIO;
    long long deadline = start + (long long)*secs * 1000000000LL + *nanos;
    int word = 0;
    for (long long now = start; now < deadline; now = monotonic_ns()) {
        if (now < 0)
            return EIO;
        long long left = deadline - now;
        struct timespec rel = {(time_t)(left / 1000000000LL), (long)(left % 1000000000LL)};
        int e = sysdep<FutexWait>(&word, 0, &rel);
        if (e && e != ETIMEDOUT)
            return e;
    }
    *secs = 0;
    *nanos = 0;
    return 0;
}

// A Cell is one process. getpid() reports this fixed value (POSIX ledger,
// Tier-B `GetPid`); there is no process hierarchy to identify.
pid_t Sysdeps<GetPid>::operator()() {
    return 1;
}

// EINVAL hands a name back to mlibc's defaults (_SC_PAGE_SIZE, _SC_OPEN_MAX, …).
int Sysdeps<Sysconf>::operator()(int num, long *ret) {
    switch (num) {
    case _SC_NPROCESSORS_ONLN:
    case _SC_NPROCESSORS_CONF:
        // No syscall reports the hart count to a Cell.
        *ret = 1;
        return 0;
    case _SC_PHYS_PAGES:
    case _SC_AVPHYS_PAGES: {
        // MemInfo is opt-in telemetry (allowlist bit); without it the answer
        // is -1/ENOSYS rather than a made-up page count.
        vi_mem_info_v1 info = {};
        long n = vicell_syscall(VI_SYS_MEM_INFO, (long)&info, sizeof(info), 0, 0);
        if (n != (long)sizeof(info) || info.page_size == 0)
            return ENOSYS;
        uint64_t frames = num == _SC_PHYS_PAGES ? info.total_frames : info.free_frames;
        *ret = (long)(frames * (info.page_size / 4096));
        return 0;
    }
    default:
        return EINVAL;
    }
}

// GetRandom returns at most 64 bytes per call and 0 without an RNG device.
int Sysdeps<GetEntropy>::operator()(void *buffer, size_t length) {
    if (length > 256)
        return EIO;
    auto *out = static_cast<unsigned char *>(buffer);
    while (length) {
        size_t chunk = length < 64 ? length : 64;
        long n = vicell_syscall(VI_SYS_GET_RANDOM, (long)out, (long)chunk, 0, 0);
        if (n <= 0 || n == VI_SYSCALL_ERR || (size_t)n > chunk)
            return EIO;
        out += n;
        length -= (size_t)n;
    }
    return 0;
}

} // namespace mlibc
