// SPDX-License-Identifier: MIT
#pragma once

// mmap prot flags
#define PROT_NONE   0
#define PROT_READ   1
#define PROT_WRITE  2
#define PROT_EXEC   4

// mmap flags — only MAP_ANONYMOUS is handled (via AnonAllocate); all others → EINVAL
#define MAP_SHARED    0x01
#define MAP_PRIVATE   0x02
#define MAP_FIXED     0x10
#define MAP_ANONYMOUS 0x20
#define MAP_ANON      MAP_ANONYMOUS
#define MAP_FAILED    ((void *)-1)
