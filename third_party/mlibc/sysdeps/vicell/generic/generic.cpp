// SPDX-License-Identifier: MIT
// Cellos (vicell) mlibc sysdeps, one `Sysdeps<Tag>::operator()` per tag listed in
// include/mlibc/sysdeps.hpp.
//
// Build with: -fno-exceptions -fno-rtti -Os
// Depends on: vicell/syscall.h (inline asm, no libc dependency)
//
// ABI contract (do not change without updating syscall.h and syscall.rs):
//   riscv64: a7=id, a0-a3=args, a0=ret, ecall
//   aarch64: x0=id, x1-x4=args, x0=ret, svc #0     <- non-Linux
//   x86_64:  rax=id, rdi/rsi/rdx/r10=args, rax=ret, syscall
//
// Return convention: 0 on success, otherwise an errno value that mlibc stores
// in `errno` for its caller.
//
// Arg order for Cellos Open (must mirror posix/sysio.rs::_open exactly):
//   vicell_syscall(101, path_ptr, path_len, flags, mode)

#include <abi-bits/fcntl.h>
#include <mlibc/all-sysdeps.hpp>
#include <mlibc/tcb.hpp>
#include <vicell/syscall.h>

// ─── Local helpers (avoid circular dependency on libc itself) ────────────

static size_t vi_strlen(const char *s) {
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

static void vi_memset(void *dst, int val, size_t n) {
    unsigned char *p = (unsigned char *)dst;
    while (n--) *p++ = (unsigned char)val;
}

// ─── 16 MiB static bump arena (backs mlibc's slab allocator / AnonAllocate) ──
// Interim until anonymous memory (runtime plan R3b, AnonMemory = 440). With the
// `posix` option, start-up alone touches enough slab size classes (256 KiB
// superblock each) to exhaust 4 MiB. The arena is .bss: every mlibc Cell pays
// its 16 MiB at load.
static constexpr size_t ARENA_SIZE = 16 * 1024 * 1024;
static unsigned char arena[ARENA_SIZE];
static size_t arena_offset = 0;

namespace mlibc {

// Log an ASCII message through the kernel Log channel.
void Sysdeps<LibcLog>::operator()(const char *message) {
    vicell_syscall(VI_SYS_LOG, (long)message, (long)vi_strlen(message), 0, 0);
}

// Log "PANIC" then exit with status 1 (mlibc internal assertion failures).
void Sysdeps<LibcPanic>::operator()() {
    sysdep<LibcLog>("mlibc PANIC");
    vicell_syscall(VI_SYS_EXIT, 1, 0, 0, 0);
    __builtin_unreachable();
}

void Sysdeps<Exit>::operator()(int status) {
    vicell_syscall(VI_SYS_EXIT, (long)status, 0, 0, 0);
    __builtin_unreachable();
}

int Sysdeps<Open>::operator()(const char *pathname, int flags, mode_t mode, int *fd) {
    long ret = vicell_syscall(VI_SYS_OPEN, (long)pathname, (long)vi_strlen(pathname),
                              (long)flags, (long)(unsigned)mode);
    if (ret == VI_SYSCALL_ERR) return ENOENT;
    *fd = (int)ret;
    return 0;
}

int Sysdeps<Read>::operator()(int fd, void *buf, size_t count, ssize_t *bytes_read) {
    long ret = vicell_syscall(VI_SYS_READ, (long)fd, (long)buf, (long)count, 0);
    if (ret == VI_SYSCALL_ERR) return EIO;
    *bytes_read = (ssize_t)ret;
    return 0;
}

int Sysdeps<Write>::operator()(int fd, const void *buf, size_t count, ssize_t *bytes_written) {
    long ret = vicell_syscall(VI_SYS_WRITE, (long)fd, (long)buf, (long)count, 0);
    if (ret == VI_SYSCALL_ERR) return EIO;
    *bytes_written = (ssize_t)ret;
    return 0;
}

int Sysdeps<Close>::operator()(int fd) {
    if (vicell_syscall(VI_SYS_CLOSE, (long)fd, 0, 0, 0) == VI_SYSCALL_ERR) return EBADF;
    return 0;
}

int Sysdeps<Seek>::operator()(int fd, off_t offset, int whence, off_t *new_offset) {
    long ret = vicell_syscall(VI_SYS_SEEK, (long)fd, (long)offset, (long)whence, 0);
    if (ret == VI_SYSCALL_ERR) return ESPIPE;
    *new_offset = (off_t)ret;
    return 0;
}

// Monotonic source per architecture (kernel GetTime, kernel/src/task/syscall.rs):
// riscv64 op 0 = 10 MHz mtime (the kernel assumes 10 MHz too); aarch64 op 1 =
// milliseconds, because the generic-timer frequency varies by board; x86_64
// op 0 = nanoseconds.
#if defined(__riscv)
static constexpr long VI_MONO_OP = VI_GETTIME_TICKS;
static constexpr long long VI_MONO_NS = 100;
#elif defined(__aarch64__)
static constexpr long VI_MONO_OP = 1;
static constexpr long long VI_MONO_NS = 1000000;
#else
static constexpr long VI_MONO_OP = VI_GETTIME_TICKS;
static constexpr long long VI_MONO_NS = 1;
#endif

// CLOCK_REALTIME (0): GetTime op 2, epoch nanoseconds.
// CLOCK_MONOTONIC (1): GetTime op 0, converted to nanoseconds.
int Sysdeps<ClockGet>::operator()(int clock, time_t *secs, long *nanos) {
    long long ns;
    if (clock == 0) {
        ns = vicell_syscall(VI_SYS_GETTIME, VI_GETTIME_NS, 0, 0, 0);
    } else if (clock == 1) {
        ns = vicell_syscall(VI_SYS_GETTIME, VI_MONO_OP, 0, 0, 0) * VI_MONO_NS;
    } else {
        return EINVAL;
    }
    if (ns < 0) return EIO;
    *secs = (time_t)(ns / 1000000000LL);
    *nanos = (long)(ns % 1000000000LL);
    return 0;
}

// 16-byte aligned, zeroed memory from the static arena; never reclaimed.
// Threads allocate concurrently, so the bump pointer advances by CAS.
int Sysdeps<AnonAllocate>::operator()(size_t size, void **pointer) {
    size_t aligned = (size + 15) & ~(size_t)15;
    size_t offset = __atomic_load_n(&arena_offset, __ATOMIC_RELAXED);
    do {
        if (aligned > ARENA_SIZE - offset) {
            sysdep<LibcLog>("mlibc: anon arena exhausted (16 MiB limit)");
            return ENOMEM;
        }
    } while (!__atomic_compare_exchange_n(&arena_offset, &offset, offset + aligned, true,
                                          __ATOMIC_RELAXED, __ATOMIC_RELAXED));
    void *ptr = arena + offset;
    vi_memset(ptr, 0, size);
    *pointer = ptr;
    return 0;
}

int Sysdeps<AnonFree>::operator()(void *, size_t) {
    return 0;
}

// MAP_ANONYMOUS only, served by AnonAllocate; any other mapping is EINVAL.
int Sysdeps<VmMap>::operator()(void *, size_t size, int, int flags, int, off_t, void **window) {
    if (flags & MAP_ANONYMOUS) return sysdep<AnonAllocate>(size, window);
    return EINVAL;
}

int Sysdeps<VmUnmap>::operator()(void *pointer, size_t size) {
    return sysdep<AnonFree>(pointer, size);
}

// fds 0-2 are terminals (0), so stdio stays line-buffered; others ENOTTY.
int Sysdeps<Isatty>::operator()(int fd) {
    return (fd >= 0 && fd <= 2) ? 0 : ENOTTY;
}

// Cellos Fstat v1 wire record (kernel ViFstatV1, 32 bytes).
struct vi_fstat_v1 {
    uint32_t kind;
    uint32_t access;
    uint64_t size;
    uint64_t reserved[2];
};

static int vi_fstat(int fd, struct stat *st) {
    struct vi_fstat_v1 wire = {0, 0, 0, {0, 0}};
    long ret = vicell_syscall(VI_SYS_FSTAT, (long)fd, (long)&wire, sizeof(wire), 0);
    if (ret != (long)sizeof(wire)) return EBADF;
    vi_memset(st, 0, sizeof(*st));
    switch (wire.kind) {
    case 1: st->st_mode = S_IFCHR; break;
    case 2: st->st_mode = S_IFREG; break;
    case 3: st->st_mode = S_IFDIR; break;
    default: return EIO;
    }
    st->st_size = (off_t)wire.size;
    return 0;
}

// fstat (fd), stat (path, or AT_FDCWD + path). Only st_mode and st_size are set.
int Sysdeps<Stat>::operator()(fsfd_target target, int fd, const char *path, int, struct stat *st) {
    if (!st) return EINVAL;
    if (target == fsfd_target::fd) return vi_fstat(fd, st);
    bool by_path = target == fsfd_target::path
                   || (target == fsfd_target::fd_path && fd == AT_FDCWD);
    if (!by_path || !path) return EINVAL;
    int opened = -1;
    if (int err = sysdep<Open>(path, 0, (mode_t)0, &opened)) return err;
    int ret = vi_fstat(opened, st);
    sysdep<Close>(opened);
    return ret;
}

} // namespace mlibc
