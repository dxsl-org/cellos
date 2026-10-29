# Phase 03 amendment A3 — external byte stages and mixed foreground jobs

Status: **approved by the user (“Duyệt A3”); implemented in the §3 file closure; gates G1 PASS, G2 FAIL, G3 NOT PROVEN — per §4 work stopped; a kernel/ABI-level amendment is required (see Results).** No source, ABI, selector, kernel, lock or image change is made by this document. It extends the Phase 03 contract ([plan step 4 and 6](phase-03-typed-pipelines.md)) after the CP2-approved Phase 02 baseline ([dossier](phase-02-cp2-dossier.md)). Evidence of the gap: [phase-03 runtime evidence](reports/phase-03-runtime-evidence.md) "Not met".

## 1. Problem
The shell refuses any typed/byte pipeline containing a spawned (external) Cell, because its only spawn path, `TerminalSession::spawn_foreground` (`libs/terminal/src/session.rs:176-221`), wires the child's stdin to the keyboard owner (`authorize_input`, input-service receipts). A pipeline needs the child's stdin/stdout to be shell-owned byte streams instead.

## 2. Finding that makes a kernel change unnecessary (from source; **not yet proven on an image**)
- The kernel already publishes a prepared child whose stdin/stdout/stderr are arbitrary delegated endpoints with no input-service receipt: `Operation::prepare_child` → `Endpoint::delegate` → `PreparedChild::write_context` → `PreparedChild::commit` (`libs/terminal/src/stream.rs:31-79`). The private Phase02 parent fixture does exactly this (`cells/tests/phase02-session-parent/src/main.rs:100-116`) and passed the descendant/duplicate/cancel scenarios.
- Exit status of any task is observable via the existing `Syscall::Wait { pid }` (`kernel/src/task/syscall.rs:5411-5450`, already in the shell's allowlist), using `ChildRefV1.root_tid` returned by commit.
- Legacy cells read fd 0 / write fd 1,2 through a committed child context's endpoints ("context-present committed child routes fd0/1/2", Phase02 A-notes), so ordinary `/bin` tools need no change.
- Launch authority stays `shell-launch-edge` (`kernel/src/loader/launch_profile/profiles.rs:56-63`): only `reviewed_user_target_ceiling` targets, same ceiling as today's external launch. No new rights.

## 3. Proposed change (userspace only)
Files (closure; no recursive unlock):
- `libs/terminal/src/{pipe_child.rs (new), lib.rs}`: new `PipeChild` API — `spawn_piped(parent: OperationRef-owning session, path, argv, cwd, env, stdin: Option<Endpoint reader>, stdout: Option<Endpoint writer>, stderr: Endpoint writer) -> PipeChild { child, root_tid, operation }`, `wait(&PipeChild) -> Result<i32, Status>` (sys_wait), `cancel(reason)`, `close()`; cwd handled with the same VFS prepared-cwd reservation that `spawn_foreground` already uses (no new VFS selector); every failure path cancels and closes the child operation (same bounded close as `close_foreground`). No `authorize_input`/`bind_receipt`/foreground lease: the child never owns the keyboard.
- `cells/tools/shell/src/{typed,executor,shell_state,jobs,main}.rs`: lift the "external stage refused" admission rule for these forms only: `rows → to … → external`, `external → from json|jsonl [--typed] → rows`, `external | external`, `cat F | external`, external as last stage to terminal/redirect. The shell pumps bytes between 4 KiB stream rings (existing `create_stream(4096)`), drains each child's stderr separately to the shell stderr sink (never into a record stream), and keeps reading the session's own event queue so Ctrl-C cancels the whole pipeline operation.
- `libs/ostd`, `libs/api`, kernel, input/VFS services, lock files: **untouched**.

Behaviour contract:
1. Whole-pipeline admission (existing) still runs before any spawn or redirect; limits stay ≤16 stages, ≤16 simultaneous child operations, 4 KiB ring/edge, record/frame limits from spec.md.
2. Pipeline status = last stage's status; earlier non-zero stage statuses are reported on stderr (existing policy); a killed/cancelled stage reports 130; spawn failure aborts the pipeline, cancels already-started stages and reports 126/127 as today.
3. Consumer close (`take`, redirect failure, downstream exit) closes the reader endpoint; upstream writers get `BrokenPipe`; the shell then cancels and reaps each remaining stage (bounded wait, then cancel + close). EOF = last-writer close, as in Phase 02.
4. One job identity per pipeline; `jobs` shows member statuses. **Foreground only.** `&` on any mixed/external-typed pipeline, `fg`, `bg` remain refused with status 2 before effects (no background job control is invented). Parent cwd/env never mutate; stateful builtins stay refused inside pipelines.
5. Redirect semantics unchanged (`>`/`>>`/`<`, `2>` refused on typed pipelines); redirect target written only after pipeline success.
6. Terminal interaction (raw/echo, `top`, `rm -i`) remains a separate session contract; interactive stages are refused inside pipelines.

## 4. Gates (stop and request a new amendment if any fails)
- **G1 probe:** on the private image, a `shell`-spawned `PipeChild` for `/bin/cat`/`/bin/echo` must commit without input receipts under the **ordinary** (non-`phase02-terminal-profile`) launch profile. If the kernel refuses, a kernel/launch-profile change is required and is **not** approved by this document.
- **G2 probe:** `Syscall::Wait` on the committed `root_tid` returns the real exit code; if not, status propagation is reported as unavailable rather than guessed.
- **G3:** `/bin/cat`, `/bin/echo`, `/bin/ps`, `/bin/ls` behave as stdin/stdout filters under delegated endpoints with exact bytes (NUL, non-UTF-8, no trailing newline).

## 5. Verification (private signed x86 image only; no canonical artifact touched)
Runner `scripts/qemu-phase03-pipeline-test.sh` gains: `ps --records | to jsonl | /bin/cat` (bytes identical to builtin path), `/bin/echo … | from jsonl | to jsonl` (round trip), exact binary bytes through `cat F | cat`, `take`-closes-an-endless-producer (external stage reaped, no leak: child count back to baseline), Ctrl-C mid-pipeline cancels and the prompt recovers, non-zero stage status propagation, stderr separation (diagnostic not in rows), unknown-command / bad path failing before any effect. Host tests for the `PipeChild` argument/limit validation. Existing Phase 02 fixture (48 checks) and Phase 03 (15) must still pass.

## 6. Rollback
Revert only the files in §3; the previous refusal path remains. No persisted state besides files a pipeline wrote by explicit redirect; delivered bytes cannot be recalled. Quarantined artifacts, CP2, rollout and Phases 04–10 remain blocked; this does not approve Phase 03 CP2.

## Results (2026-10-21, private signed x86 image, TCG; [raw output](reports/phase-03-evidence/pipeline-commands-a3.json))
Host: terminal-sdk 32, app-shell 61, text-engine 74 tests pass; shell compiles bare-metal with/without `shell_test`.
- **G1 PASS.** A shell-spawned `PipeChild` for `/bin/cat` and `/bin/echo` commits under the ordinary launch profile with no input receipt; an unknown program is denied at the launch edge (`DENY launch edge … /bin/p03-nosuchprog`) before any upstream stage runs and no rows reach the output.
- **G2 FAIL.** `Syscall::Wait` on the committed `root_tid` returns `Stale` for a child that exits quickly (kernel `Wait`: unknown/already-reaped task → refusal, `syscall.rs:5411-5430`). The shell reports `shell: /bin/cat: exit status unavailable: Stale`; exit codes of fast-exiting stages are not observable from userspace. Fixing it needs a retained, preparer-queryable terminal record (the kernel already keeps `exit_code`/`terminal_sequence` in receipts, but `QueryReceiptChild` is issuer-only) — a kernel/ABI change that A3 does not approve.
- **G3 NOT PROVEN.** `/bin/cat` takes a path argument (`usage: cat <path>` on empty args) and `/bin/echo` ignores stdin; **no stdin-reading external filter exists in the image**, so exact-byte stdin→stdout through delegated endpoints is untested. A private fixture filter cell would need its own approval (workspace member + lock change, outside §3).
- Observed side effect: a stage that exits without reading stdin makes its neighbours fail with `Cancelled` (kernel final-reader-close teardown), so `/bin/echo x | /bin/cat` fails at spawn.
- Passing image checks unchanged for typed-only pipelines (14/14 earlier set); new external checks 1 PASS (unknown program) + 1 PASS (status visible) + 6 FAIL as above. Ctrl-C at the prompt recovered (`^C` then prompt), but it was exercised against an already-failed pipeline, not a long-running one.
