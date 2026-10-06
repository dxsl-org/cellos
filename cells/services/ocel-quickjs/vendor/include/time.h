/* Cellos freestanding time.h. Only the epoch conversion the engine's Date
 * support needs is declared; the shim implements it over the cell clock. */
#ifndef QJS_VIOS_TIME_H
#define QJS_VIOS_TIME_H

#include <stddef.h>

typedef long time_t;

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
    /* glibc/BSD extension the engine's Date support reads directly. */
    long tm_gmtoff;
    const char *tm_zone;
};

struct tm *localtime_r(const time_t *timep, struct tm *result);
time_t time(time_t *tloc);

#endif
