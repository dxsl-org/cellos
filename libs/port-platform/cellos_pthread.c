/* SPDX-License-Identifier: MPL-2.0 */
#include "cellos_pthread.h"

#include "cellos_syscall.h"

#include <stddef.h>
#include <stdint.h>

#define CELLOS_SYSCALL_EXIT 60u
#define CELLOS_SYSCALL_WAIT 8u
#define CELLOS_SYSCALL_SPAWN 5u
#define CELLOS_SYSCALL_YIELD 104u
#define CELLOS_SYSCALL_FUTEX_WAIT 17u
#define CELLOS_SYSCALL_FUTEX_WAKE 18u
#define CELLOS_SYSCALL_SET_TLS_BASE 9u

#define CELLOS_FUTEX_WAKE_ALL 0u
#define CELLOS_THREAD_FREE 0u
#define CELLOS_THREAD_STARTING 1u
#define CELLOS_THREAD_RUNNING 2u
#define CELLOS_THREAD_EXITED 3u

struct cellos_thread_slot {
    uint32_t state;
    uint32_t joined;
    uint32_t detached;
    uintptr_t tid;
    void *(*start_routine)(void *);
    void *arg;
    void *result;
    const void *tls_values[CELLOS_PTHREAD_KEYS_MAX];
    uintptr_t tcb[64] __attribute__((aligned(64)));
};

struct cellos_pthread_key_entry {
    uint32_t allocated;
    void (*destructor)(void *);
};

static struct cellos_thread_slot cellos_threads[CELLOS_PTHREAD_MAX_THREADS];
static uint32_t cellos_thread_slots_lock;

static struct cellos_pthread_key_entry cellos_pthread_keys[CELLOS_PTHREAD_KEYS_MAX];
static const void *cellos_main_tls_values[CELLOS_PTHREAD_KEYS_MAX];
static uintptr_t cellos_main_tcb[64] __attribute__((aligned(64)));
static uint32_t cellos_main_tls_initialized = 0;
static void cellos_yield(void) {
    (void)cellos_syscall4(CELLOS_SYSCALL_YIELD, 0, 0, 0, 0);
}

static void cellos_futex_wait(uint32_t *word, uint32_t expected) {
    (void)cellos_syscall4(CELLOS_SYSCALL_FUTEX_WAIT, (uintptr_t)word, expected, 0, 0);
}

static void cellos_futex_wake(uint32_t *word, uint32_t count) {
    (void)cellos_syscall4(CELLOS_SYSCALL_FUTEX_WAKE, (uintptr_t)word, count, 0, 0);
}

/*
 * Wait is the kernel's terminal-lifecycle boundary. If the scheduler already
 * reaped this non-reused TID, it has necessarily crossed that same boundary.
 */
static void cellos_wait_for_task_exit(uintptr_t tid) {
    (void)cellos_syscall4(CELLOS_SYSCALL_WAIT, tid, 0, 0, 0);
}

static void cellos_lock_slots(void) {
    for (;;) {
        uint32_t expected = 0;
        if (__atomic_compare_exchange_n(&cellos_thread_slots_lock, &expected, 1, 0,
                                        __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
            return;
        }
        cellos_yield();
    }
}

static void cellos_unlock_slots(void) {
    __atomic_store_n(&cellos_thread_slots_lock, 0, __ATOMIC_RELEASE);
}
static struct cellos_thread_slot *cellos_current_slot(void) {
    uintptr_t slot_ptr = 0;
#if defined(__x86_64__)
    __asm__ volatile("mov %%fs:8, %0" : "=r"(slot_ptr));
#elif defined(__aarch64__)
    uintptr_t base = 0;
    __asm__ volatile("mrs %0, tpidr_el0" : "=r"(base));
    if (base != 0) {
        slot_ptr = ((uintptr_t *)base)[1];
    }
#elif defined(__riscv)
    uintptr_t base = 0;
    __asm__ volatile("mv %0, tp" : "=r"(base));
    if (base != 0) {
        slot_ptr = ((uintptr_t *)base)[1];
    }
#endif
    return (struct cellos_thread_slot *)slot_ptr;
}

static const void **cellos_current_tls_values(void) {
    struct cellos_thread_slot *slot = cellos_current_slot();
    if (slot != NULL) {
        return slot->tls_values;
    }
    if (!__atomic_load_n(&cellos_main_tls_initialized, __ATOMIC_ACQUIRE)) {
        cellos_main_tcb[0] = (uintptr_t)&cellos_main_tcb[0];
        cellos_main_tcb[1] = 0;
        (void)cellos_syscall4(CELLOS_SYSCALL_SET_TLS_BASE, (uintptr_t)&cellos_main_tcb[0], 0, 0, 0);
        __atomic_store_n(&cellos_main_tls_initialized, 1, __ATOMIC_RELEASE);
    }
    return cellos_main_tls_values;
}

static void cellos_thread_entry(uintptr_t raw_slot) {
    struct cellos_thread_slot *slot = (struct cellos_thread_slot *)raw_slot;
    void *result;

    slot->tcb[0] = (uintptr_t)&slot->tcb[0];
    slot->tcb[1] = (uintptr_t)slot;
    (void)cellos_syscall4(CELLOS_SYSCALL_SET_TLS_BASE, (uintptr_t)&slot->tcb[0], 0, 0, 0);

    __atomic_store_n(&slot->state, CELLOS_THREAD_RUNNING, __ATOMIC_RELEASE);
    result = slot->start_routine(slot->arg);
    slot->result = result;

    for (uint32_t k = 0; k < CELLOS_PTHREAD_KEYS_MAX; ++k) {
        if (slot->tls_values[k] != NULL && cellos_pthread_keys[k].allocated && cellos_pthread_keys[k].destructor != NULL) {
            void *val = (void *)slot->tls_values[k];
            slot->tls_values[k] = NULL;
            cellos_pthread_keys[k].destructor(val);
        }
    }

    __atomic_store_n(&slot->state, CELLOS_THREAD_EXITED, __ATOMIC_RELEASE);
    if (__atomic_load_n(&slot->detached, __ATOMIC_ACQUIRE)) {
        cellos_lock_slots();
        slot->start_routine = NULL;
        slot->arg = NULL;
        slot->result = NULL;
        slot->tid = 0;
        slot->detached = 0;
        __atomic_store_n(&slot->state, CELLOS_THREAD_FREE, __ATOMIC_RELEASE);
        cellos_unlock_slots();
    } else {
        cellos_futex_wake(&slot->state, CELLOS_FUTEX_WAKE_ALL);
    }
    (void)cellos_syscall4(CELLOS_SYSCALL_EXIT, 0, 0, 0, 0);
    for (;;) {
        cellos_yield();
    }
}

int pthread_create(pthread_t *thread, const pthread_attr_t *attr,
                   void *(*start_routine)(void *), void *arg) {
    struct cellos_thread_slot *slot = NULL;
    uintptr_t tid;
    uint32_t index;

    if (thread == NULL || start_routine == NULL || attr != NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }

    cellos_lock_slots();
    for (index = 0; index < CELLOS_PTHREAD_MAX_THREADS; ++index) {
        if (__atomic_load_n(&cellos_threads[index].state, __ATOMIC_RELAXED) == CELLOS_THREAD_FREE) {
            slot = &cellos_threads[index];
            slot->joined = 0;
            slot->detached = 0;
            slot->tid = 0;
            slot->start_routine = start_routine;
            slot->arg = arg;
            slot->result = NULL;
            for (uint32_t k = 0; k < CELLOS_PTHREAD_KEYS_MAX; ++k) {
                slot->tls_values[k] = NULL;
            }
            __atomic_store_n(&slot->state, CELLOS_THREAD_STARTING, __ATOMIC_RELEASE);
            break;
        }
    }
    cellos_unlock_slots();

    if (slot == NULL) {
        return CELLOS_PTHREAD_EAGAIN;
    }

    tid = (uintptr_t)cellos_syscall4(CELLOS_SYSCALL_SPAWN,
                                     (uintptr_t)cellos_thread_entry,
                                     (uintptr_t)slot, 0, 0);
    if (tid == 0 || tid == UINTPTR_MAX) {
        cellos_lock_slots();
        __atomic_store_n(&slot->state, CELLOS_THREAD_FREE, __ATOMIC_RELEASE);
        cellos_unlock_slots();
        return CELLOS_PTHREAD_EAGAIN;
    }

    slot->tid = tid;
    *thread = index + 1u;
    return 0;
}

int pthread_join(pthread_t thread, void **retval) {
    struct cellos_thread_slot *slot;
    uint32_t expected_joined = 0;
    uint32_t state;

    if (thread == 0 || thread > CELLOS_PTHREAD_MAX_THREADS) {
        return CELLOS_PTHREAD_EINVAL;
    }
    slot = &cellos_threads[thread - 1u];
    if (__atomic_load_n(&slot->detached, __ATOMIC_ACQUIRE)) {
        return CELLOS_PTHREAD_EINVAL;
    }
    if (!__atomic_compare_exchange_n(&slot->joined, &expected_joined, 1, 0,
                                     __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
        return CELLOS_PTHREAD_EINVAL;
    }

    for (;;) {
        state = __atomic_load_n(&slot->state, __ATOMIC_ACQUIRE);
        if (state == CELLOS_THREAD_EXITED) {
            break;
        }
        if (state == CELLOS_THREAD_FREE) {
            __atomic_store_n(&slot->joined, 0, __ATOMIC_RELEASE);
            return CELLOS_PTHREAD_EINVAL;
        }
        cellos_futex_wait(&slot->state, state);
    }

    /*
     * EXITED publishes the return value, but the worker can still be executing
     * between its futex wake and Syscall::Exit. Do not release this slot until
     * the kernel confirms terminal task lifecycle.
     */
    cellos_wait_for_task_exit(slot->tid);

    if (retval != NULL) {
        *retval = slot->result;
    }
    cellos_lock_slots();
    slot->start_routine = NULL;
    slot->arg = NULL;
    slot->result = NULL;
    slot->tid = 0;
    /*
     * Publish FREE before dropping the join claim. A racing joiner then either
     * sees the existing claim or observes FREE and returns EINVAL; it cannot
     * consume a result already released by this joiner.
     */
    __atomic_store_n(&slot->state, CELLOS_THREAD_FREE, __ATOMIC_RELEASE);
    __atomic_store_n(&slot->joined, 0, __ATOMIC_RELEASE);
    cellos_unlock_slots();
    return 0;
}

int pthread_detach(pthread_t thread) {
    struct cellos_thread_slot *slot;
    uint32_t state;

    if (thread == 0 || thread > CELLOS_PTHREAD_MAX_THREADS) {
        return CELLOS_PTHREAD_EINVAL;
    }
    slot = &cellos_threads[thread - 1u];
    if (__atomic_load_n(&slot->joined, __ATOMIC_ACQUIRE) ||
        __atomic_load_n(&slot->detached, __ATOMIC_ACQUIRE)) {
        return CELLOS_PTHREAD_EINVAL;
    }
    state = __atomic_load_n(&slot->state, __ATOMIC_ACQUIRE);
    if (state == CELLOS_THREAD_FREE) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_store_n(&slot->detached, 1, __ATOMIC_RELEASE);
    if (state == CELLOS_THREAD_EXITED) {
        cellos_wait_for_task_exit(slot->tid);
        cellos_lock_slots();
        slot->start_routine = NULL;
        slot->arg = NULL;
        slot->result = NULL;
        slot->tid = 0;
        slot->detached = 0;
        __atomic_store_n(&slot->state, CELLOS_THREAD_FREE, __ATOMIC_RELEASE);
        cellos_unlock_slots();
    }
    return 0;
}

int pthread_mutex_init(pthread_mutex_t *mutex, const pthread_mutexattr_t *attr) {
    if (mutex == NULL || attr != NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_store_n(&mutex->state, 0, __ATOMIC_RELEASE);
    return 0;
}

int pthread_mutex_destroy(pthread_mutex_t *mutex) {
    if (mutex == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    return __atomic_load_n(&mutex->state, __ATOMIC_ACQUIRE) == 0 ? 0 : CELLOS_PTHREAD_EBUSY;
}

int pthread_mutex_lock(pthread_mutex_t *mutex) {
    if (mutex == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    for (;;) {
        uint32_t expected = 0;
        if (__atomic_compare_exchange_n(&mutex->state, &expected, 1, 0,
                                        __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
            return 0;
        }
        cellos_futex_wait(&mutex->state, 1);
    }
}

int pthread_mutex_trylock(pthread_mutex_t *mutex) {
    uint32_t expected = 0;
    if (mutex == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    return __atomic_compare_exchange_n(&mutex->state, &expected, 1, 0,
                                       __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)
               ? 0
               : CELLOS_PTHREAD_EBUSY;
}

int pthread_mutex_unlock(pthread_mutex_t *mutex) {
    if (mutex == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_store_n(&mutex->state, 0, __ATOMIC_RELEASE);
    cellos_futex_wake(&mutex->state, 1);
    return 0;
}

int pthread_cond_init(pthread_cond_t *cond, const pthread_condattr_t *attr) {
    if (cond == NULL || attr != NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_store_n(&cond->sequence, 0, __ATOMIC_RELEASE);
    return 0;
}

int pthread_cond_destroy(pthread_cond_t *cond) {
    return cond == NULL ? CELLOS_PTHREAD_EINVAL : 0;
}

int pthread_cond_wait(pthread_cond_t *cond, pthread_mutex_t *mutex) {
    uint32_t sequence;
    int result;
    if (cond == NULL || mutex == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    sequence = __atomic_load_n(&cond->sequence, __ATOMIC_ACQUIRE);
    result = pthread_mutex_unlock(mutex);
    if (result != 0) {
        return result;
    }
    cellos_futex_wait(&cond->sequence, sequence);
    return pthread_mutex_lock(mutex);
}

int pthread_cond_signal(pthread_cond_t *cond) {
    if (cond == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_add_fetch(&cond->sequence, 1, __ATOMIC_RELEASE);
    cellos_futex_wake(&cond->sequence, 1);
    return 0;
}

int pthread_cond_broadcast(pthread_cond_t *cond) {
    if (cond == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_add_fetch(&cond->sequence, 1, __ATOMIC_RELEASE);
    cellos_futex_wake(&cond->sequence, CELLOS_FUTEX_WAKE_ALL);
    return 0;
}

int pthread_rwlock_init(pthread_rwlock_t *rwlock, const pthread_rwlockattr_t *attr) {
    if (rwlock == NULL || attr != NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    __atomic_store_n(&rwlock->state, 0, __ATOMIC_RELEASE);
    return 0;
}

int pthread_rwlock_destroy(pthread_rwlock_t *rwlock) {
    if (rwlock == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    return __atomic_load_n(&rwlock->state, __ATOMIC_ACQUIRE) == 0 ? 0 : CELLOS_PTHREAD_EBUSY;
}

int pthread_rwlock_rdlock(pthread_rwlock_t *rwlock) {
    if (rwlock == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    for (;;) {
        uint32_t val = __atomic_load_n(&rwlock->state, __ATOMIC_RELAXED);
        if (val & 0x80000000u) {
            cellos_futex_wait(&rwlock->state, val);
            continue;
        }
        if (__atomic_compare_exchange_n(&rwlock->state, &val, val + 1, 0,
                                        __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
            return 0;
        }
    }
}

int pthread_rwlock_tryrdlock(pthread_rwlock_t *rwlock) {
    uint32_t val;
    if (rwlock == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    val = __atomic_load_n(&rwlock->state, __ATOMIC_RELAXED);
    if (val & 0x80000000u) {
        return CELLOS_PTHREAD_EBUSY;
    }
    if (__atomic_compare_exchange_n(&rwlock->state, &val, val + 1, 0,
                                    __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
        return 0;
    }
    return CELLOS_PTHREAD_EBUSY;
}

int pthread_rwlock_wrlock(pthread_rwlock_t *rwlock) {
    if (rwlock == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    for (;;) {
        uint32_t expected = 0;
        if (__atomic_compare_exchange_n(&rwlock->state, &expected, 0x80000000u, 0,
                                        __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
            return 0;
        }
        cellos_futex_wait(&rwlock->state, expected);
    }
}

int pthread_rwlock_trywrlock(pthread_rwlock_t *rwlock) {
    uint32_t expected = 0;
    if (rwlock == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    if (__atomic_compare_exchange_n(&rwlock->state, &expected, 0x80000000u, 0,
                                    __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
        return 0;
    }
    return CELLOS_PTHREAD_EBUSY;
}

int pthread_rwlock_unlock(pthread_rwlock_t *rwlock) {
    uint32_t val;
    if (rwlock == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    val = __atomic_load_n(&rwlock->state, __ATOMIC_RELAXED);
    if (val & 0x80000000u) {
        __atomic_store_n(&rwlock->state, 0, __ATOMIC_RELEASE);
        cellos_futex_wake(&rwlock->state, CELLOS_FUTEX_WAKE_ALL);
        return 0;
    }
    if (val > 0) {
        uint32_t prev = __atomic_fetch_sub(&rwlock->state, 1, __ATOMIC_RELEASE);
        if (prev == 1) {
            cellos_futex_wake(&rwlock->state, CELLOS_FUTEX_WAKE_ALL);
        }
        return 0;
    }
    return CELLOS_PTHREAD_EINVAL;
}

int pthread_barrier_init(pthread_barrier_t *barrier, const pthread_barrierattr_t *attr, unsigned int count) {
    if (barrier == NULL || attr != NULL || count == 0) {
        return CELLOS_PTHREAD_EINVAL;
    }
    barrier->count = 0;
    barrier->target = count;
    barrier->cycle = 0;
    return 0;
}

int pthread_barrier_destroy(pthread_barrier_t *barrier) {
    if (barrier == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    return 0;
}

int pthread_barrier_wait(pthread_barrier_t *barrier) {
    uint32_t cycle;
    uint32_t arrived;
    if (barrier == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    cycle = __atomic_load_n(&barrier->cycle, __ATOMIC_ACQUIRE);
    arrived = __atomic_add_fetch(&barrier->count, 1, __ATOMIC_ACQ_REL);
    if (arrived == barrier->target) {
        __atomic_store_n(&barrier->count, 0, __ATOMIC_RELEASE);
        __atomic_add_fetch(&barrier->cycle, 1, __ATOMIC_RELEASE);
        cellos_futex_wake(&barrier->cycle, CELLOS_FUTEX_WAKE_ALL);
        return PTHREAD_BARRIER_SERIAL_THREAD;
    }
    while (__atomic_load_n(&barrier->cycle, __ATOMIC_ACQUIRE) == cycle) {
        cellos_futex_wait(&barrier->cycle, cycle);
    }
    return 0;
}

int pthread_key_create(pthread_key_t *key, void (*destructor)(void *)) {
    if (key == NULL) {
        return CELLOS_PTHREAD_EINVAL;
    }
    cellos_lock_slots();
    for (uint32_t i = 0; i < CELLOS_PTHREAD_KEYS_MAX; ++i) {
        if (!cellos_pthread_keys[i].allocated) {
            cellos_pthread_keys[i].allocated = 1;
            cellos_pthread_keys[i].destructor = destructor;
            *key = i;
            cellos_unlock_slots();
            return 0;
        }
    }
    cellos_unlock_slots();
    return CELLOS_PTHREAD_EAGAIN;
}

int pthread_key_delete(pthread_key_t key) {
    if (key >= CELLOS_PTHREAD_KEYS_MAX) {
        return CELLOS_PTHREAD_EINVAL;
    }
    cellos_lock_slots();
    cellos_pthread_keys[key].allocated = 0;
    cellos_pthread_keys[key].destructor = NULL;
    cellos_unlock_slots();
    return 0;
}

int pthread_setspecific(pthread_key_t key, const void *value) {
    const void **vals;
    if (key >= CELLOS_PTHREAD_KEYS_MAX || !cellos_pthread_keys[key].allocated) {
        return CELLOS_PTHREAD_EINVAL;
    }
    vals = cellos_current_tls_values();
    vals[key] = value;
    return 0;
}

void *pthread_getspecific(pthread_key_t key) {
    const void **vals;
    if (key >= CELLOS_PTHREAD_KEYS_MAX || !cellos_pthread_keys[key].allocated) {
        return NULL;
    }
    vals = cellos_current_tls_values();
    return (void *)vals[key];
}
