/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Granted child half of the C child-launch adapter witness.
 *
 * This cell is started only through the reviewed
 * `(caller="c-spawn", route=Elf, target="/bin/c-spawn-child")` edge. It reads
 * the command line its launcher staged, receives the endpoint tokens granted to
 * it, writes one report header plus an ordered payload, closes its writer end,
 * and exits with a status its launcher observes.
 *
 * The report header is `{ item_count, argv_ok, payload_len, exit_code }`. Making
 * the report self-describing (rather than only pass/fail) is what lets the
 * launcher prove three separate things: that the command line arrived, that a
 * denied launch left no command line behind, and that the status reported over
 * the endpoint equals the status the kernel publishes.
 *
 * The whole report must fit the ring without blocking, because the launcher
 * waits for this child before it drains: the launcher's kernel-published status
 * is only available to a waiter registered before the child leaves the task
 * table, so waiting first is the ordering that keeps it observable.
 *
 * Markers:
 *   `[c-spawn-child] argv items=`
 *   `[c-spawn-child] payload bytes=`
 *   `[c-spawn-child] argv mismatch`
 */

#include "cellos_spawn.h"

#include "cellos_syscall.h"

#include <stddef.h>
#include <stdint.h>

/* Kernel ABI opcodes (libs/api/src/abi/syscall.rs). */
#define CELLOS_SYSCALL_PIPE_WRITE 23u

#define PAYLOAD 192u
#define RING_CAPACITY 256u
#define HEADER_WORDS 4u
#define CHILD_STATUS 42
#define MAX_ITEMS 4u

/* The report may not exceed the ring: this child writes before anyone drains. */
enum { REPORT_BYTES = HEADER_WORDS * sizeof(uintptr_t) + PAYLOAD };

static uint8_t payload_byte(size_t index) {
    return (uint8_t)(index % 251u);
}

static int item_equals(const char *item, const char *expected) {
    size_t index = 0;
    while (item[index] != '\0' || expected[index] != '\0') {
        if (item[index] != expected[index]) {
            return 0;
        }
        index++;
    }
    return 1;
}

static int write_all(uintptr_t handle, const uint8_t *bytes, size_t count) {
    size_t at = 0;

    while (at < count) {
        intptr_t written = cellos_syscall4(CELLOS_SYSCALL_PIPE_WRITE, handle,
                                           (uintptr_t)(bytes + at), count - at, 0);
        if (written <= 0) {
            return -1;
        }
        at += (size_t)written;
    }
    return 0;
}

int cellos_spawn_child_witness(void) {
    char argv[CELLOS_SPAWN_ARGV_MAX];
    const char *items[MAX_ITEMS];
    uintptr_t handles[CELLOS_SPAWN_MAX_GRANTS];
    uintptr_t header[HEADER_WORDS];
    uint8_t payload[64];
    size_t length;
    size_t count;
    size_t grants;
    size_t written;
    size_t index;
    int status = CHILD_STATUS;

    length = cellos_spawn_argv_raw(argv, sizeof argv);
    if (length == (size_t)-1) {
        cellos_log_text("[c-spawn-child] argv carrier refused\n");
        return 10;
    }
    count = cellos_spawn_argv_split(argv, length, items, MAX_ITEMS);
    if (count == (size_t)-1) {
        cellos_log_text("[c-spawn-child] argv wire format invalid\n");
        return 11;
    }
    /* Accepted command lines are exactly "no arguments" or the reviewed pair.
     * Anything else keeps the report flowing but fails the run, so the launcher
     * sees a wrong header and a wrong exit status. */
    if (count == 0) {
        header[1] = 1;
    } else if (count == 2 && item_equals(items[0], "alpha") && item_equals(items[1], "beta gamma")) {
        header[1] = 1;
    } else {
        header[1] = 0;
        status = 16;
    }
    header[0] = count;
    header[2] = PAYLOAD;
    header[3] = (uintptr_t)status;

    grants = cellos_spawn_receive_grants(handles, CELLOS_SPAWN_MAX_GRANTS);
    if (grants != 1) {
        cellos_log_value("[c-spawn-child] granted endpoints=", grants);
        return 12;
    }

    if (write_all(handles[0], (const uint8_t *)header, sizeof header) != 0) {
        return 13;
    }
    written = 0;
    while (written < PAYLOAD) {
        size_t chunk = PAYLOAD - written;
        if (chunk > sizeof payload) {
            chunk = sizeof payload;
        }
        for (index = 0; index < chunk; index++) {
            payload[index] = payload_byte(written + index);
        }
        if (write_all(handles[0], payload, chunk) != 0) {
            return 14;
        }
        written += chunk;
    }
    if (written + sizeof header != REPORT_BYTES || REPORT_BYTES > RING_CAPACITY) {
        return 17;
    }
    /* A zero-length write closes the writer end, which is how the launcher's
     * read observes exact EOF. */
    if (cellos_syscall4(CELLOS_SYSCALL_PIPE_WRITE, handles[0], (uintptr_t)payload, 0, 0) != 0) {
        return 15;
    }

    cellos_log_value("[c-spawn-child] argv items=", count);
    cellos_log_value("[c-spawn-child] payload bytes=", written);
    if (status != CHILD_STATUS) {
        cellos_log_text("[c-spawn-child] argv mismatch\n");
    }
    return status;
}
