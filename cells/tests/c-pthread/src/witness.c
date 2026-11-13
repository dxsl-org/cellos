#include "cellos_pthread.h"

#include <stdint.h>



static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t ready = PTHREAD_COND_INITIALIZER;
static uint32_t released;
static uint32_t waiting;
static uint32_t total;

static void *worker(void *arg) {
    uintptr_t value = (uintptr_t)arg;

    if (pthread_mutex_lock(&mutex) != 0) {
        return (void *)0;
    }
    waiting += 1;
    /* `ready` carries two predicates (main: waiting == 2; workers: released),
     * so a signal could wake a worker parked ahead of main and strand main. */
    if (pthread_cond_broadcast(&ready) != 0) {
        (void)pthread_mutex_unlock(&mutex);
        return (void *)0;
    }
    while (released == 0) {
        if (pthread_cond_wait(&ready, &mutex) != 0) {
            (void)pthread_mutex_unlock(&mutex);
            return (void *)0;
        }
    }
    total += (uint32_t)value;
    (void)pthread_mutex_unlock(&mutex);
    return arg;
}
static void *quick_worker(void *arg) {
    return arg;
}

static pthread_rwlock_t test_rwlock = PTHREAD_RWLOCK_INITIALIZER;
static pthread_barrier_t test_barrier;
static pthread_key_t test_key;
static uint32_t key_dtor_ran = 0;

static void key_destructor(void *val) {
    if ((uintptr_t)val == 0xbeef) {
        key_dtor_ran = 1;
    }
}

static void *key_worker(void *arg) {
    (void)arg;
    if (pthread_setspecific(test_key, (void *)(uintptr_t)0xbeef) != 0) {
        return (void *)(uintptr_t)1;
    }
    if ((uintptr_t)pthread_getspecific(test_key) != 0xbeef) {
        return (void *)(uintptr_t)2;
    }
    return (void *)0;
}

static void *barrier_worker(void *arg) {
    (void)arg;
    int res = pthread_barrier_wait(&test_barrier);
    if (res != 0 && res != PTHREAD_BARRIER_SERIAL_THREAD) {
        return (void *)(uintptr_t)1;
    }
    return (void *)0;
}

static void *rwlock_worker(void *arg) {
    (void)arg;
    if (pthread_rwlock_rdlock(&test_rwlock) != 0) {
        return (void *)(uintptr_t)1;
    }
    if (pthread_rwlock_unlock(&test_rwlock) != 0) {
        return (void *)(uintptr_t)2;
    }
    return (void *)0;
}

int cellos_pthread_witness(void) {
    pthread_t first;
    pthread_t second;
    void *first_result = 0;
    void *second_result = 0;

    if (pthread_create(&first, 0, worker, (void *)(uintptr_t)1) != 0
        || pthread_create(&second, 0, worker, (void *)(uintptr_t)2) != 0) {
        return 10;
    }
    if (pthread_mutex_lock(&mutex) != 0) {
        return 11;
    }
    while (waiting != 2) {
        if (pthread_cond_wait(&ready, &mutex) != 0) {
            (void)pthread_mutex_unlock(&mutex);
            return 12;
        }
    }
    released = 1;
    if (pthread_cond_broadcast(&ready) != 0 || pthread_mutex_unlock(&mutex) != 0) {
        return 12;
    }
    if (pthread_join(first, &first_result) != 0 || pthread_join(second, &second_result) != 0) {
        return 13;
    }
    if ((uintptr_t)first_result != 1 || (uintptr_t)second_result != 2 || total != 3) {
        return 14;
    }
    /*
     * A joined slot is reusable. More than the kernel's per-cell thread cap of
     * sequential create/join cycles proves every worker left the task table,
     * not merely its userspace slot.
     */
    for (uint32_t i = 0; i < 32; ++i) {
        pthread_t thread;
        void *result = 0;
        if (pthread_create(&thread, 0, quick_worker, (void *)(uintptr_t)(i + 1)) != 0) {
            return 15;
        }
        if (pthread_join(thread, &result) != 0) {
            return 150 + (int)i;
        }
        if ((uintptr_t)result != i + 1) {
            return 200 + (int)i;
        }
    }
    if (pthread_mutex_destroy(&mutex) != 0 || pthread_cond_destroy(&ready) != 0) {
        return 16;
    }
    /* 2. pthread_detach */
    pthread_t detached_thread;
    if (pthread_create(&detached_thread, 0, quick_worker, (void *)0) != 0) {
        return 30;
    }
    if (pthread_detach(detached_thread) != 0) {
        return 31;
    }
    if (pthread_join(detached_thread, 0) != CELLOS_PTHREAD_EINVAL) {
        return 32;
    }

    /* 3. pthread_rwlock */
    if (pthread_rwlock_init(&test_rwlock, 0) != 0) {
        return 40;
    }
    if (pthread_rwlock_wrlock(&test_rwlock) != 0) {
        return 41;
    }
    if (pthread_rwlock_tryrdlock(&test_rwlock) != CELLOS_PTHREAD_EBUSY) {
        return 42;
    }
    if (pthread_rwlock_unlock(&test_rwlock) != 0) {
        return 43;
    }
    if (pthread_rwlock_rdlock(&test_rwlock) != 0) {
        return 44;
    }
    pthread_t rw_th;
    if (pthread_create(&rw_th, 0, rwlock_worker, 0) != 0) {
        return 45;
    }
    void *rw_res = 0;
    if (pthread_join(rw_th, &rw_res) != 0 || (uintptr_t)rw_res != 0) {
        return 46;
    }
    if (pthread_rwlock_unlock(&test_rwlock) != 0) {
        return 47;
    }
    if (pthread_rwlock_destroy(&test_rwlock) != 0) {
        return 48;
    }

    /* 4. pthread_barrier */
    if (pthread_barrier_init(&test_barrier, 0, 2) != 0) {
        return 50;
    }
    pthread_t b_th;
    if (pthread_create(&b_th, 0, barrier_worker, 0) != 0) {
        return 51;
    }
    int b_res = pthread_barrier_wait(&test_barrier);
    if (b_res != 0 && b_res != PTHREAD_BARRIER_SERIAL_THREAD) {
        return 52;
    }
    void *b_worker_res = 0;
    if (pthread_join(b_th, &b_worker_res) != 0 || (uintptr_t)b_worker_res != 0) {
        return 53;
    }
    if (pthread_barrier_destroy(&test_barrier) != 0) {
        return 54;
    }

    /* 5. pthread_key and destructor */
    if (pthread_key_create(&test_key, key_destructor) != 0) {
        return 60;
    }
    if (pthread_setspecific(test_key, (void *)(uintptr_t)0x1234) != 0) {
        return 61;
    }
    if ((uintptr_t)pthread_getspecific(test_key) != 0x1234) {
        return 62;
    }
    pthread_t k_th;
    if (pthread_create(&k_th, 0, key_worker, 0) != 0) {
        return 63;
    }
    void *k_res = 0;
    if (pthread_join(k_th, &k_res) != 0 || (uintptr_t)k_res != 0) {
        return 64;
    }
    if (key_dtor_ran != 1) {
        return 65;
    }
    if (pthread_key_delete(test_key) != 0) {
        return 66;
    }
    /* The recreated key reuses the deleted index; main's stale 0x1234 must not show. */
    if (pthread_key_create(&test_key, 0) != 0) {
        return 67;
    }
    if (pthread_getspecific(test_key) != 0) {
        return 68;
    }
    if (pthread_key_delete(test_key) != 0) {
        return 69;
    }
    return 0;
}
