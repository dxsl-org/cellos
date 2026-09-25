/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Narrow C child-launch adapter for Cellos.
 *
 * This is not `fork`, `exec`, `posix_spawn`, a shell, or a PATH search, and it
 * deliberately offers none of their semantics. A launch is authorized only
 * when the kernel holds a reviewed launch-profile row for the exact
 * `(caller identity, route, target path)` edge, so the target is a reviewed
 * constant rather than caller-supplied authority: an arbitrary path string
 * cannot expand what a cell may start. Session, job control, signal and
 * process-group surfaces do not exist here, and a call that would need them is
 * absent rather than approximated.
 *
 * The adapter composes three existing kernel facilities and adds no syscall:
 *   * the caller-private staged command line (`StateStash`/`StateRestore` on the
 *     reserved argv key) that the kernel moves into the child before it is
 *     runnable,
 *   * `SpawnFromPath` under the reviewed launch edge, and
 *   * `PipeShare`, which duplicates an endpoint this task owns into the child's
 *     handle table.
 *
 * Endpoint handles are never carried in `argv`. The launch grants each endpoint
 * with `PipeShare` and then hands the child the resulting tokens over ordinary
 * IPC; the token is transport, and the kernel's per-task handle table is the
 * authority. A child therefore receives exactly the endpoints it was granted.
 */
#ifndef CELLOS_SPAWN_H
#define CELLOS_SPAWN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Encoded command-line bound the kernel enforces for one launch transaction:
 * the "\0argv1\0" prefix plus NUL-terminated items. */
#define CELLOS_SPAWN_ARGV_MAX 512

/* Longest reviewed target path accepted, including its terminating NUL. */
#define CELLOS_SPAWN_TARGET_MAX 128

/* Endpoints one launch may grant in a single transaction. */
#define CELLOS_SPAWN_MAX_GRANTS 8

/* Return codes. Every failure is negative and named; there is no errno and no
 * partial-success value. */
#define CELLOS_SPAWN_OK 0
#define CELLOS_SPAWN_EINVAL (-1)   /* malformed request struct */
#define CELLOS_SPAWN_ETARGET (-2)  /* target missing, unterminated, or too long */
#define CELLOS_SPAWN_EARGV (-3)    /* the encoded command line is over bound */
#define CELLOS_SPAWN_EDENIED (-4)  /* kernel refused the launch edge */
#define CELLOS_SPAWN_ELAUNCH (-5)  /* launch failed for another reason */
#define CELLOS_SPAWN_EGRANT (-6)  /* child exists, endpoint grant failed */
#define CELLOS_SPAWN_EWAIT (-7)   /* wait failed while the child was live */
#define CELLOS_SPAWN_EREAPED (-8) /* the kernel had already collected the child */

/* One launched child. `tid` is the kernel task id, which is also what `Wait`
 * and `PipeShare` address. */
typedef struct cellos_child {
    uintptr_t tid;
} cellos_child;

typedef struct cellos_spawn_request {
    /* Exact reviewed target path, e.g. "/bin/c-spawn-child". Never a search path
     * or a caller-derived string. */
    const char *target;
    /* A caller-owned Grant holding `target`'s ELF bytes, or 0 to let the kernel
     * resolve `target` itself.
     *
     * Both forms are the same reviewed edge: the kernel authorizes an exact
     * `(caller identity, route, target)` row and derives the child's ceiling from
     * it, so supplying bytes cannot widen authority — a lying path hint only
     * loses privilege. The grant form exists because post-boot the cell store is
     * served by the VFS service, not by the kernel: on this platform the kernel
     * drives no block hardware, so a caller that can reach its VFS fetches the
     * bytes and passes them here. The grant remains the caller's to free. */
    uintptr_t elf_grant;
    /* Bytes of ELF in `elf_grant`. Ignored when `elf_grant` is 0. */
    size_t elf_len;
    /* NULL-terminated argument items, or NULL for no arguments. An item is
     * therefore NUL-free by construction, and spaces or empty items survive
     * byte-for-byte; the total encoded command line is bounded. */
    const char *const *argv;
    /* Endpoints to duplicate into the child. A handle is a nonzero token and
     * becomes usable by the child only through this grant. */
    const uintptr_t *grant_handles;
    size_t grant_count;
} cellos_spawn_request;

/* Stage the command line, launch the reviewed child, grant it `grant_handles`,
 * and deliver their tokens. Synchronous: when this returns a value other than
 * CELLOS_SPAWN_OK / CELLOS_SPAWN_EGRANT no child was created.
 *
 * On CELLOS_SPAWN_EGRANT the child exists and `out->tid` is set, because a
 * launched child cannot be recalled without lifecycle authority. The caller may
 * still wait for it; its own channel use fails, which is the documented way a
 * half-established child terminates. */
int cellos_spawn(const cellos_spawn_request *request, cellos_child *out);

/* Block until the child is terminal. Returns CELLOS_SPAWN_OK and writes the
 * kernel-published exit status to *status when the waiter was registered before
 * the kernel collected the child.
 *
 * Returns CELLOS_SPAWN_EREAPED when the kernel had already collected the child:
 * a task leaves the task table only after crossing the terminal boundary, so
 * this still proves the child is gone — it only means the status is no longer
 * published on this channel. A launcher that needs the status must therefore
 * wait promptly (its own thread, immediately after the launch) or have the child
 * report its status over a granted endpoint, which is exact and race-free. This
 * mirrors the lifecycle convention the pthread adapter already documents. */
int cellos_child_wait(const cellos_child *child, int *status);

/* Child side: copy this task's staged command line into `buf` and return its
 * length. Returns 0 when no command line was staged, or (size_t)-1 on failure.
 * The payload is consumed: a second call reports 0. */
size_t cellos_spawn_argv_raw(char *buf, size_t capacity);

/* Split a payload from `cellos_spawn_argv_raw` into NUL-terminated items stored
 * in `buf`. Returns the item count, or (size_t)-1 when the payload is not the
 * structured wire format. A malformed payload fails loudly instead of falling
 * back to a whitespace guess. */
size_t cellos_spawn_argv_split(char *buf, size_t length, const char **items, size_t capacity);

/* Child side: receive the endpoint tokens granted by the launcher. Returns the
 * number written into `handles`, or (size_t)-1 on failure. */
size_t cellos_spawn_receive_grants(uintptr_t *handles, size_t capacity);

#ifdef __cplusplus
}
#endif
#endif
