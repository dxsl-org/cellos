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
    if (pthread_cond_signal(&ready) != 0) {
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
    return 0;
}
