// SPDX-License-Identifier: MPL-2.0
// mlibc `posix` checks for x86_64 Tier-2 (runtime plan R3a). Each check prints
// "MLIBC-POSIX: <name> OK|FAIL"; the summary line is "MLIBC-POSIX: <p>/<t> pass".

#include <elf.h>
#include <errno.h>
#include <fcntl.h>
#include <link.h>
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#define THREADS 8
#define ROUNDS 1000
#define COND_WAIT_NS 50000000LL
#define SLEEP_NS 20000000LL

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_once_t once = PTHREAD_ONCE_INIT;
static long counter;
static int once_runs;
static int tls_ok;
static __thread int tls_value = 7;

static void once_fn(void) { once_runs++; }

static long long mono_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}

// Every thread must see the initial TLS image, then keep its own TLS value and
// its own errno across a lock-heavy loop that yields.
static void *worker(void *arg) {
    int id = (int)(long)arg;
    int ok = tls_value == 7;
    tls_value = 100 + id;
    errno = 200 + id;
    pthread_once(&once, once_fn);
    for (int i = 0; i < ROUNDS; i++) {
        pthread_mutex_lock(&lock);
        counter++;
        pthread_mutex_unlock(&lock);
        if ((i & 63) == 0)
            sched_yield();
    }
    ok = ok && tls_value == 100 + id && errno == 200 + id;
    pthread_mutex_lock(&lock);
    tls_ok += ok;
    pthread_mutex_unlock(&lock);
    return (void *)(long)ok;
}

struct phdr_scan {
    int eh_frame;
    int build_id;
};

static void scan_notes(const char *n, const char *end, struct phdr_scan *s) {
    while (n + sizeof(Elf64_Nhdr) <= end) {
        const Elf64_Nhdr *h = (const Elf64_Nhdr *)n;
        size_t name = (h->n_namesz + 3u) & ~3u;
        size_t desc = (h->n_descsz + 3u) & ~3u;
        const char *owner = n + sizeof(*h);
        if (h->n_type == NT_GNU_BUILD_ID && h->n_namesz == 4 && memcmp(owner, "GNU", 4) == 0
            && h->n_descsz == 20)
            s->build_id = 1;
        n = owner + name + desc;
    }
}

static int scan_phdrs(struct dl_phdr_info *info, size_t size, void *data) {
    (void)size;
    struct phdr_scan *s = data;
    for (int i = 0; i < info->dlpi_phnum; i++) {
        const Elf64_Phdr *p = &info->dlpi_phdr[i];
        if (p->p_type == PT_GNU_EH_FRAME)
            s->eh_frame = 1;
        if (p->p_type == PT_NOTE) {
            const char *n = (const char *)(info->dlpi_addr + p->p_vaddr);
            scan_notes(n, n + p->p_memsz, s);
        }
    }
    return 0;
}

static int check(const char *name, int ok) {
    printf("MLIBC-POSIX: %s %s\n", name, ok ? "OK" : "FAIL");
    return ok ? 1 : 0;
}

int mlibc_posix_witness(void) {
    int pass = 0, total = 0;

    pthread_t threads[THREADS];
    int created = 1, joined = 1;
    for (long i = 0; i < THREADS; i++)
        created &= pthread_create(&threads[i], NULL, worker, (void *)i) == 0;
    for (int i = 0; created && i < THREADS; i++) {
        void *ret = NULL;
        joined &= pthread_join(threads[i], &ret) == 0 && ret == (void *)1;
    }
    total++, pass += check("threads", created && joined && counter == THREADS * ROUNDS);
    total++, pass += check("once", once_runs == 1);
    total++, pass += check("tls-errno", tls_ok == THREADS && tls_value == 7);

    // Nobody signals: the wait must time out, and not before its deadline.
    pthread_condattr_t attr;
    pthread_cond_t cond;
    pthread_condattr_init(&attr);
    pthread_condattr_setclock(&attr, CLOCK_MONOTONIC);
    pthread_cond_init(&cond, &attr);
    struct timespec deadline;
    clock_gettime(CLOCK_MONOTONIC, &deadline);
    long long start = (long long)deadline.tv_sec * 1000000000LL + deadline.tv_nsec;
    deadline.tv_nsec += COND_WAIT_NS;
    deadline.tv_sec += deadline.tv_nsec / 1000000000L;
    deadline.tv_nsec %= 1000000000L;
    pthread_mutex_lock(&lock);
    int rc = pthread_cond_timedwait(&cond, &lock, &deadline);
    pthread_mutex_unlock(&lock);
    total++, pass += check("cond-timedwait", rc == ETIMEDOUT && mono_ns() - start >= COND_WAIT_NS);

    struct timespec req = {0, SLEEP_NS};
    start = mono_ns();
    rc = clock_nanosleep(CLOCK_MONOTONIC, 0, &req, NULL);
    total++, pass += check("nanosleep", rc == 0 && mono_ns() - start >= SLEEP_NS);

    errno = 0;
    int fd = open("/nonexistent/mlibc-posix-x86", O_RDONLY);
    total++, pass += check("errno", fd < 0 && errno == ENOENT);

    struct phdr_scan scan = {0, 0};
    dl_iterate_phdr(scan_phdrs, &scan);
    total++, pass += check("dl_iterate_phdr", scan.eh_frame && scan.build_id);

    printf("MLIBC-POSIX: %d/%d pass\n", pass, total);
    return pass == total ? 0 : 1;
}
