# Phase 17b — Standard POSIX-style Utilities

**Effort:** 160h (of 320h total; companion to phase-17a) | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 17a, Phase 13, Phase 15

## Overview

Ship the standard set of command-line utilities that make ViOS feel like a real OS: file ops (cp, mv, rm, mkdir, touch, find), text tools (cat, grep, sed, sort, wc, head, tail), system tools (ps, kill, top, shutdown, env, uname), and network tools (ping, curl, nc, wget). Each is a separate Cell binary under `cells/apps/utils`, `cells/apps/sys-tools`, `cells/apps/net-tools`.

## Context Links

- Phase 17a — shell with pipes/redirects/jobs to exercise these
- Phase 13 — VFS full read/write
- Phase 15 — network for net-tools
- `cells/apps/utils/src/bin/` — current location for utils (verify; may live as separate crates)
- `docs/11-shell.md` — shell utility expectations

## Key Insights

- Each utility is its own `cells/apps/<area>/src/bin/<name>.rs` building to a separate `/bin/<name>` ELF. Keeps per-binary size small and lets the shell `SpawnFromPath` to invoke any of them.
- Many tools share patterns: argv parsing, stdin read loop, line-iterator, exit code. Centralize in a tiny `libs/ostd-cli` helper module (or as part of OSTD) — `read_lines(stdin) → iterator`, `parse_args(spec)` — to avoid DRY violations.
- Avoid pulling in clap or other heavy arg-parsers for v1.0; a hand-rolled minimal parser is ~50 lines and ships smaller.
- Some commands (`ps`, `top`, `kill`) need cell-listing/cell-control APIs — define them in Phase 17b via Config Cell + new syscalls.

## Requirements

**Functional**
- 17 utilities total split across 3 cells:
  - **utils**: cat, cp, mv, rm, mkdir, touch, find, grep, sed, sort, wc, head, tail, ls (already exists; extend)
  - **sys-tools**: ps, kill, top, shutdown, env, uname, date, free
  - **net-tools**: ping, curl, nc, wget
- All utilities respect stdin/stdout/stderr from shell's pipe/redirect machinery
- All return correct POSIX-like exit codes (0 = success, non-zero = error)

**Non-functional**
- Per-binary < 200 KB
- Startup < 20ms (spawn + arg parse + first output)
- No `unsafe` in any utility cell

## Architecture

```
shell                         shell.exec("grep foo < /etc/hosts | wc -l")
  │ spawn /bin/grep, /bin/wc
  ▼                                  
cells/apps/utils/src/bin/grep.rs
  ├─ parse args (-i, -v, -E, etc.)
  ├─ open input (stdin or file from args)
  ├─ for each line: regex match (libs/regex tiny crate)
  ├─ write matches to stdout
  └─ exit(0 if matches, 1 if none)
```

## Related Code Files

**Modify:**
- `cells/apps/utils/src/bin/ls.rs` (if exists; extend with `-l`, `-a`, `-h`)
- `cells/apps/utils/Cargo.toml` — register all `[[bin]]` targets
- `libs/ostd/src/lib.rs` — re-export new `cli` helper module
- `gen_disk.ps1` — bake all `/bin/<name>` binaries into the disk image

**Create (cells/apps/utils/src/bin/):**
- `cat.rs`, `cp.rs`, `mv.rs`, `rm.rs`, `mkdir.rs`, `touch.rs`, `find.rs`
- `grep.rs`, `sed.rs`, `sort.rs`, `wc.rs`, `head.rs`, `tail.rs`

**Create cells/apps/sys-tools/ (new cell crate):**
- `cells/apps/sys-tools/Cargo.toml`
- `cells/apps/sys-tools/src/bin/ps.rs`, `kill.rs`, `top.rs`, `shutdown.rs`, `env.rs`, `uname.rs`, `date.rs`, `free.rs`

**Create cells/apps/net-tools/ (new cell crate):**
- `cells/apps/net-tools/Cargo.toml`
- `cells/apps/net-tools/src/bin/ping.rs`, `curl.rs`, `nc.rs`, `wget.rs`

**Create shared helpers:**
- `libs/ostd/src/cli.rs` — `parse_args(spec)`, `read_stdin_lines()`, `write_stdout(bytes)`, `eprintln`, `exit_with_code`
- `libs/regex-mini/` (NEW crate) — minimal regex engine (POSIX BRE subset) for grep/sed; or vendor an existing crate (e.g. `regex-lite` if no_std-compatible). Decision: pick `regex-lite` if available; else implement subset
- `tests/integration/utilities_smoke.rs` — drive shell through each utility, assert exit code + key output

**Modify (new syscalls for sys-tools):**
- `libs/api/src/syscall.rs` — add `ListCells() → Vec<CellInfo>`, `KillCell(id, sig)`, `Shutdown(mode)`, `Uptime() → Duration`, `MemInfo() → MemStats`
- `kernel/src/task/syscall.rs` — implement dispatchers

## Implementation Steps

### Phase 17b.1 — shared helpers (16h)

1. Implement `libs/ostd/src/cli.rs`:
   - `pub struct ArgSpec { …  }` declarative arg layout
   - `parse(argv: &[&str], spec: &ArgSpec) -> Result<ParsedArgs, ExitWith>` 
   - `read_lines(stdin: Stdin) -> impl Iterator<Item = String>`
   - `println!`-style macros backed by stdout cap (not console direct)
2. Integrate `regex-lite` or implement a tiny BRE engine in `libs/regex-mini/src/lib.rs`
3. Tests for these helpers

### Phase 17b.2 — utils crate (40h)

4. For each of 13 utilities, implement under `cells/apps/utils/src/bin/<name>.rs`:
   - `cat`: read each arg-file (or stdin), write to stdout, `-n` for line numbers
   - `cp`: src dst (file or dir); `-r` recursive; preserve perms (where applicable)
   - `mv`: src dst; same FS = rename, cross-FS = copy+delete
   - `rm`: paths; `-r` recursive; `-f` force ignore-missing
   - `mkdir`: path(s); `-p` create parents
   - `touch`: path(s); update mtime or create empty
   - `find`: path, optional `-name`, `-type`, `-size` filters; recursive walk
   - `grep`: pattern + files (or stdin); `-i`, `-v`, `-n`, `-c`, `-E`
   - `sed`: limited subset: `s/pat/rep/[g]`, `-i` in-place
   - `sort`: stdin lines; `-r`, `-n`, `-u`; in-memory sort (cap 100k lines)
   - `wc`: counts; `-l`, `-w`, `-c`
   - `head`: first N lines/bytes; `-n N`, `-c N`
   - `tail`: last N lines/bytes; `-f` follow (uses VFS notify)

### Phase 17b.3 — sys-tools crate (60h)

5. Add syscalls in `libs/api/src/syscall.rs`:
   - `ListCells() → Box<[CellInfo]>` (id, name, state, rss_bytes)
   - `KillCell(id: CellId, sig: Signal) → Result`
   - `Shutdown(SoftReboot | HardReboot | Halt | PowerOff)`
   - `Uptime() → u64` (ns since boot)
   - `MemInfo() → MemStats { total, free, used }`
6. Implement kernel dispatchers in `kernel/src/task/syscall.rs`
7. Implement utilities:
   - `ps`: `ListCells()` → format table; `-e` all, `-f` full
   - `kill`: parse signal arg + cell id; `KillCell(id, sig)`
   - `top`: redraw every 1s with ps + memory
   - `shutdown`: parse mode (`-h`, `-r`); `Shutdown(...)`
   - `env`: print or set environment (Config Cell)
   - `uname`: `-a`, `-r`, `-m`; reads from Config (`system.version`, `system.arch`)
   - `date`: read kernel time; `+format` strftime subset
   - `free`: `MemInfo()` formatted

### Phase 17b.4 — net-tools crate (40h)

8. Build on Phase 15's TcpStream/UdpSocket OSTD wrappers:
   - `ping`: ICMP via raw socket (requires `NET_RAW` cap — gated)
   - `curl`: HTTP/1.1 GET only; parse URL, connect, send request, print response; `-X POST -d` for body
   - `nc`: TCP client + server; bidirectional pipe to stdin/stdout
   - `wget`: HTTP GET, save to file (combines curl + VFS write)

### Phase 17b.5 — integration tests (4h)

9. `tests/integration/utilities_smoke.rs`:
   - For each utility: drive shell to run a canonical invocation; assert expected exit code + stdout contents
   - Examples: `ls /bin | wc -l > 10`, `grep root /etc/passwd`, `head -n 2 /etc/hosts`, `ps | grep shell`

### Phase 17b.6 — disk image (verify)

10. `gen_disk.ps1` updated to bake all `/bin/<name>` binaries; verify all utilities reachable from `PATH`

## Todo List

- [ ] Implement `libs/ostd/src/cli.rs` (shared arg parser + IO helpers)
- [ ] Choose & integrate regex crate (or write minimal BRE)
- [ ] Implement 13 `utils` binaries
- [ ] Add 5 new syscalls (ListCells, KillCell, Shutdown, Uptime, MemInfo) + kernel dispatchers
- [ ] Implement 8 `sys-tools` binaries
- [ ] Implement 4 `net-tools` binaries
- [ ] Bake binaries into disk image via `gen_disk.ps1`
- [ ] Integration test for each utility (canonical invocation + exit code)
- [ ] CI green

## Success Criteria

- `cat /etc/hosts | grep localhost | wc -l` returns correct count
- `ls /bin` shows all 25+ utilities
- `ps` lists all running cells; `kill <id>` terminates
- `curl http://10.0.2.2/` retrieves an HTTP response
- `ping 10.0.2.2` succeeds (or graceful error if NET_RAW not granted)
- `shutdown -h` cleanly halts QEMU
- Per-binary < 200 KB
- All utilities zero-`unsafe`

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Regex crate `regex-lite` may not be no_std-friendly | Med | Med | Fallback: implement BRE subset (POSIX char classes, *, ., [], anchors) |
| `ps` exposes too much info (info disclosure across cells) | Med | Med | Per-cell: a cell sees its own + children only, unless `OBSERVE_ALL` cap held |
| `kill` lets a cell terminate any other (privilege bug) | Med | High | Same gating: only `OBSERVE_ALL` + `SIGNAL_ANY` caps allow cross-cell kill |
| `find -size` slow on large FS | Med | Low | Document v1.0 limitation; cap traversal at 10K entries |
| `tail -f` requires VFS notify; not in Phase 13 scope | Cert | Low | Implement `tail -f` as periodic re-stat poll for v1.0; document |
| `curl` minimal HTTP/1.1 doesn't handle redirects, gzip | Cert | Low | Document as v1.0 limitations; matches `curl --version` minimal |
| Total LOC blow-up (~5K lines new code) | Cert | Med | Strict DRY via cli.rs; review each binary < 200 LOC |

## Security Considerations

- `ps`/`kill` gated by capability — default cell cannot list/kill other cells
- `curl`/`wget`/`nc` use network cap from Phase 15 (already capability-checked)
- `find` respects per-cell mount visibility — cannot walk paths cell can't see
- `shutdown` requires `POWER` cap (only the `init` cell holds by default; can be granted to a privileged admin cell)

## Rollback

Each crate (utils, sys-tools, net-tools) is independent. Revert per-crate restores the prior subset. Shell pipe/redirect machinery (Phase 17a) still works; just fewer programs to compose.

## Next Steps

Phase 18 — runtimes use these utilities (e.g., `lua` script invoking `os.execute("ls /")`). Phase 22 benchmarks startup time. Phase 23 community can contribute new tools via good-first-issue.
