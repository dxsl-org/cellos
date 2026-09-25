/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Tier-2 C child-launch adapter witness (portability program P2).
 *
 * One run proves, against the published `cellos_spawn.h` surface:
 *   1. a reviewed target launches and the child is a separate Tier 2 domain,
 *   2. the staged command line reaches the child byte-for-byte, including an
 *      item that contains a space,
 *   3. a pipe endpoint granted with the launch is usable by the child, its
 *      ordered payload arrives intact, and EOF is exact,
 *   4. the child's terminal state is observed through `cellos_child_wait`, and
 *      the kernel-published status matches the status the child reported over
 *      the endpoint,
 *   5. an unreviewed target is denied by the kernel launch edge — the same
 *      bytes do not authorize a different path,
 *   6. an over-long command line is refused before the kernel sees it,
 *   7. a denied launch leaves no staged command line behind.
 *
 * Ordering note: the launcher waits before it drains. The kernel publishes a
 * child's exit status only to a waiter registered before the child leaves the
 * task table, so a launcher that blocks on the child's output first can miss it;
 * the child therefore keeps its whole report inside the ring, and the reported
 * status crosses on the endpoint as well.
 *
 * Markers (integration-test contract):
 *   `[c-spawn] launched tid=`
 *   `[c-spawn] route elf-bytes reviewed edge`
 *   `[c-spawn] wait published status=` / `[c-spawn] wait already-terminal`
 *   `[c-spawn] child argv items=2`
 *   `[c-spawn] ordered payload bytes=192`
 *   `[c-spawn] child reported status=42`
 *   `[c-spawn] unreviewed target denied`
 *   `[c-spawn] overlong argv rejected`
 *   `[c-spawn] denied launch left no staged argv`
 */

#include "cellos_spawn.h"

#include "cellos_syscall.h"

#include <stddef.h>
#include <stdint.h>

/* Kernel ABI opcodes (libs/api/src/abi/syscall.rs). */
#define CELLOS_SYSCALL_PIPE_CREATE 19u
#define CELLOS_SYSCALL_PIPE_READ 22u
#define CELLOS_SYSCALL_PIPE_CLOSE 24u

#define CHILD_PATH "/bin/c-spawn-child"
/* A real, reviewed cell for a *different* caller: the kernel must refuse it
 * here, so the adapter holds exactly one launch edge and no more. */
#define UNREVIEWED_PATH "/bin/pipe-peer"

/* One item longer than the whole encoded command line bound, so the adapter
 * must refuse it before staging anything. */
static const char kOverlongItem[601] = {[0 ... 599] = 'x'};
static const char *const kOverlongArgv[] = {kOverlongItem, (const char *)0};

#define CAPACITY 256u
#define PAYLOAD 192u
#define HEADER_WORDS 4u
#define CHILD_STATUS 42
#define EXPECTED_ITEMS 2

static uint8_t payload_byte(size_t index) {
    return (uint8_t)(index % 251u);
}

static int pipe_create(size_t capacity, uintptr_t *read_handle, uintptr_t *write_handle) {
    uintptr_t out[2];
    intptr_t result;

    out[0] = 0;
    out[1] = 0;
    result = cellos_syscall4(CELLOS_SYSCALL_PIPE_CREATE, capacity, (uintptr_t)out, 0, 0);
    if (result != 0) {
        return -1;
    }
    *read_handle = out[0];
    *write_handle = out[1];
    return 0;
}

static int pipe_close(uintptr_t handle) {
    return cellos_syscall4(CELLOS_SYSCALL_PIPE_CLOSE, handle, 0, 0, 0) == 0 ? 0 : -1;
}

static int read_exact(uintptr_t handle, uint8_t *dst, size_t count) {
    size_t at = 0;

    while (at < count) {
        intptr_t read = cellos_syscall4(CELLOS_SYSCALL_PIPE_READ, handle, (uintptr_t)(dst + at),
                                        count - at, 0);
        if (read <= 0) {
            return -1;
        }
        at += (size_t)read;
    }
    return 0;
}

/* Drain to EOF, checking that byte `n` of the stream is `n % 251`. */
static int read_ordered_payload(uintptr_t handle, uint8_t *scratch, size_t capacity) {
    size_t total = 0;

    for (;;) {
        intptr_t read = cellos_syscall4(CELLOS_SYSCALL_PIPE_READ, handle, (uintptr_t)scratch,
                                        capacity, 0);
        size_t index;
        if (read < 0) {
            return -1;
        }
        if (read == 0) {
            return (int)total;
        }
        for (index = 0; index < (size_t)read; index++) {
            if (scratch[index] != payload_byte(total + index)) {
                return -1;
            }
        }
        total += (size_t)read;
    }
}

/* Read one child's report and verify every field against what the child was
 * told to send; `expected_items` is the only field that differs per launch. */
static int read_child_report(uintptr_t handle, uintptr_t expected_items) {
    uintptr_t header[HEADER_WORDS];
    uint8_t scratch[64];
    int drained;

    if (read_exact(handle, (uint8_t *)header, sizeof header) != 0) {
        cellos_log_text("[c-spawn] child header read failed\n");
        return -1;
    }
    if (header[0] != expected_items || header[1] != 1 || header[2] != PAYLOAD
        || header[3] != CHILD_STATUS) {
        cellos_log_value("[c-spawn] header items=", header[0]);
        cellos_log_value("[c-spawn] header accepted=", header[1]);
        cellos_log_value("[c-spawn] header payload=", header[2]);
        cellos_log_value("[c-spawn] header status=", header[3]);
        return -1;
    }
    drained = read_ordered_payload(handle, scratch, sizeof scratch);
    if (drained != (int)PAYLOAD) {
        return -1;
    }
    cellos_log_value("[c-spawn] ordered payload bytes=", (uintptr_t)drained);
    cellos_log_value("[c-spawn] child reported status=", header[3]);
    return 0;
}

/* The lifecycle barrier. `Wait` publishes the status only while the kernel still
 * holds the child's record; once the child has left the task table it has still
 * crossed the same terminal boundary, which is what this call establishes. */
static int child_is_terminal(cellos_child *child, int *published) {
    int status = 0;
    int waited = cellos_child_wait(child, &status);

    if (waited == CELLOS_SPAWN_EREAPED) {
        cellos_log_text("[c-spawn] wait already-terminal\n");
        return 0;
    }
    if (waited != CELLOS_SPAWN_OK) {
        cellos_log_value("[c-spawn] wait refused code=", (uintptr_t)(0 - waited));
        return -1;
    }
    if (status != CHILD_STATUS) {
        cellos_log_value("[c-spawn] published status=", (uintptr_t)status);
        return -1;
    }
    cellos_log_value("[c-spawn] wait published status=", (uintptr_t)status);
    *published = status;
    return 0;
}

int cellos_spawn_witness(uintptr_t elf_grant, size_t elf_len) {
    static const char *const kArguments[] = {"alpha", "beta gamma", (const char *)0};
    uintptr_t handles[2];
    uintptr_t granted[1];
    cellos_child child;
    cellos_spawn_request request;
    int published = 0;
    int launched;

    if (pipe_create(CAPACITY, &handles[0], &handles[1]) != 0) {
        return 20;
    }

    granted[0] = handles[1];
    request.target = CHILD_PATH;
    request.elf_grant = elf_grant;
    request.elf_len = elf_len;
    request.argv = kArguments;
    request.grant_handles = granted;
    request.grant_count = 1;

    launched = cellos_spawn(&request, &child);
    if (launched != CELLOS_SPAWN_OK) {
        /* Report the named code, not just "refused": EARGV means the kernel did
         * not take the staged command line, EDENIED means the launch edge, and
         * ELAUNCH means the launch itself. */
        cellos_log_value("[c-spawn] spawn refused code=",
                         (uintptr_t)(launched < 0 ? -launched : launched));
        return 21;
    }
    cellos_log_value("[c-spawn] launched tid=", child.tid);
    cellos_log_text("[c-spawn] route elf-bytes reviewed edge\n");

    /* The launcher must not keep the write end, or EOF never arrives. */
    if (pipe_close(handles[1]) != 0) {
        return 22;
    }
    if (child_is_terminal(&child, &published) != 0) {
        return 23;
    }
    if (read_child_report(handles[0], EXPECTED_ITEMS) != 0) {
        cellos_log_text("[c-spawn] child report mismatch\n");
        return 24;
    }
    cellos_log_value("[c-spawn] child argv items=", EXPECTED_ITEMS);
    if (pipe_close(handles[0]) != 0) {
        return 25;
    }

    /* ── Negative: an unreviewed target cannot be launched ─────────────────── */
    request.target = UNREVIEWED_PATH;
    request.argv = (const char *const *)0;
    request.grant_handles = (const uintptr_t *)0;
    request.grant_count = 0;
    if (cellos_spawn(&request, &child) != CELLOS_SPAWN_EDENIED) {
        return 40;
    }
    if (child.tid != 0) {
        return 41;
    }
    cellos_log_text("[c-spawn] unreviewed target denied\n");

    /* ── Negative: an over-long command line is refused before the kernel ─── */
    request.target = CHILD_PATH;
    request.argv = kOverlongArgv;
    if (cellos_spawn(&request, &child) != CELLOS_SPAWN_EARGV) {
        return 42;
    }
    if (child.tid != 0) {
        return 44;
    }
    cellos_log_text("[c-spawn] overlong argv rejected\n");

    /* ── Negative: an empty target names no reviewed edge ──────────────────── */
    request.target = "";
    request.argv = (const char *const *)0;
    if (cellos_spawn(&request, &child) != CELLOS_SPAWN_ETARGET) {
        return 43;
    }

    /* ── Second launch: a denied attempt left no command line behind ───────── */
    if (pipe_create(CAPACITY, &handles[0], &handles[1]) != 0) {
        return 50;
    }
    granted[0] = handles[1];
    request.target = CHILD_PATH;
    request.elf_grant = elf_grant;
    request.elf_len = elf_len;
    request.argv = (const char *const *)0;
    request.grant_handles = granted;
    request.grant_count = 1;
    if (cellos_spawn(&request, &child) != CELLOS_SPAWN_OK) {
        return 51;
    }
    if (pipe_close(handles[1]) != 0) {
        return 52;
    }
    if (child_is_terminal(&child, &published) != 0) {
        return 53;
    }
    if (read_child_report(handles[0], 0) != 0) {
        cellos_log_text("[c-spawn] unargumented child report mismatch\n");
        return 54;
    }
    if (pipe_close(handles[0]) != 0) {
        return 55;
    }
    cellos_log_text("[c-spawn] denied launch left no staged argv\n");
    return 0;
}
