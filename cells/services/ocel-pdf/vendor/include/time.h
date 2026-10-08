/* SPDX-License-Identifier: AGPL-3.0-or-later */
#ifndef OCEL_PDF_TIME_H
#define OCEL_PDF_TIME_H
#include <stddef.h>
typedef long time_t;
struct tm { int tm_sec,tm_min,tm_hour,tm_mday,tm_mon,tm_year,tm_wday,tm_yday,tm_isdst; long tm_gmtoff; const char *tm_zone; };
#define time ocel_pdf_time
#define timegm ocel_pdf_timegm
#define gmtime ocel_pdf_gmtime
#define localtime ocel_pdf_gmtime
#define strftime ocel_pdf_strftime
time_t time(time_t *);
time_t timegm(struct tm *);
struct tm *gmtime(const time_t *);
size_t strftime(char *,size_t,const char *,const struct tm *);
char *ctime(const time_t *);
#endif
