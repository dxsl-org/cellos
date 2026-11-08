/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Narrow Cellos pthread subset for one Cell's task group.
 *
 * Threads share one Cell's address space, heap, capability set, and protection
 * domain.
 */
#ifndef CELLOS_PTHREAD_H
#define CELLOS_PTHREAD_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifndef CELLOS_PTHREAD_MAX_THREADS
#define CELLOS_PTHREAD_MAX_THREADS 16u
#endif

/* Standard positive error values returned by this freestanding subset. */
#define CELLOS_PTHREAD_EAGAIN 11
#define CELLOS_PTHREAD_EBUSY 16
#define CELLOS_PTHREAD_EINVAL 22
#define CELLOS_PTHREAD_ENOSYS 38

#define PTHREAD_BARRIER_SERIAL_THREAD -1
#define CELLOS_PTHREAD_KEYS_MAX 32u

/* Opaque slot ID; valid only until its single successful pthread_join. */
typedef uint32_t pthread_t;
typedef struct { uint32_t state; } pthread_mutex_t;
typedef struct { uint32_t sequence; } pthread_cond_t;
typedef struct { uint32_t state; } pthread_rwlock_t;
typedef struct { uint32_t count; uint32_t target; uint32_t cycle; } pthread_barrier_t;
typedef uint32_t pthread_key_t;
typedef struct { uint32_t reserved; } pthread_attr_t;
typedef struct { uint32_t reserved; } pthread_mutexattr_t;
typedef struct { uint32_t reserved; } pthread_condattr_t;
typedef struct { uint32_t reserved; } pthread_rwlockattr_t;
typedef struct { uint32_t reserved; } pthread_barrierattr_t;

#define PTHREAD_MUTEX_INITIALIZER { 0u }
#define PTHREAD_COND_INITIALIZER { 0u }
#define PTHREAD_RWLOCK_INITIALIZER { 0u }

/* Only NULL attributes are supported. `start_routine` must not be NULL. */
int pthread_create(pthread_t *thread, const pthread_attr_t *attr,
                   void *(*start_routine)(void *), void *arg);

/* Exactly one joiner consumes a joinable thread and releases its slot. */
int pthread_join(pthread_t thread, void **retval);
int pthread_detach(pthread_t thread);

/* Non-recursive mutex. Ownership misuse is undefined, matching POSIX defaults. */
int pthread_mutex_init(pthread_mutex_t *mutex, const pthread_mutexattr_t *attr);
int pthread_mutex_destroy(pthread_mutex_t *mutex);
int pthread_mutex_lock(pthread_mutex_t *mutex);
int pthread_mutex_trylock(pthread_mutex_t *mutex);
int pthread_mutex_unlock(pthread_mutex_t *mutex);

/* Condition variables require the usual caller-held mutex/predicate discipline. */
int pthread_cond_init(pthread_cond_t *cond, const pthread_condattr_t *attr);
int pthread_cond_destroy(pthread_cond_t *cond);
int pthread_cond_wait(pthread_cond_t *cond, pthread_mutex_t *mutex);
int pthread_cond_signal(pthread_cond_t *cond);
int pthread_cond_broadcast(pthread_cond_t *cond);

/* Reader-writer locks via futex. */
int pthread_rwlock_init(pthread_rwlock_t *rwlock, const pthread_rwlockattr_t *attr);
int pthread_rwlock_destroy(pthread_rwlock_t *rwlock);
int pthread_rwlock_rdlock(pthread_rwlock_t *rwlock);
int pthread_rwlock_tryrdlock(pthread_rwlock_t *rwlock);
int pthread_rwlock_wrlock(pthread_rwlock_t *rwlock);
int pthread_rwlock_trywrlock(pthread_rwlock_t *rwlock);
int pthread_rwlock_unlock(pthread_rwlock_t *rwlock);

/* Barriers via futex. */
int pthread_barrier_init(pthread_barrier_t *barrier, const pthread_barrierattr_t *attr, unsigned int count);
int pthread_barrier_destroy(pthread_barrier_t *barrier);
int pthread_barrier_wait(pthread_barrier_t *barrier);

/* Thread-specific data keys. */
int pthread_key_create(pthread_key_t *key, void (*destructor)(void *));
int pthread_key_delete(pthread_key_t key);
int pthread_setspecific(pthread_key_t key, const void *value);
void *pthread_getspecific(pthread_key_t key);

#ifdef __cplusplus
}
#endif
#endif
