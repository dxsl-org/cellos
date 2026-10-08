/* SPDX-License-Identifier: AGPL-3.0-or-later */
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <time.h>
#include <limits.h>
#include <errno.h>
#include <math.h>

long lrintf(float value) { return (long)rintf(value); }

int ocel_pdf_errno;

/* LCMS profile-writing error cleanup can retain remove through static link
 * reachability even though the service only opens profiles from memory.
 * Fail closed: this renderer is read-only and never reports a deletion. */
int remove(const char *path)
{
    (void)path;
    errno = EACCES;
    return -1;
}

/* RV64GC has no mandatory hardware population-count instruction. Keep the
 * compiler helper in this lp64d archive rather than pulling a toolchain
 * compiler_builtins object with an incompatible floating-point ELF ABI.
 * Parallel bit summation is exact for all 64-bit inputs and uses no helpers. */
int __popcountdi2(uint64_t value)
{
    value -= (value >> 1) & UINT64_C(0x5555555555555555);
    value = (value & UINT64_C(0x3333333333333333)) +
        ((value >> 2) & UINT64_C(0x3333333333333333));
    value = (value + (value >> 4)) & UINT64_C(0x0f0f0f0f0f0f0f0f);
    return (int)((value * UINT64_C(0x0101010101010101)) >> 56);
}
int abs(int x) { return x < 0 ? -x : x; }
long labs(long x) { return x < 0 ? -x : x; }
long long llabs(long long x) { return x < 0 ? -x : x; }
/* Cells have no process environment. */
char *getenv(const char *name) { (void)name; return NULL; }
char *strerror(int e)
{
    switch (e) {
    case 0: return "Success";
    case ENOMEM: return "Out of memory";
    case EINVAL: return "Invalid argument";
    case ERANGE: return "Out of range";
    case ENOENT: return "File not found";
    case EACCES: return "Permission denied";
    default: return "Cell I/O error";
    }
}
char *strstr(const char *s, const char *find)
{
    size_t n = strlen(find);
    if (!n) return (char *)s;
    for (; *s; ++s) if (*s == *find && !strncmp(s, find, n)) return (char *)s;
    return NULL;
}
char *strchrnul(const char *s, int c) { while (*s && *s != c) ++s; return (char *)s; }
char *strpbrk(const char *s, const char *set)
{ for (; *s; ++s) if (strchr(set, *s)) return (char *)s; return NULL; }
size_t strspn(const char *s, const char *set)
{ size_t n = 0; while (s[n] && strchr(set, s[n])) ++n; return n; }
size_t strcspn(const char *s, const char *set)
{ size_t n = 0; while (s[n] && !strchr(set, s[n])) ++n; return n; }
char *strdup(const char *s)
{ size_t n = strlen(s) + 1; char *p = malloc(n); if (p) memcpy(p, s, n); return p; }

static unsigned long parse_integer(const char *input, char **end, int base,
    int *negative, unsigned long limit)
{
    const char *s = input;
    while (isspace((unsigned char)*s)) ++s;
    *negative = *s == '-';
    if (*s == '-' || *s == '+') ++s;
    if (base && (base < 2 || base > 36)) { errno = EINVAL; if (end) *end = (char *)input; return 0; }
    if ((!base || base == 16) && s[0] == '0' && (s[1] == 'x' || s[1] == 'X') && isxdigit((unsigned char)s[2])) {
        base = 16; s += 2;
    } else if (!base) base = *s == '0' ? 8 : 10;
    const char *digits = s;
    unsigned long value = 0;
    int overflow = 0;
    while (*s) {
        int c = (unsigned char)*s;
        unsigned int d = isdigit(c) ? (unsigned int)(c-'0') : isalpha(c) ? (unsigned int)(tolower(c)-'a'+10) : 36;
        if (d >= (unsigned int)base) break;
        if (value > (limit - d) / (unsigned int)base) overflow = 1;
        else if (!overflow) value = value * (unsigned int)base + d;
        ++s;
    }
    if (end) *end = (char *)(s == digits ? input : s);
    if (overflow) { errno = ERANGE; return limit; }
    return value;
}
unsigned long strtoul(const char *s, char **end, int base)
{
    int negative, previous = errno;
    errno = 0;
    unsigned long n = parse_integer(s, end, base, &negative, ULONG_MAX);
    int overflow = errno == ERANGE;
    if (!errno) errno = previous;
    return negative && !overflow ? 0UL - n : n;
}
long strtol(const char *s, char **end, int base)
{
    const char *p = s; while (isspace((unsigned char)*p)) ++p;
    unsigned long limit = *p == '-' ? (unsigned long)LONG_MAX + 1 : LONG_MAX;
    int negative;
    unsigned long n = parse_integer(s, end, base, &negative, limit);
    return negative ? (n == (unsigned long)LONG_MAX + 1 ? LONG_MIN : -(long)n) : (long)n;
}
unsigned long long strtoull(const char *s, char **e, int b) { return strtoul(s,e,b); }
long long strtoll(const char *s, char **e, int b) { return strtol(s,e,b); }
int atoi(const char *s) { return (int)strtol(s,NULL,10); }
long atol(const char *s) { return strtol(s,NULL,10); }
long long atoll(const char *s) { return strtol(s,NULL,10); }

static void swap(unsigned char *a, unsigned char *b, size_t width)
{ while (width--) { unsigned char c = *a; *a++ = *b; *b++ = c; } }
static void sift(unsigned char *base, size_t count, size_t root, size_t width,
    int (*compare)(const void *, const void *))
{
    while (root < count / 2) {
        size_t child = root * 2 + 1;
        if (child + 1 < count && compare(base + child * width, base + (child + 1) * width) < 0) ++child;
        if (compare(base + root * width, base + child * width) >= 0) break;
        swap(base + root * width, base + child * width, width);
        root = child;
    }
}
void qsort(void *ptr, size_t count, size_t width, int (*compare)(const void *, const void *))
{
    if (!width || count < 2) return;
    unsigned char *base = ptr;
    for (size_t i = count / 2; i; --i) sift(base, count, i-1, width, compare);
    for (size_t i = count; i > 1; --i) {
        swap(base, base + (i-1)*width, width); sift(base, i-1, 0, width, compare);
    }
}
void *bsearch(const void *key, const void *ptr, size_t count, size_t width,
    int (*compare)(const void *, const void *))
{
    const unsigned char *base = ptr;
    while (count) {
        size_t half = count / 2;
        const unsigned char *mid = base + half * width;
        int result = compare(key, mid);
        if (!result) return (void *)mid;
        if (result < 0) count = half;
        else { base = mid + width; count -= half + 1; }
    }
    return NULL;
}

/* No wall-clock epoch is available in this cell. C time() reports the standard
 * unavailable sentinel; date parsing/formatting of document timestamps is real. */
time_t time(time_t *out) { if (out) *out = -1; return -1; }
static long days_from_civil(long year, int month, int day)
{
    year -= month <= 2;
    long era = (year >= 0 ? year : year - 399) / 400;
    unsigned int yoe = (unsigned int)(year - era * 400);
    unsigned int doy = (153u * (unsigned int)(month + (month > 2 ? -3 : 9)) + 2) / 5 + day - 1;
    return era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
}
time_t timegm(struct tm *tm)
{
    long year = (long)tm->tm_year + 1900;
    long mon = tm->tm_mon;
    year += mon / 12; mon %= 12;
    if (mon < 0) { mon += 12; --year; }
    return days_from_civil(year, (int)mon+1, tm->tm_mday) * 86400 +
        (long)tm->tm_hour*3600 + (long)tm->tm_min*60 + tm->tm_sec;
}
struct tm *gmtime(const time_t *input)
{
    static struct tm result;
    long days = *input / 86400, rem = *input % 86400;
    if (rem < 0) { rem += 86400; --days; }
    long z = days + 719468;
    long era = (z >= 0 ? z : z - 146096) / 146097;
    unsigned int doe = (unsigned int)(z - era*146097);
    unsigned int yoe = (doe - doe/1460 + doe/36524 - doe/146096)/365;
    long year = yoe + era*400;
    unsigned int doy = doe - (365*yoe + yoe/4 - yoe/100);
    unsigned int mp = (5*doy+2)/153;
    unsigned int day = doy - (153*mp+2)/5+1;
    int month = (int)mp + (mp < 10 ? 3 : -9);
    year += month <= 2;
    memset(&result, 0, sizeof(result));
    result.tm_year = (int)(year-1900); result.tm_mon = month-1; result.tm_mday = (int)day;
    result.tm_hour = (int)(rem/3600); result.tm_min = (int)(rem/60%60); result.tm_sec = (int)(rem%60);
    result.tm_wday = (int)((days+4)%7); if (result.tm_wday < 0) result.tm_wday += 7;
    result.tm_yday = (int)(days-days_from_civil(year,1,1)); result.tm_zone = "UTC";
    return &result;
}
size_t strftime(char *out, size_t capacity, const char *format, const struct tm *tm)
{
    size_t used = 0;
    while (*format) {
        char text[32]; const char *part = text;
        if (*format != '%') { text[0] = *format++; text[1] = 0; }
        else {
            ++format;
            switch (*format++) {
            case '%': part = "%"; break;
            case 'Y': snprintf(text,sizeof(text),"%04d",tm->tm_year+1900); break;
            case 'm': snprintf(text,sizeof(text),"%02d",tm->tm_mon+1); break;
            case 'd': snprintf(text,sizeof(text),"%02d",tm->tm_mday); break;
            case 'H': snprintf(text,sizeof(text),"%02d",tm->tm_hour); break;
            case 'M': snprintf(text,sizeof(text),"%02d",tm->tm_min); break;
            case 'S': snprintf(text,sizeof(text),"%02d",tm->tm_sec); break;
            case 'z': part = "+0000"; break;
            case 'Z': part = "UTC"; break;
            case 'F': snprintf(text,sizeof(text),"%04d-%02d-%02d",tm->tm_year+1900,tm->tm_mon+1,tm->tm_mday); break;
            case 'T': snprintf(text,sizeof(text),"%02d:%02d:%02d",tm->tm_hour,tm->tm_min,tm->tm_sec); break;
            default: return 0;
            }
        }
        size_t n = strlen(part);
        if (used >= capacity || n >= capacity-used) return 0;
        memcpy(out+used,part,n); used += n;
    }
    if (!capacity) return 0;
    out[used] = 0; return used;
}
