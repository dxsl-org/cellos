# Phase 17a — Enhanced Shell (Pipes, Redirects, Jobs, Readline)

**Effort:** 160h (of 320h total; see phase-17b for utilities) | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 13, Phase 14, Phase 15

## Overview

Upgrade the shell from a minimal REPL to a feature-rich POSIX-like shell: pipelines, I/O redirection, background jobs, job control (fg/bg/jobs), command history, tab completion, line editing. Foundations for any productive use. Companion phase 17b adds the standard utility binaries.

## Context Links

- `docs/11-shell.md` — shell architecture
- `cells/apps/shell/src/shell.rs` — current REPL
- `cells/apps/shell/src/commands.rs` — current built-ins
- Phase 13 (VFS write enables redirection), Phase 14 (KeyEvent enables proper line editing), Phase 06 (SpawnFromPath enables exec of arbitrary `/bin/` programs)

## Key Insights

- Pipes between cells in ViOS aren't POSIX file descriptors — they are owned-buffer IPC channels (Phase 07 capability + buffer transfer). Shell builds a chain: A's stdout cap is wired to B's stdin cap before spawn.
- Background jobs: each command line becomes a "Job" (one or more Cells in a pipeline). Shell tracks {job_id, cells: Vec<CellId>, state: Running/Stopped/Done}. `Ctrl+Z` sends Pause to all cells in the foreground job; `fg`/`bg` resumes; `jobs` lists.
- Line editing needs proper key handling — depends on Phase 14's KeyEvent (not raw bytes). Implement minimal readline: arrows for history, Home/End for line nav, Ctrl+A/E shortcuts, Backspace/Delete, history file persisted to `~/.vios_history` via VFS.
- Tab completion: walk the directories on `PATH` to list matching prefixes; also complete on file paths for second args. Cache PATH dir entries with mtime invalidation.

## Requirements

**Functional**
- Pipelines: `cat /etc/hosts | grep 127`
- Redirects: `echo hi > /tmp/a.txt`, `cat < /tmp/a.txt`, `cmd 2> /tmp/err.log`, `cmd &>>/tmp/all.log`
- Background: `sleep 10 &`, prints `[1] <pid>`
- Job control: `jobs`, `fg %1`, `bg %1`, `kill %1`, Ctrl+Z stop
- Line editing: arrows for history nav, Home/End, Ctrl+A/E/W/U/K
- Tab completion: command names (PATH walk) + file paths
- History persisted across boot

**Non-functional**
- Line-edit latency < 5ms per keystroke
- History file capped at 10000 lines
- Tab completion < 50ms for 1000 entries on PATH

## Architecture

```
Shell process structure:
  ┌─────────────────────────────────────┐
  │ shell.rs main loop                  │
  │   read_key() (Phase 14 KeyEvent)    │
  │   readline state machine            │
  │     ↓ enter
  │   parser: tokenize → ast            │
  │     ↓
  │   executor: walk ast                │
  │     - simple cmd: spawn, wait       │
  │     - pipeline: spawn each + wire   │
  │       caps stdout(i) -> stdin(i+1)  │
  │     - redirect: open file cap,      │
  │       hand to child as stdin/stdout │
  │     - background: don't wait, push  │
  │       to jobs                       │
  │     ↓
  │   wait_or_yield()                   │
  └─────────────────────────────────────┘
```

## Related Code Files

**Modify:**
- `cells/apps/shell/src/main.rs`
- `cells/apps/shell/src/shell.rs` — main REPL → split into shell-sized pieces
- `cells/apps/shell/src/commands.rs` — built-ins (cd, jobs, fg, bg, exit, history, alias, export)
- `cells/apps/shell/src/async_utils.rs`
- `libs/ostd/src/io.rs` — add Pipe primitive built on IPC + owned buf

**Create:**
- `cells/apps/shell/src/parser.rs` — tokenizer + AST (Cmd, Pipeline, Redirect, Background, Sequence)
- `cells/apps/shell/src/executor.rs` — walk AST, spawn cells, wire caps
- `cells/apps/shell/src/readline.rs` — line editing state machine
- `cells/apps/shell/src/history.rs` — history file read/write/dedupe
- `cells/apps/shell/src/completion.rs` — tab completion
- `cells/apps/shell/src/jobs.rs` — Jobs table + control
- `cells/apps/shell/src/path.rs` — PATH lookup + caching
- `cells/apps/shell/src/aliases.rs` — alias table (read from `~/.viosrc`)
- `tests/integration/shell_pipeline.rs` — pipeline + redirect end-to-end
- `tests/integration/shell_jobs.rs` — Ctrl+Z + fg + bg flow
- `tests/integration/shell_history.rs` — history persists across "boot" (re-init shell)

## Implementation Steps

1. **Parser** `cells/apps/shell/src/parser.rs`:
   - Tokenize: handle quoted strings (single/double), escapes, special chars `|`, `<`, `>`, `>>`, `&`, `;`, `&&`, `||`
   - Build AST: `Sequence(Vec<Stmt>)`, `Stmt::Pipeline { stages: Vec<Cmd>, background: bool }`, `Cmd { argv, redirs }`, `Redir { kind, target_path or fd }`
   - Test cases: `a | b | c`, `a > x.txt`, `a && b || c`, `a; b &`
2. **Executor** `cells/apps/shell/src/executor.rs`:
   - For simple Cmd: lookup PATH → SpawnFromPath; wait on its CellId
   - For Pipeline: spawn all stages; create IPC pipe caps for each junction; pass `stdin_cap` / `stdout_cap` as Cell args (or via cell metadata at spawn — decide one and document)
   - For Redirects: open file via VFS → pass FileHandle as stdin/stdout cap to spawned cell
   - For Background: spawn but don't wait; register Job
3. **OSTD `Pipe`** primitive in `libs/ostd/src/io.rs`:
   - `Pipe::new() -> (PipeReader, PipeWriter)`
   - Backed by a kernel-allocated SPSC ring + waker pair
   - Writer.write(buf) returns when space; Reader.read(buf) blocks until data
4. **Readline** `cells/apps/shell/src/readline.rs`:
   - State: current line `String`, cursor `usize`, history index `Option<usize>`
   - Handle KeyEvent variants:
     - printable → insert at cursor
     - Backspace → delete left of cursor
     - Delete → delete right of cursor
     - Arrows Up/Down → history nav (load line into buffer, cursor at end)
     - Arrows Left/Right → cursor move
     - Home/End or Ctrl+A/E → cursor to start/end
     - Ctrl+K → kill to end of line; Ctrl+U → kill to start
     - Ctrl+W → kill word left
     - Tab → invoke completion module
     - Enter → submit line
   - Re-render line after each edit (cursor positioning ANSI escapes if terminal supports them; else clear-and-print)
5. **History** `cells/apps/shell/src/history.rs`:
   - In-memory `VecDeque<String>` capacity 10000
   - On boot: read `~/.vios_history` line by line
   - On Enter: append line if non-empty + non-duplicate-of-previous
   - On exit: flush back to file
6. **Completion** `cells/apps/shell/src/completion.rs`:
   - Determine context: first token → command completion; later tokens → file completion
   - Walk dirs from `path::lookup_dirs()`, list executable files
   - For file completion, parse current arg as prefix path; readdir parent dir; filter by prefix
   - Show options if ambiguous (>1 match)
7. **Jobs** `cells/apps/shell/src/jobs.rs`:
   - Table `Vec<Job { id, cells: Vec<CellId>, state }>`
   - On Ctrl+Z (from KeyEvent.modifiers): send Pause syscall to foreground job's cells; mark state Stopped; release foreground; print `[1]+ Stopped …`
   - `fg %1`: take cells, mark Running, attach as foreground, wait
   - `bg %1`: send Resume but don't attach foreground
   - `kill %1`: send Kill to all cells in job
   - Reaper: on cell exit, mark job Done; cleanup
8. **Path cache** `cells/apps/shell/src/path.rs`:
   - Parse `$PATH` (from `Config::get("env.PATH")` or default `/bin:/usr/bin`)
   - Cache `BTreeMap<dirname, (mtime, Vec<filename>)>`
   - Invalidate entry when stat reports newer mtime
9. **Aliases** `cells/apps/shell/src/aliases.rs`:
   - On boot read `~/.viosrc`; lines `alias ll='ls -la'`
   - Before parser tokenizes, expand first token if it matches an alias (one level — no recursive expansion to avoid infinite loops)
10. **Integration tests**:
    - `shell_pipeline.rs`: `echo abc | grep b` produces `abc`
    - `shell_jobs.rs`: `sleep 10 &` + `jobs` lists job; `kill %1` removes
    - `shell_history.rs`: enter 3 lines, restart shell, history has 3 lines

## Todo List

- [ ] Parser (tokenizer + AST + tests)
- [ ] Executor (simple, pipeline, redirect, background)
- [ ] OSTD `Pipe` primitive
- [ ] Readline state machine + line rendering
- [ ] History file load/save
- [ ] Tab completion (command + file)
- [ ] Jobs table + Ctrl+Z + fg/bg/kill
- [ ] PATH cache with mtime invalidation
- [ ] Aliases (`~/.viosrc`)
- [ ] Integration tests: pipeline, jobs, history
- [ ] CI green

## Success Criteria

- `cat /etc/hosts | grep 127` outputs only matching lines
- `echo hi > /tmp/a.txt && cat /tmp/a.txt` round-trips
- `sleep 5 &` returns immediately; `jobs` shows it; auto-removed when done
- Up/Down arrow navigates history; Tab completes filenames
- Ctrl+Z stops foreground; `fg` resumes
- History persists across reboot (`~/.vios_history`)
- Latency < 5ms per keystroke

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Pipe cap transfer semantics race (parent loses cap before child receives) | High | Med | Atomic spawn-with-caps: kernel transfers caps as part of spawn syscall, not as separate IPC |
| Ctrl+Z interception interferes with cells that want raw key events | Low | Med | Modifier+key combos consumed by shell only when in foreground; pass-through to cells when surface focus is theirs (Phase 16) |
| Readline ANSI cursor moves wrong when terminal width unknown | High | Low | Compute display column by counting since last newline; treat unknown-width as 80 |
| History file growth unbounded | Cert | Low | Cap to 10000 lines on write; trim oldest |
| Tab completion blocks on slow VFS readdir | Med | Low | Run completion off the readline tick; cancellable if next keystroke arrives |
| Alias recursion despite one-level rule | Low | Low | Visit set tracks expanded aliases per line; bail after 8 hops |

## Security Considerations

- Job control limited to children of this shell — shell cannot signal arbitrary cells
- History file readable only by the user's cell (per-user FS perms in v1.x; for v1.0 single-user)
- Alias expansion does NOT inherit privileges from the alias defining cell

## Rollback

Revert restores minimal REPL. Phase 17b's utilities still work via simple `/bin/<tool>` invocation; they just lose pipe/redirect composability.

## Next Steps

Phase 17b builds the standard utility binaries that exercise these shell features. Phase 18 (runtimes) gives Lua/MicroPython interactive REPL via the same readline. Phase 22 benchmarks shell-to-cell exec latency.
