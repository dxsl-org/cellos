// SPDX-License-Identifier: MIT
// ViCell mlibc sysdeps — all 17 mandatory sysdeps + isatty.
//
// Build with: -fno-exceptions -fno-rtti -Os
// Depends on: vicell/syscall.h (inline asm, no libc dependency)
//
// ABI contract (do not change without updating syscall.h and syscall.rs):
//   riscv64: a7=id, a0-a3=args, a0=ret, ecall
//   aarch64: x0=id, x1-x4=args, x0=ret, svc #0  ← non-Linux
//
// Arg order for ViCell Open (must mirror posix/sysio.rs::_open exactly):
//   vicell_syscall(101, path_ptr, path_len, flags, mode)

#include <mlibc/sysdeps.hpp>
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

// ─── 4 MB static bump arena (backs frg::slab_allocator / AnonAllocate) ──

static constexpr size_t ARENA_SIZE = 4 * 1024 * 1024;
static unsigned char arena[ARENA_SIZE];
static size_t arena_offset = 0;

// ─── Mandatory sysdep implementations ────────────────────────────────────

namespace mlibc {

// Log an ASCII message through the kernel Log channel.
void sys_libc_log(const char *message) {
    size_t len = vi_strlen(message);
    vicell_syscall(VI_SYS_LOG, (long)message, (long)len, 0, 0);
}

// Log "PANIC" then exit — mlibc calls this on internal assertion failures.
[[noreturn]] void sys_libc_panic() {
    sys_libc_log("mlibc PANIC");
    vicell_syscall(VI_SYS_EXIT, 1, 0, 0, 0);
    __builtin_unreachable();
}

// Clean process exit.
[[noreturn]] void sys_exit(int status) {
    vicell_syscall(VI_SYS_EXIT, (long)status, 0, 0, 0);
    __builtin_unreachable();
}

// Set the Thread Control Block pointer (used by mlibc TLS).
// riscv64: tp register; aarch64: tpidr_el0
int sys_tcb_set(void *pointer) {
#if defined(__riscv)
    __asm__ volatile("mv tp, %0" :: "r"(pointer) : "memory");
#elif defined(__aarch64__)
    __asm__ volatile("msr tpidr_el0, %0" :: "r"(pointer) : "memory");
#else
    (void)pointer;
#endif
    return 0;
}

// Futex wait — spin-loop stub: single-threaded G2, pthreads disabled.
// Block until *pointer != expected (we yield to avoid monopolising CPU).
int sys_futex_wait(int *pointer, int expected, const struct timespec *) {
    while (__atomic_load_n(pointer, __ATOMIC_ACQUIRE) == expected) {
        // In a single-threaded SAS cell this loop is unreachable in practice,
        // but provide a Yield syscall to be polite if it ever spins.
        vicell_syscall(104, 0, 0, 0, 0); // ViSyscall::Yield = 104
    }
    return 0;
}

// Futex wake — no-op: we have no waiting threads in G2.
int sys_futex_wake(int *) {
    return 0;
}

// Open a file.  Arg order matches posix/sysio.rs::_open exactly:
//   kernel Open(a0=path_ptr, a1=path_len, a2=flags, a3=mode) → fd or usize::MAX
int sys_open(const char *pathname, int flags, mode_t mode, int *fd) {
    size_t len = vi_strlen(pathname);
    long ret = vicell_syscall(VI_SYS_OPEN, (long)pathname, (long)len,
                              (long)flags, (long)(unsigned)mode);
    if (ret == VI_SYSCALL_ERR) return ENOENT;
    *fd = (int)ret;
    return 0;
}

// Read bytes from a file descriptor.
int sys_read(int fdi, void *buf, size_t count, ssize_t *bytes_read) {
    long ret = vicell_syscall(VI_SYS_READ, (long)fdi, (long)buf,
                              (long)count, 0);
    if (ret == VI_SYSCALL_ERR) return EIO;
    *bytes_read = (ssize_t)ret;
    return 0;
}

// Write bytes to a file descriptor.
int sys_write(int fdi, const void *buf, size_t count, ssize_t *bytes_written) {
    long ret = vicell_syscall(VI_SYS_WRITE, (long)fdi, (long)buf,
                              (long)count, 0);
    if (ret == VI_SYSCALL_ERR) return EIO;
    *bytes_written = (ssize_t)ret;
    return 0;
}

// Close a file descriptor.
int sys_close(int fdi) {
    long ret = vicell_syscall(VI_SYS_CLOSE, (long)fdi, 0, 0, 0);
    if (ret == VI_SYSCALL_ERR) return EBADF;
    return 0;
}

// Seek to position in a file.
int sys_seek(int fdi, off_t offset, int whence, off_t *new_offset) {
    long ret = vicell_syscall(VI_SYS_SEEK, (long)fdi, (long)offset,
                              (long)whence, 0);
    if (ret == VI_SYSCALL_ERR) return ESPIPE;
    *new_offset = (off_t)ret;
    return 0;
}

// Get wall clock or monotonic time.
//   CLOCK_REALTIME  (0): GetTime op=2 → epoch nanoseconds from RTC
//   CLOCK_MONOTONIC (1): GetTime op=0 → 10 MHz ticks × 100 = nanoseconds
//   Other: EINVAL
int sys_clock_get(int clock, time_t *secs, long *nanos) {
    long ns;
    if (clock == 0) {
        // CLOCK_REALTIME: op=2 returns epoch nanoseconds
        ns = vicell_syscall(VI_SYS_GETTIME, VI_GETTIME_NS, 0, 0, 0);
    } else if (clock == 1) {
        // CLOCK_MONOTONIC: op=0 returns 10 MHz ticks; convert to ns
        long ticks = vicell_syscall(VI_SYS_GETTIME, VI_GETTIME_TICKS, 0, 0, 0);
        ns = ticks * 100LL; // 10 MHz tick = 100 ns
    } else {
        return EINVAL;
    }
    if (ns < 0) return EIO;
    *secs  = (time_t)(ns / 1000000000LL);
    *nanos = (long)(ns  % 1000000000LL);
    return 0;
}

// Allocate anonymous memory from the static bump arena.
// AnonFree is a no-op — the arena is not reclaimed in G2.
// Overflow returns ENOMEM and logs a message via kernel.
int sys_anon_allocate(size_t size, void **pointer) {
    // Round up to 16-byte alignment
    size_t aligned = (size + 15) & ~(size_t)15;
    if (arena_offset + aligned > ARENA_SIZE) {
        sys_libc_log("mlibc: anon arena exhausted (4 MB limit)");
        return ENOMEM;
    }
    void *ptr = arena + arena_offset;
    arena_offset += aligned;
    vi_memset(ptr, 0, size);
    *pointer = ptr;
    return 0;
}

int sys_anon_free(void *, size_t) {
    // Bump allocator: freeing individual regions is unsupported in G2.
    return 0;
}

// vm_map: MAP_ANONYMOUS → anon_allocate; all other flags → EINVAL.
int sys_vm_map(void *, size_t size, int, int flags, int, off_t, void **window) {
    if (flags & MAP_ANONYMOUS) {
        return sys_anon_allocate(size, window);
    }
    return EINVAL;
}

int sys_vm_unmap(void *pointer, size_t size) {
    return sys_anon_free(pointer, size);
}

// isatty: fd 0-2 are stdin/stdout/stderr — report as terminals so mlibc
// uses line-buffered I/O instead of full-buffered (which would never flush).
int sys_isatty(int fd) {
    return (fd >= 0 && fd <= 2) ? 1 : 0;
}

} // namespace mlibc
