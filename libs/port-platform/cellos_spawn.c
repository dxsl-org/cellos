/* SPDX-License-Identifier: MPL-2.0 */
#include "cellos_spawn.h"

#include "cellos_syscall.h"

#include <stddef.h>
#include <stdint.h>

/* Kernel ABI opcodes (libs/api/src/abi/syscall.rs). */
#define CELLOS_SYSCALL_SEND 0u
#define CELLOS_SYSCALL_RECV 1u
#define CELLOS_SYSCALL_WAIT 8u
#define CELLOS_SYSCALL_SPAWN_FROM_PATH 12u
#define CELLOS_SYSCALL_SPAWN_FROM_ELF 238u
#define CELLOS_SYSCALL_PIPE_SHARE 25u
#define CELLOS_SYSCALL_STATE_STASH 410u
#define CELLOS_SYSCALL_STATE_RESTORE 411u
#define CELLOS_SYSCALL_STATE_STASH_CLEAR 412u

/* Reserved kernel argv slot. Any other key addresses the generic stash, which
 * the kernel keeps separate from a pending command line on purpose. */
#define CELLOS_SPAWN_ARGV_KEY ((uintptr_t)0x0061726776000000ull)

/* "\0argv1\0" — the structured prefix `ostd::args` encodes and decodes. */
#define CELLOS_SPAWN_ARGV_PREFIX_LEN 7u
static const char cellos_argv_prefix[CELLOS_SPAWN_ARGV_PREFIX_LEN] = {
    0, 'a', 'r', 'g', 'v', '1', 0,
};

/// Encode `argv` for the kernel carrier, or `(size_t)-1` when it does not fit.
static size_t encode_argv(const char *const *argv, char *out, size_t capacity) {
    size_t at = 0;
    size_t index;

    for (index = 0; index < CELLOS_SPAWN_ARGV_PREFIX_LEN; index++) {
        if (at >= capacity) {
            return (size_t)-1;
        }
        out[at++] = cellos_argv_prefix[index];
    }
    if (argv == NULL) {
        /* A prefix with no items: "launched with no arguments", which the child
         * distinguishes from "nothing was staged". */
        return at;
    }
    for (index = 0; argv[index] != NULL; index++) {
        const char *item = argv[index];
        size_t offset = 0;
        while (item[offset] != '\0') {
            if (at >= capacity) {
                return (size_t)-1;
            }
            out[at++] = item[offset++];
        }
        if (at >= capacity) {
            return (size_t)-1;
        }
        out[at++] = '\0';
    }
    return at;
}

/// Length of a NUL-terminated reviewed target, or 0 when it is not a bounded
/// NUL-terminated string.
static size_t target_length(const char *target) {
    size_t length = 0;
    while (length < CELLOS_SPAWN_TARGET_MAX && target[length] != '\0') {
        length++;
    }
    if (length == 0 || length == CELLOS_SPAWN_TARGET_MAX) {
        return 0;
    }
    return length;
}

int cellos_spawn(const cellos_spawn_request *request, cellos_child *out) {
    char argv[CELLOS_SPAWN_ARGV_MAX];
    size_t argv_len;
    size_t length;
    size_t index;
    intptr_t result;
    uintptr_t message[1 + CELLOS_SPAWN_MAX_GRANTS];
    size_t message_len;

    if (request == NULL || out == NULL || request->target == NULL) {
        return CELLOS_SPAWN_EINVAL;
    }
    if (request->grant_count > CELLOS_SPAWN_MAX_GRANTS) {
        return CELLOS_SPAWN_EINVAL;
    }
    if (request->grant_count > 0 && request->grant_handles == NULL) {
        return CELLOS_SPAWN_EINVAL;
    }
    out->tid = 0;

    length = target_length(request->target);
    if (length == 0) {
        return CELLOS_SPAWN_ETARGET;
    }
    argv_len = encode_argv(request->argv, argv, sizeof argv);
    if (argv_len == (size_t)-1) {
        return CELLOS_SPAWN_EARGV;
    }

    /* The kernel clears the slot after every external-launch attempt, so a
     * failure below cannot leave a command line addressed to a later child. */
    result = cellos_syscall4(CELLOS_SYSCALL_STATE_STASH, CELLOS_SPAWN_ARGV_KEY, (uintptr_t)argv,
                             argv_len, 0);
    if (result != (intptr_t)argv_len) {
        (void)cellos_syscall4(CELLOS_SYSCALL_STATE_STASH_CLEAR, CELLOS_SPAWN_ARGV_KEY, 0, 0, 0);
        return CELLOS_SPAWN_EARGV;
    }

    /* Both routes resolve the same reviewed `(caller, route, target)` row. The
     * grant form is the post-boot one: the cell store belongs to the VFS
     * service, so the caller brings the bytes it already fetched. */
    if (request->elf_grant != 0) {
        if (request->elf_len == 0) {
            (void)cellos_syscall4(CELLOS_SYSCALL_STATE_STASH_CLEAR, CELLOS_SPAWN_ARGV_KEY, 0, 0, 0);
            return CELLOS_SPAWN_EINVAL;
        }
        result = cellos_syscall4(CELLOS_SYSCALL_SPAWN_FROM_ELF, request->elf_grant, request->elf_len,
                                 (uintptr_t)request->target, length);
    } else {
        result = cellos_syscall4(CELLOS_SYSCALL_SPAWN_FROM_PATH, (uintptr_t)request->target, length,
                                 0, 0);
    }
    if (result <= 0) {
        return result == CELLOS_DENIED ? CELLOS_SPAWN_EDENIED : CELLOS_SPAWN_ELAUNCH;
    }
    out->tid = (uintptr_t)result;

    /* Authorization for an endpoint is the grant, not the token: PipeShare is
     * owner-only, so a caller cannot hand over a handle it does not hold. */
    for (index = 0; index < request->grant_count; index++) {
        result = cellos_syscall4(CELLOS_SYSCALL_PIPE_SHARE,
                                 (uintptr_t)request->grant_handles[index], out->tid, 0, 0);
        if (result != 0) {
            return CELLOS_SPAWN_EGRANT;
        }
    }
    message[0] = (uintptr_t)request->grant_count;
    for (index = 0; index < request->grant_count; index++) {
        message[index + 1] = request->grant_handles[index];
    }
    message_len = sizeof(uintptr_t) * (1 + request->grant_count);
    result = cellos_syscall4(CELLOS_SYSCALL_SEND, out->tid, (uintptr_t)message, message_len, 0);
    if (result < 0) {
        return CELLOS_SPAWN_EGRANT;
    }
    return CELLOS_SPAWN_OK;
}

int cellos_child_wait(const cellos_child *child, int *status) {
    intptr_t result;

    if (child == NULL || status == NULL || child->tid == 0) {
        return CELLOS_SPAWN_EINVAL;
    }
    result = cellos_syscall4(CELLOS_SYSCALL_WAIT, child->tid, 0, 0, 0);
    if (result == CELLOS_DENIED) {
        /* The kernel no longer has the record: the child already crossed the
         * terminal boundary, which is what this call exists to establish. */
        return CELLOS_SPAWN_EREAPED;
    }
    if (result < 0) {
        return CELLOS_SPAWN_EWAIT;
    }
    *status = (int)result;
    return CELLOS_SPAWN_OK;
}

size_t cellos_spawn_argv_raw(char *buf, size_t capacity) {
    intptr_t result;

    if (buf == NULL || capacity == 0 || capacity > CELLOS_SPAWN_ARGV_MAX) {
        return (size_t)-1;
    }
    result = cellos_syscall4(CELLOS_SYSCALL_STATE_RESTORE, CELLOS_SPAWN_ARGV_KEY, (uintptr_t)buf,
                             capacity, 0);
    if (result < 0) {
        return (size_t)-1;
    }
    return (size_t)result;
}

size_t cellos_spawn_argv_split(char *buf, size_t length, const char **items, size_t capacity) {
    size_t count = 0;
    size_t start = CELLOS_SPAWN_ARGV_PREFIX_LEN;
    size_t index;

    if (length == 0) {
        return 0;
    }
    if (buf == NULL || items == NULL || length < CELLOS_SPAWN_ARGV_PREFIX_LEN) {
        return (size_t)-1;
    }
    for (index = 0; index < CELLOS_SPAWN_ARGV_PREFIX_LEN; index++) {
        if (buf[index] != cellos_argv_prefix[index]) {
            return (size_t)-1;
        }
    }
    if (start == length) {
        return 0;
    }
    if (buf[length - 1] != '\0') {
        return (size_t)-1;
    }
    while (start < length) {
        size_t end = start;
        while (end < length && buf[end] != '\0') {
            end++;
        }
        if (end == length || count == capacity) {
            return (size_t)-1;
        }
        items[count++] = &buf[start];
        start = end + 1;
    }
    return count;
}

size_t cellos_spawn_receive_grants(uintptr_t *handles, size_t capacity) {
    uintptr_t message[1 + CELLOS_SPAWN_MAX_GRANTS];
    intptr_t sender;
    size_t count;
    size_t index;

    if (handles == NULL || capacity == 0 || capacity > CELLOS_SPAWN_MAX_GRANTS) {
        return (size_t)-1;
    }
    for (index = 0; index < 1 + CELLOS_SPAWN_MAX_GRANTS; index++) {
        message[index] = 0;
    }
    sender = cellos_syscall4(CELLOS_SYSCALL_RECV, 0, (uintptr_t)message, sizeof message, 0);
    if (sender < 0) {
        return (size_t)-1;
    }
    count = (size_t)message[0];
    if (count > capacity || count > CELLOS_SPAWN_MAX_GRANTS) {
        return (size_t)-1;
    }
    for (index = 0; index < count; index++) {
        handles[index] = message[index + 1];
    }
    return count;
}
