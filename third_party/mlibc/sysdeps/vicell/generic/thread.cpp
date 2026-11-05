// SPDX-License-Identifier: MIT
// Cellos (vicell) mlibc sysdeps: thread pointer, futexes and threads.
//
// A thread is a kernel task made by Spawn (5). The kernel gives it a user
// stack of its own (16 pages, kernel/src/task.rs stack_pages_for("thread")),
// the creator's address space, authority and TLS base, and one argument.
// Exit (60) from a non-root thread ends only that thread.
//
// The argument is a start block written by PrepareStack:
//   [0] entry  [1] user_arg  [2] Tcb*  [3] 0 = switch sp to the block, 1 = stay
// __vicell_start_thread (<arch>/thread-entry.S) reads it, optionally switches
// to the caller's stack and calls __mlibc_enter_thread.

#include <bits/ensure.h>
#include <errno.h>
#include <mlibc/all-sysdeps.hpp>
#include <mlibc/tcb.hpp>
#include <mlibc/threads.hpp>
#include <vicell/syscall.h>

extern "C" void __vicell_start_thread(void *start_block);

// Without a caller-supplied stack a thread runs on the kernel's Spawn stack
// until anonymous memory (runtime plan R3b) can back mlibc's 2 MiB default.
static constexpr size_t VI_SPAWN_STACK_BYTES = 16 * 4096;

// No syscall reports the caller's own task id. The root thread uses this value
// (inside mlibc's 30-bit futex owner mask); kernel tids are allocated upwards
// from small numbers and are never reused, so a Spawn tid cannot reach it.
static constexpr pid_t VI_ROOT_TID = 0x3fffffff;

extern "C" [[noreturn]] void __mlibc_enter_thread(void *entry, void *user_arg, Tcb *tcb) {
    // The creator publishes our tid after Spawn returns; wait for it.
    while (!__atomic_load_n(&tcb->tid, __ATOMIC_RELAXED))
        mlibc::sysdep<FutexWait>(&tcb->tid, 0, nullptr);
    if (mlibc::sysdep<TcbSet>(tcb))
        __ensure(!"sys_tcb_set() failed");
    tcb->invokeThreadFunc(entry, user_arg);
    mlibc::thread_exit(tcb->returnValue);
}

namespace mlibc {

// Install the thread pointer for `pointer` (mlibc's Tcb) with mlibc's Linux
// per-architecture offsets: RV64 and AArch64 use TLS variant I (thread pointer
// just past the Tcb), x86_64 variant II (FS points at the Tcb). The kernel owns
// the register and reinstalls it on every resume, so it is set only through
// SetTlsBase, never written directly.
int Sysdeps<TcbSet>::operator()(void *pointer) {
    uintptr_t tp = reinterpret_cast<uintptr_t>(pointer);
#if defined(__riscv)
    tp += sizeof(Tcb);
#elif defined(__aarch64__)
    tp += sizeof(Tcb) - 0x10;
#endif
    if (vicell_syscall(VI_SYS_SET_TLS_BASE, (long)tp, 0, 0, 0) == VI_SYSCALL_ERR)
        return EINVAL;
    return 0;
}

// Relative timeout → FutexWait ticks. The kernel's 0 means "forever", so a
// zero-length wait is answered here after one value check. One tick is added
// to the rounded-up count because the current tick is already partly spent:
// a timed wait may end late, never early.
int Sysdeps<FutexWait>::operator()(int *pointer, int expected, const struct timespec *time) {
    long ticks = 0;
    if (time) {
        if (time->tv_sec < 0 || time->tv_nsec < 0 || time->tv_nsec >= 1000000000L)
            return EINVAL;
        if (time->tv_sec == 0 && time->tv_nsec == 0)
            return __atomic_load_n(pointer, __ATOMIC_ACQUIRE) == expected ? ETIMEDOUT : EAGAIN;
        constexpr long max_secs = 0x7fffffffL / (1000000000L / VI_FUTEX_TICK_NS);
        if (time->tv_sec >= max_secs) {
            ticks = 0x7fffffffL;
        } else {
            ticks = time->tv_sec * (1000000000L / VI_FUTEX_TICK_NS)
                    + (time->tv_nsec + VI_FUTEX_TICK_NS - 1) / VI_FUTEX_TICK_NS + 1;
        }
    }
    long ret = vicell_syscall(VI_SYS_FUTEX_WAIT, (long)pointer, (long)(unsigned)expected, ticks, 0);
    switch (ret) {
    case VI_FUTEX_WOKEN: return 0;
    case VI_FUTEX_MISMATCH: return EAGAIN;
    case VI_FUTEX_TIMEOUT: return ETIMEDOUT;
    default: return EINVAL;
    }
}

int Sysdeps<FutexWake>::operator()(int *pointer, bool all) {
    if (vicell_syscall(VI_SYS_FUTEX_WAKE, (long)pointer, all ? 0 : 1, 0, 0) == VI_SYSCALL_ERR)
        return EINVAL;
    return 0;
}

void Sysdeps<Yield>::operator()() {
    vicell_syscall(VI_SYS_YIELD, 0, 0, 0, 0);
}

pid_t Sysdeps<GetTid>::operator()() {
    return VI_ROOT_TID;
}

int Sysdeps<PrepareStack>::operator()(void **stack, void *entry, void *user_arg, void *tcb,
                                      size_t *stack_size, size_t *guard_size, void **stack_base) {
    *guard_size = 0;
    uintptr_t *block;
    if (*stack) {
        // Caller's stack: the block sits at its 16-byte-aligned top, and the
        // thread switches sp to it.
        if (*stack_size < 4096)
            return EINVAL;
        *stack_base = *stack;
        uintptr_t top = (reinterpret_cast<uintptr_t>(*stack) + *stack_size) & ~uintptr_t(15);
        block = reinterpret_cast<uintptr_t *>(top) - 4;
        block[3] = 0;
    } else {
        void *mem;
        if (int e = sysdep<AnonAllocate>(4 * sizeof(uintptr_t), &mem))
            return e;
        block = static_cast<uintptr_t *>(mem);
        block[3] = 1;
        *stack_base = nullptr;
        *stack_size = VI_SPAWN_STACK_BYTES;
    }
    block[0] = reinterpret_cast<uintptr_t>(entry);
    block[1] = reinterpret_cast<uintptr_t>(user_arg);
    block[2] = reinterpret_cast<uintptr_t>(tcb);
    *stack = block;
    return 0;
}

int Sysdeps<Clone>::operator()(void *, pid_t *pid_out, void *stack) {
    long tid = vicell_syscall(VI_SYS_SPAWN, (long)&__vicell_start_thread, (long)stack, 0, 0);
    if (tid == VI_SYSCALL_ERR || tid <= 0)
        return EAGAIN;
    *pid_out = (pid_t)tid;
    return 0;
}

// Ends only the calling thread; the root thread's Exit retires the Cell.
void Sysdeps<ThreadExit>::operator()() {
    vicell_syscall(VI_SYS_EXIT, 0, 0, 0, 0);
    __builtin_unreachable();
}

} // namespace mlibc
