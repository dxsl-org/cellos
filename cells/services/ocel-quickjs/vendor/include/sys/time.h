/* Cellos freestanding sys/time.h. */
#ifndef QJS_VIOS_SYS_TIME_H
#define QJS_VIOS_SYS_TIME_H

#include <time.h>

struct timeval {
    long tv_sec;
    long tv_usec;
};

struct timezone {
    int tz_minuteswest;
    int tz_dsttime;
};

struct timespec {
    long tv_sec;
    long tv_nsec;
};

int gettimeofday(struct timeval *tv, void *tz);
int clock_gettime(int clk_id, struct timespec *tp);

#define CLOCK_REALTIME 0
#define CLOCK_MONOTONIC 1

#endif
