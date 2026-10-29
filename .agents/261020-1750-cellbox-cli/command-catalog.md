# Phase 01 command compatibility and caller-right catalog — candidate v1

**State:** source-inspected preparation and proposed freeze for maintainer review, **not CP1/CP2, implementation approval, runtime proof or CBX-01 completion**. “Observed” below describes current code; “candidate” selects migration behavior. No current command is deleted by this catalog. Existing candidate engine/session contracts in [spec.md](spec.md) and [abi-session-launch.md](abi-session-launch.md) remain the shared design; this file owns the per-command dispositions and caller/right matrix rather than introducing a competing authority model. [command-contracts.md](command-contracts.md) is this catalog's normative candidate annex for exact new `rg`/`edit`/`ssh` argv/options/modes, bytes/session policy, bounds, schemas, status and required acceptance; no new-command behavior is delegated to an unspecified future flag freeze.

## 1. Coverage and compatibility rules

Completion inventory is exactly:

`alias awk bg blktest break cat cd clear continue date echo env exec exit export fg find free grep head help history hv ifconfig ip jobs kill ls mkdir mv ps pwd read rm rmdir sed shutdown sleep snapshot sort source tail tee test top touch unalias uniq unset uname uptime vappend vcat vwrite wc cp`

Source: `cells/tools/shell/src/executor.rs:48-56`. In addition, `[` dispatches `test` and `.` dispatches `source`; neither appears in completion. `ifconfig` and `ip` share behavior. `alias`, `unalias`, `export`, and REPL `history -c` are intercepted before AST execution (`shell.rs:96-164`), not ordinary executor commands. Every name has its own row below, including these semantic aliases. Standalone targets are the eleven sys-tools targets (`cells/tools/sys-tools/Cargo.toml:11-55`) and six net-tools targets (`cells/tools/net-tools/Cargo.toml:11-34`).

**As built (revision 2026-10-25).** The registry is now `cells/tools/cellbox/src/executor.rs:940-948` `BUILTINS`: 60 names, the 56 above plus `edit`, `ssh`, `completion` and `rg`. `[` and `.` still dispatch outside `BUILTINS` (`executor.rs:2196,2201`). The typed stages and `--records` modes are classified in `cells/tools/cellbox/src/typed.rs:178-209`, outside `BUILTINS`; see "Typed stages and record modes" below. `cells/tools/shell` no longer exists: Phase 04 S1 moved it (git mv) to `cells/tools/cellbox`. **Revision 2026-10-26 (Phase 10 S10-0):** `/bin/cellbox COMMAND` exists as a one-shot for the catalog commands, through the closed table of §3.2 (`cells/tools/cellbox/src/oneshot.rs`); the same table serves the in-shell explicit form `cellbox COMMAND` / `/bin/cellbox COMMAND`. Line numbers cited in this catalog are those of their revision date.

Candidate conventions applying to **every row**:

- Canonical suite invocation is `/bin/cellbox COMMAND`; shell short names resolve to the same registry in `cellbox sh`. Shell-state rows are registry metadata/parent-context operations, not external child invocations. No basename dispatch, forwarding binaries or obsolete executable-path aliases survive final cutover. Semantic aliases `.`/`[`/`ifconfig` remain genuine catalog entries.
- `S` = owning-shell state; `M` = shared cellbox module; `P` = explicitly preserved specialized command/module; `W` = independently packaged service/worker; `L` = approved external launch. “Preserve” means preserve the named useful operation/options, not reproduce false-success/error swallowing or accidental text corruption.
- Candidate status **N**: 0 success; 2 invalid/missing/extra/unsupported options or operands; 1 operational denial/I/O/provider failure; cancellation 130, timeout 124, session loss 125. Unknown command 127; denied/nonexecutable launch 126. Search/test exceptions are stated in their rows. These intentional status corrections replace legacy usage/failure-as-0, are not claimed current compatibility, and require migrated script acceptance before retirement. Other global engine rules are in spec.md §3.
- Candidate unknown flags and excess operands reject **before effects** unless the row defines a variadic operand list. Keep currently accepted documented flag combinations; do not infer POSIX options from a familiar name. Help/completion derive from this catalog, include all aliases, and distinguish specialized/stub commands.
- Pure observations may use immutable shell context snapshots in pipelines. Parent-state mutations, `exec`, control and session modes must obey spec.md's pre-effect pipeline/background checks. No hidden privilege is obtained from selecting a command.
- Rights codes are defined in §4. Each per-command rights cell lists the **complete command-specific requirement**, in addition to base own-operation IO/clock/log/syscall admission. `R?` means file reads only when file/pattern-file operands are used; stdin-only needs no VFS read. `—` means no backend authority beyond that base, not bypass of session/launch checks.
- Each row's scenario is a **required future acceptance fixture**, not an executed result. All source evidence here is static inspection; no suite executable, server, build, dependency probe, signing or network exposure was run for this catalog.

## 2. Exact shell name dispositions

Source keys: **E** `cells/tools/shell/src/executor.rs`; **C** `commands.rs`; **F** `cmd_fs.rs`; **Y** `cmd_sys.rs`; **H** `history.rs`; **D** `cmd_cwd.rs`; **REPL** `shell.rs`. All are under `cells/tools/shell/src/`. Status “0” in observed column means the implementation returns `Ok(())` even in the explicitly noted error cases; dispatch maps `Ok→0`, `Err→1` (E:598-647).

**Historical baseline.** The "observed" column of the 58 legacy rows below (the 56 completion names plus `[` and `.`) records the 2026-10-20 bytes of the deleted `cells/tools/shell/src/*`. Its line numbers are those bytes, not the current `cells/tools/cellbox/src/*`. Later phases changed some of these behaviors under their own CP2, for example `ps`/`top` (P06), `kill`/`snapshot`/`shutdown` (§3.1) and `cat`. The four rows `rg`, `edit`, `ssh` and `completion`, added 2026-10-25, cite the current tree.

| Name | Observed options, state/data and status | Candidate disposition / rights | Required acceptance scenario |
|---|---|---|---|
| `alias` | REPL:124-137 lists aliases or sets first whitespace-delimited `k=v`; strips outer quote characters; not AST builtin; returns success. | S; preserve list/set with quote-preserving parsed operand; N; — | Set alias containing spaces, invoke it, and reject pipeline mutation before any stage runs. |
| `awk` | E:1078-1127: `-F sep`/`-Fsep`, one UTF-8 input file or captured stdin; bounded program engine; non-program argument falls back to legacy `/literal/ col,... [file]` field extractor. New engine 0/error2; legacy errors may be 0. | M; retain **both** accepted syntaxes and separators, bounded UTF-8 text; N; R?; never rename to `rg`. | Quoted `{print $1}`, literal legacy filter/column form, separator and >1-file/error rejection. |
| `bg` | E:661-665 prints no-job-control message, 0; no resume effect. | S; preserve name, explicit unsupported status2 unless runtime already supports stopped-job resume; no new suspension facility; — | `bg JOB` does not pretend to resume or read foreground input. |
| `blktest` | Y:146-158 attempts raw sector0 read; prints `blkio: denied` or `ALLOWED (BUG)`; 0 in either case. | P; retain diagnostic name and explicit expected-denial probe; **never grant B**; return 0 for expected denial, 1 for unexpected allow. | Restricted invocation reports denied and causes no raw read; unexpected allow is failure, not success. |
| `break` | E:680-683 sets current loop signal, 0, even outside loop. | S; preserve in loops; outside-loop misuse2; — | Break current loop in parent context; pipeline/background rejected before state/effects. |
| `cat` | C:340-437 multiple paths/`-`/captured stdin; VFS first (1 MiB bound), BootFS fallback; VFS invalid UTF-8 may fall through; kernel path replaces malformed sequences; open/read diagnostics may still return0. | M; retain operands/stdin, **exact Bytes** for all paths, no UTF-8 dropping/replacement; N; R? | Multi-file + `-`, split UTF-8 and arbitrary octets survive redirect/pipe; absent file nonzero. |
| `cd` | D:31-68 exactly one operand, resolves cwd; VFS stat validation for subpaths then kernel chdir; errors1. | S; parent cwd only via explicit directory context; N; R+C | Relative `..`, missing/not-directory and denied directory leave cwd unchanged. |
| `clear` | C:29-32 sends ANSI directly to console, bypasses capture, 0. | M/session-control; explicit terminal only; N; T | Authorized terminal clears; redirected/nonterminal invocation errors without emitting ANSI. |
| `continue` | E:684-687 sets loop signal, 0. | S; current loop only, invalid outside loop2; — | Continue parent loop; pre-effect pipeline/background denial. |
| `date` | Y:268-305 no flags; UTC `Mon DD hh:mm:ss UTC YYYY`; epoch0 prints `date: no RTC — uptime N seconds`; 0. | M; use shell UTC text profile; native wall-time-or-unavailable + monotonic data, never fabricate wall time; N; O | RTC present/absent fixtures distinguish wall time from uptime; no claim date-setting support. |
| `echo` | C:546-576 leading `-e`, `-n`, `-en`/`-ne`; escapes `\n \t \\ \r`; 0. Redirect helper C:505-515 joins raw args + LF, **does not honor flags**. | M; choose shell flags, same exact output for console/pipe/redirect; N; — | `echo -en` matches all sinks; literal/empty/space args survive. |
| `env` | Y:91-95 prints static `PATH=/bin`, `SHELL=/bin/shell`, `OS=Cellos`; no Config read; 0. Standalone adds VERSION. | M read-only shell snapshot; include default OS/PATH/VERSION and canonical SHELL identity at cutover; **not** a claim exports currently appear; N; — | Snapshot survives child handoff without reading global Config or secrets; old static-output assertions migrate explicitly. |
| `exec` | C:34-129 **spawn-and-wait, not process replacement**; kernel-FS path then `/` prefix, argv stash512, ELF grant launch; most failures print and return0. | S/L; preserve spawn-and-wait and literal argv; no implied POSIX replacement; N; R+E | Exact reviewed Elf edge succeeds; forged privileged advisory path/over-limit argv denied before child publication. |
| `exit` | E:692-698 requests shell exit using first parsed i32/default0; command dispatch returns0. | S; terminate owning shell with requested code; no background/pipeline child-state trick; — | `exit 7` closes only that session and restores terminal guards. |
| `export` | REPL:145-151 first `k=v` writes **shared ConfigClient.set**, ignores result; does not export local shell variable/environment. | S+P; retain explicit legacy Config-set effect only under Q grant; N; no implicit env mirroring/new global privilege. | Granted key update visible through Config; denied key yields nonzero and no mutation; `env` does not fabricate it. |
| `fg` | E:661-665 no-job-control message, 0. | S; only transfer a resumable existing job's granted session; otherwise unsupported2, no fabricated control; T | Unsupported job leaves current reader unchanged; valid lease transfers/restores once. |
| `find` | F:825-885 first dir/default`.` then optional `-name pattern`; **substring**, not glob; files only, recursion depth16; bounded ListDir may truncate silently; 0 even traversal failure. | M; retain substring/file-only syntax and depth16; N; R; incomplete traversal is explicit error/metadata, not complete success. | Substring versus wildcard fixture, depth/cap/provider denial reported without silent omissions. |
| `free` | Y:58-87 validates physical-frame snapshot, frame×page_size/1024 KiB, errors1; not Cell/process heap. | M; retain truthful physical-frame KiB and unavailable/error; N; M | Overflow/invalid snapshot denied; never substitute heap/free estimate. |
| `grep` | text_tools/grep.rs:15-43; flags `-E -F -i -v -n -c -q -x -r -e -f --`, combinable short flags, attached `-ePAT`/`-fPATH`; ERE-lite/fixed, multiple files; status0 selected,1 no selection,2 error. | M; retain bounded engine/flags and 0/1/2, no fake `rg`; R? | Every flag plus pattern-file reads, no-match, malformed regex, denied traversal/UTF-8/limit yields true status. |
| `head` | F:112-144 `-n N` default10, bad N silently10; one kernel-FS file, no stdin fallback; UTF-8 lines, reconstructed LF; missing path0, read error1. | M; retain single-file line mode/default10, invalid N2; no invented `-c`/follow; N; R | N=0/default/invalid count and non-UTF-8 denial; no accidental byte corruption. |
| `help` | C:6-26 static/incomplete family list, 0. | M; generated exhaustive registry help (shell-only/service/stub labels); N; — | Every catalog name/alias discoverable; no unsupported flags advertised. |
| `history` | H:98-120 `[-c] [n]`, last n with original indices; REPL `-c` clears two history stores; ordinary execution has global history clear too. | S; session-owned list/clear; N; — | Clear affects current history only; secret input absent; numeric limit stable. |
| `hv` | E:1001-1038 no arg/`start` starts `/bin/hypervisor` without waiting; help/-h/--help0, other subcommand1; Path first then VFS path fallback; spawn failure1. | P/L; retain exact start/help, independent hypervisor identity/ceiling, no inline worker; N; E+H | Path-only reviewed capability-bearing launch; Elf substitute rejected; no extra device/control rights to common artifact. |
| `ifconfig` | Y:180-213 ignores args; service-net GetLocalIp, DHCP waiting/address text; missing service/reply still0. | M semantic alias of `ip`; query only, no interface mutation; N; I | Same native/text result as ip; unavailable service nonzero, never treat as address0 success. |
| `ip` | Same dispatcher as `ifconfig`; no `addr`/`route`/set grammar implemented. | M; retain query-only no-arg surface; N; I | Unknown Linux-style subcommand rejects before any network mutation. |
| `jobs` | E:657-659,745-758 lists stored ID/Running/Done/name;0; builtins/background bookkeeping is not real suspend/resume. | S read-only job snapshot; N; — | Whole-pipeline identity/member completion from spec, not one job per stage; unrelated sessions hidden. |
| `kill` | C:597-663 first positive decimal TID, 16-row GetProcs lookup; Waiting→IPC `0xff`, Dead message, other live state→ForceExit; errors/denials mostly0. | P; explicit lifecycle client to supervisor KillTarget, no ambient ForceExit or arbitrary cooperative `0xff`; N; K | Missing/wrong/stale target grant denied with no IPC/kill; authorized exact lifetime only, no TID reuse. |
| `ls` | C:159-337 `-a/-A` both include `.`/`..`, `-l`, `-F`, first path/default cwd; ignores unknown flags/extra paths; merged root VFS+BootFS case-insensitive dedup, lexical sort; many failures0. | M; choose shell flags/cwd/merged namespace; `-A` retains current dots equivalence (not POSIX); N; R | All flags, root dedup, cwd, empty directory versus denied/error distinguish; standalone consumers migrate format. |
| `mkdir` | F:237-287 combined `-p -v`, multi-path; unknown flags ignored; parent walk; usage/backend failure0. | M; preserve flags/path list, N; R+W | Parent-existing and parent-create paths; denial reports failed operand without success claim. |
| `mv` | F:517-594 multiple sources+destination, ignores flags; tries Rename only `/srv/`→`/srv/`, otherwise copy+delete (directories too); **not generally atomic**; errors0. | M; retain operand behavior and fallback with explicit non-atomic/partial-outcome diagnostics; N; R+W | Multi-source dir, cross-backend copy failure never deletes source or claims atomic move. |
| `ps` | C:450-499 `-p TID`, `-s` state order,64 rows; malformed filter silently none, read failure0. | M; retain filters/task drill-down under explicit telemetry scope; N; X. **Phase 06 S6-2 (implemented; CP2 approved 2026-10-24):** `ps [-p TID] [-s]` (`commands.rs` `cmd_ps`) is one `TelemetrySnapshot` capture rendered by the shared `top` task-view renderer (D62-2). The 64-row cap is gone (74 tasks listed at runtime, `top-load-over-64-text`); `-s` sorts by the `State` field (the old `state_order` table and 64-row `GetProcs` buffer are deleted); bad options → 2; denied, failed or incomplete telemetry → 1 with no table. `ps --records` is one capture of `top.task.v1` rows keyed by `tid` (the `pid` field is gone; D62-1). | Filter/state order, denied cross-Cell query, truncated sample explicit; no control right from rows. |
| `pwd` | Y reexports D:24-27 kernel cwd, errors1. | S read-only immutable cwd snapshot usable as stage; N; C | Parent/child explicit cwd handoff and unavailable cwd; no ambient directory access implied by printed path. |
| `read` | E:840-846 first variable/default REPLY, focus-aware line input, trims CR/LF, mutates shell variable; errors1. | S; foreground reader only, no pipeline/background mutation; N; T | Default/name assignment, EOF/cancel, wrong reader/session denied without consuming key. |
| `rm` | F:365-438 combined `-r/-R -f -i`, last f/i wins; multiple paths; interactive pipe-line or terminal yes/y; recursive only `/data/`; errors0. | M; retain options and current recursive scope; N; R+W (+T for terminal confirmation) | Last-option order, denied/protected tree, no-key/noninteractive confirmation fail closed; -f suppresses absent operand only, not authority failure. |
| `rmdir` | F:292-360 combined `-p -v`, multiple paths, empty-dir/parent traversal; errors0. | M; preserve flags, parent stopping behavior, N; R+W | Nonempty/protected parent stops removal and yields failure; verbose output only committed removals. |
| `sed` | E:1045-1075 `-n`, one script and at most one file/stdin; UTF-8; engine0/error2. Single substitution with g/p, numeric Np, `/pattern/p` or `/pattern/d`; no multi-command/hold/branch/read/write. | M; keep exact bounded supported grammar, no in-place editing; N; R? | Substitution/default versus -n/print/delete; unsupported script/multiple input files produce2 and no writes. |
| `shutdown` | Y:112-115 direct syscall502; intended nonreturn. Kernel x86 branch only hlt-loops; **no current capability gate**, see §4. | P; supervisor ResetSystem only; poweroff request, no fabricated reboot; N/provider Unsupported; Z | Common restricted caller denied before syscall; approved platform without real poweroff returns unsupported, not success. |
| `sleep` | Y:121-143 first integer seconds, 10 MHz tick deadline, saturating arithmetic, yields; invalid/missing usage0. | M; integer seconds/own-operation delay only; N; O | Zero/default misuse, cancellation and overflow rejected rather than unbounded accidental wait. |
| `snapshot` | snapshot_client.rs:20-60 no options, service-supervisor request with bounded timeout,0 success/1 unavailable/protocol/timeout; global whole-RAM snapshot, not per-file save. | P; preserve global snapshot operation only under **system-scoped** SnapshotTarget grant; N; S | Restricted caller denied, supervisor absent/timeout nonzero, no per-Cell snapshot claim from global provider. |
| `sort` | F:961-985 optional one file/captured stdin; UTF-8 lexical line sort, LF reconstruction; invalid UTF-8 silently empty; empty stdin usage0. | M text line sort (distinct from typed sort-by), N; R? | Empty input success/no usage, UTF-8/limit errors, duplicate lines preserved. |
| `source` | E:853-880 one file, at most4096 bytes, parent jobs/context, skips blanks/# lines, executes each line; missing path error. | S; retain bounded parent-context line execution, file read required; N; R | Bound/truncation rejected rather than execute a truncated final command; variables/cwd remain in parent. |
| `tail` | F:149-182 `-n N` default10/bad N→10; one kernel-FS file, no stdin/follow; UTF-8 lines + reconstructed LF; usage0/read failure1. | M; retain single-file line mode; invalid N2, no new `-f`; N; R | Last N, N=0, invalid input/UTF-8 fail visibly. |
| `tee` | F:994-1022 leading `-a`, one path, captured stdin; empty input does not create file; UTF-8-only stdout but byte VFS write in400-byte chunks; failure0. | M; keep append/replace, byte-identical stdout/file; empty input retains no-write behavior; N; W | Binary data equals stdout/file; denied mutation is nonzero; append/partial error not replayed. |
| `test` | E:768-833 `-f/-d/-e`, prefix/suffix `-z/-n`, string `=/==/!=`; true0/false or unsupported1. | S/pure conditional; preserve grammar, status0 true/1 false/2 malformed; R? for path predicates | String forms need no VFS; denied path query distinguishes operational failure from false. |
| `top` | top.rs:16-75 + text-engine/top.rs:19-86: `-a -b -n COUNT -d SECS -o cpu\|mem\|heap\|pid\|state\|name`;64 task rows, GetProcs2; one seed then sample, batch default1; q/Q quits interactive; bad options0, sampling failure1; task-TID matching/100% clamp. | M; retain task view/options as drill-down; Cell-first view/schema is new and blocked on provider, not relabelled task rows; N; X (+T interactive); M only for a separately requested global physical-memory snapshot. **Phase 06 S6-2 (implemented; CP2 approved 2026-10-24):** the provider exists (`TelemetrySnapshot`, S6-1); `top` shows Cell incarnations by default (`--tasks`, `--cell ID:GEN`, `--fields`, `-o FIELD`), records `top.cell.v1`/`top.task.v1` as Frames, no 64-row cap, no 100% clamp, real decimal `-d` seconds; bad options → 2, denied/failed/incomplete telemetry → 1. **Phase 06 S6-4 (implemented):** without `-b`, a vitui/Ratatui view runs only as the sole foreground command with a controller session and capture depth ≤ 1 (else status 2 naming `-b`, no capture); interactive `-d` < 0.1 s → 2; keys Up/Down, PageUp/PageDown, Enter drill into `ID:GEN`, Esc/Backspace back, `o` sort cycle over shown columns, `r` reverse, `f` field picker, `q`/EOF → 0, Ctrl-C → 130 + `^C`; no lifecycle key; telemetry failure → 1 (at the seed before any screen byte, later after restore); frontend detach → 1; view capped at 120×40. | Batch reads no keys/ANSI; denied cross-Cell scope; incarnation reset and honest CPU units before new Cell view qualifies. |
| `touch` | F:443-460 multiple nonflag paths; existing path is **no-op, no timestamp update**, absent writes empty; ignores flags/errors0. | M; preserve create-if-missing/no-op existing, do not claim mtime semantics; N; R+W | Existing bytes unchanged; absent file create; denial reported. |
| `unalias` | REPL:139-143 first name, no-op missing, success. | S; remove session alias, N; — | Alias removed only in owning context; pipeline/background rejects before effects. |
| `uniq` | F:926-953 first file or captured stdin; adjacent UTF-8 line dedup, LF reconstruction; empty stdin prints usage0, invalid UTF-8 silently empty; initial empty lines suppressed because previous line starts empty; open error1. | M; adjacent line dedup with first empty line retained, empty input succeeds without usage, malformed UTF-8 errors rather than silent data loss; N; R? | Adjacent versus nonadjacent duplicates, initial blank lines, empty input, invalid UTF-8 and denied file; no global dedup or count flags inferred. |
| `uname` | Y:9-22 any `-a` selects full info; default OS name, other args ignored;0. | M; choose shell default and -a, static metadata; N; — | Default/full output and standalone full-output caller migration explicit. |
| `unset` | E:700-704 removes every named shell variable,0; not Config delete. | S; preserve variadic local removal, N; — | Does not delete shared Config or mutate other shell. |
| `uptime` | Y:101-105 no options, monotonic ticks/10,000,000 seconds;0. | M; monotonic duration with unit/provenance, not wall time; N; O | Correct unit conversion and nonnegative source; timer provider errors explicit. |
| `vappend` | F:725-740 path+rest joined spaces, single typed Append request, errors0. | P; preserve named VFS diagnostic with same text join/no added LF; bounded frame rejection instead of truncation; N; W | Append exact text, denied/oversized request nonzero; no device rights. |
| `vcat` | F:800-817 first path, VFS read buffer4096, invalid UTF-8 no output; read failure1. | P; preserve VFS-only diagnostic, byte output, explicit4096-byte bound/incomplete error; N; R | VFS versus BootFS routing, arbitrary bytes and oversized file visible rejection. |
| `vwrite` | F:707-722 path+rest joined spaces, typed Write, no LF; errors0. | P; preserve VFS diagnostic name/text join, reject frame overflow before mutation; N; W | Exact empty/spaced text and denied write; no false success. |
| `wc` | F:79-107 optional first file or captured stdin; counts LF, bytes, words separated only space/LF/tab; ignores later args; empty stdin usage0, open failure1. | M; retain count definitions/single optional file, byte-count producer; N; R? | Empty input all zero, CR is not invented whitespace, bytes including invalid UTF-8 counted exactly. |
| `cp` | F:599-665 combined `-r/-R`, multiple sources+destination; directory recursion, destination directory heuristic;1 MiB per-file VFS read then kernel fallback; errors0. | M; preserve recursive/multi-source operations, no atomic/durability promise; N; R+W | File/dir/multi-source and denied destination; oversized/partial copy never claims complete success. |
| `[` | E:672-679 removes **all** `]` arguments then uses test; closing `]` not required. | S semantic test alias; candidate requires one final closing `]`, preserves test operators/status; R? | Missing/trailing/middle bracket malformed2; normal predicate agrees with test. |
| `.` | E:667 aliases source, absent in completion. | S exact source alias, same bound/state/status/rights; R | Identical parent-context execution and denial behavior to source; discoverable in help. |
| `rg` | **As built (P07).** `libs/vigrep` adapts ripgrep 15.2.0 `e89fff89` (`libs/vigrep/PROVENANCE`). `rg` is always a typed stage, and its whole argv is parsed before any effect (`typed.rs:190-195`). `rg [OPTIONS] PATTERN [PATH…]` is a row producer of `cellbox.search.line` rows that walks the VFS (`rg/tree.rs`). `rg -a PATTERN -` searches the bytes of the head before it (`Class::SearchStdin`). A usage error becomes `Class::Bad` before effects. The arena is 6 MiB (D76-3). Legacy `grep` is a separate row and unchanged. | M typed producer; ledger `InSuite` (§3.1); argv/options/schema/status as in [command-contracts.md](command-contracts.md) §2.1–§2.3, with P07 decisions D76-1..19 and D77-1..7; R for paths/traversal, none for explicit byte stdin; no new syscall or right. | command-contracts §2.4. Exercised on private TCG images: terminal-lane `rg-*` plus the `rg-fx-*` fixture checks (`rg-fx-big-exact`, `rg-fx-denied-root`, `rg-fx-denied-child`, …), P03 `rg-*`, and P02 `rg` Ctrl-C → 130 ([phase-07](phase-07-ripgrep-search.md) deviation log). Latest terminal lane: 459/459 on 2026-10-25 (`/tmp/cellbox-phase04-z1mR/evidence/verdict.json`). Link no-follow/cycle has host-fake evidence only (user decision 2026-10-24). |
| `edit` | **As built (P08).** `edit [PATH]` lives in `cells/tools/cellbox/src/edit.rs` and `edit/{editor,render,store}.rs`, with the buffer in `libs/text-engine/src/editor.rs`. Valid argv is a Session stage. Invalid argv runs as a builtin that prints usage and exits 2 (`typed.rs:137-140`). Save uses the D03 conditional save (VFS `Observe` 33, `SaveBegin` 34 … `SaveAbort` 39) on CellosFS `/srv` only. FAT32/bootfs files open, but saving them is refused (D78-1, a deviation from contract §3.1). It follows Kibi ideas only; no Kibi code is copied (`editor.rs:19`). | M foreground session; ledger `InSuite`; key map/highlight/save as in command-contracts §3.1–§3.3, with P08 D78-1..3; R+W+T, with W only through the D03 save. | command-contracts §3.4. Exercised: ten terminal-lane `edit-*` checks (incl. `edit-refusals`, `edit-fat-save-refused`, `edit-saveas-overwrite-race`, `edit-find-select-redo`) and GUI EDIT-0..5 (GUI lane 290/290, `/tmp/cellbox-phase04-gui-uyuX/evidence/verdict.json`). Fault paths have VFS self-test/host evidence only, by approved deviation ([phase-08](phase-08-compact-editor.md)). |
| `ssh` | **As built (P09).** The builtin validates argv with `vissh::args`: valid argv is a Session stage, and invalid argv prints usage and exits 2 (`typed.rs:141-144`). It then re-launches the same ELF from its second install path, `/bin/cellbox-ssh`, with argv `["ssh", …]` (`ssh.rs:3-6,22-24`), and the child takes the ssh entry (`main.rs:132-136`). The protocol core is Sunset 0.6.0, vendored in `third_party/sunset` (patches 0001–0006) and used through `libs/vissh`. `--auth password` only: `--auth publickey` and pipeline exec are refused (D79-14). Entropy comes from the kernel only (`GetRandom`, `ssh.rs:299-302`). Each outcome is written as a `cellbox.ssh.completion` v1 record through child-context slot 5 `Completion` (`kernel/src/task/child_context.rs:8-9`, D79-15). | L launcher, like `hv`: ledger `InSuite`. The shell holds no network right. Only the reviewed `cellbox → /bin/cellbox-ssh` Path edge carries `NET_CLIENT` (resolve + connect; `profiles.rs:70-75`, `targets.rs:10-14`, D79-9 revised); its Elf route is denied (`tests.rs:155`). Rights: N, plus D for host names, on that edge; Vt through the D03 VFS trust worker (requests 40–42); T for PTY, password and first-trust confirmation; O. A (KMS credential) is deferred together with publickey. | command-contracts §4.4, excluding publickey/KMS and the Tier-3 guest. Exercised: Phase 09 lane 103/103 against the disposable OpenSSH fixture (D79-1), including host-key mismatch, unknown-key confirmation, wrong password and no secret on serial. Criteria 1 and 2 are partial ([phase-09](phase-09-ssh-client.md)). |
| `completion` | **As built (P09 S9-6).** Takes no operands. Prints the newest `cellbox.ssh.completion` record as one line and returns 0. Any operand prints `usage: completion` and returns 2. With no record it prints `completion: no ssh completion recorded` and returns 1 (`executor.rs:3445-3457`). It is not part of the 2026-10-20 completion inventory. | S read-only session snapshot; ledger `InSuite`; N; — | Phase 09 S9-6 / D79-15(a) ([phase-09-ssh-client.md](phase-09-ssh-client.md):107): the record after each ssh outcome, plus the usage and no-record statuses. |

Text bounds retained: `libs/text-engine/src/records.rs:3-6` fixes 16 file operands, 65,536 aggregate input bytes, 4,096-byte records and 4,096 records; grep additionally 32 patterns/8 pattern files and recursion16 (`grep/parse.rs:7-8`, shell text_tools/grep/io.rs:11,79-93). New awk programs max512 bytes, one top-level print action, at most16 print expressions; preserve existing parser expression/filter implementation rather than invent POSIX awk (`libs/text-engine/src/awk.rs:11,50-79`, `awk/parser.rs:18-108`). Sed exact command rejection is in `libs/text-engine/src/sed/parser.rs:36-108`. These are text-engine bounds, **not** claims all legacy file helpers are already bounded. Exact Bytes commands must stream under spec.md budgets; capacity/encoding errors must never be converted to empty successful output.

### Shell execution and script baseline beyond command names

- Grammar includes simple commands, pipelines, redirects, background, sequences, `&&`, `||`, `if`, `while`, `for`, `case`, functions (`parser.rs:1-150,430-690`). Preserve supported quoting/expansion/control forms; no full Nushell grammar.
- Intermediate stages are currently sequentially captured `Vec<u8>` builtin sinks; no real external/incremental typed pipe (`E:354-388`). Candidate spec.md replaces this transport only after the session/pipe owner gate; no current real external-pipe claim.
- Current `cmd &` builtin runs synchronously and is marked Done; external skips wait (`E:248-270`); candidate rejects unsupported native/mixed/stateful/background forms before effects and retains only admitted external byte jobs, not fake job control.
- Functions save/restore up to9 positional variables plus `#/@`, execute at most479 stored body bytes through a480-byte buffer (`E:707-737`). Candidate retains bounded grammar/argument behavior but rejects over-limit body before definition/execution rather than truncate into effects.
- External launch uses512-byte structured argv excluding program name; `/bin/name`, raw path then VFS-loaded fallback; foreground releases/waits/reacquires focus; unknown name127 (`E:884-990`). Candidate exact argv/cwd/environment publication and wait/reap/status rules are owned by spec.md/D01/D02, not a second transport here.
- REPL special dispatch currently tokenizes whitespace before parsing, so aliases/export/history behavior differs from sourced AST commands. Candidate integrates parent-state dispatch into the same parsed registry and rejects unsupported contexts before effects; this intentional correction must be in script acceptance, not silently claimed legacy equivalence.

### Typed stages and record modes (revision 2026-10-25)

These names are typed-vocabulary stages, not `BUILTINS` entries, so the §3.1 ledger cannot detect their loss. They are classified before any effect in `cells/tools/cellbox/src/typed.rs:178-209`, and `might_be_typed` (`typed.rs:205-209`) routes them to admission.

| Stage / mode | As-built classification | Disposition / rights | Required acceptance |
|---|---|---|---|
| `select`, `where`, `sort-by`, `take` | `Class::Row` after `pipeline::parse_stage`; argument errors become `Class::Bad` before effects (`typed.rs:181-188`) | M pure typed transform; no backend right of its own; rights are those of the producer head | [spec.md](spec.md) §2; §5 AC-1 (pre-effect rejection), AC-2 (native rows) and AC-3 (JSON(L)); P03 lane checks ([phase-03](phase-03-typed-pipelines.md), CP2 2026-10-21) |
| `to`, `from` | `Class::To` / `Class::From` (`typed.rs:183-184`); explicit serialization only, with no implicit sniffing | M pure conversion; — (a redirect target separately needs W) | spec §2, AC-1 and AC-3 |
| `ps --records`, `ls --records`, `top --records` | `Class::Producer` (`typed.rs:189`), head only | Same rights as the command's own row (X for `ps`/`top`, R for `ls`) | spec §2; P06 `top.cell.v1`/`top.task.v1` and `ps --records` rows (§2 `ps`/`top`) |
| `cat PATH` as a head | `Class::Cat` exact-byte producer (`typed.rs:196-198`) | R, as the `cat` row | spec §2; P03 exact-bytes checks |
| `rg` | `Class::Search` / `Class::SearchStdin` (`typed.rs:190-195`) | See the §2 `rg` row | command-contracts §2.4 |

A ledger surface for typed stages, so that a host test fails if one disappears, would be a source change. It is not proposed here.

## 3. Standalone target dispositions (no unlisted family removal)

Paths are `/bin/NAME`; source is `cells/tools/{sys-tools,net-tools}/src/bin/NAME.rs` unless stated. All eleven sys-tools and six net-tools targets are represented, including unusable/stub paths. “Admitted” below means a reviewed launch-profile row exists, **not** packaged/running/signed. Sys-tools names `date/env/kill/uname` and net-tools `ping` are **absent** from reviewed_user_target_ceiling; no arbitrary `/bin/*` wildcard admits them (`targets.rs:7-109`). `/bin/shutdown` has exactly one reviewed edge since S2c-min: caller `cellbox`, route `Path`, ceiling `LEGACY` (`profiles.rs:66-69`); raw syscall 502 is accepted only from a task named `shutdown` (`RAW_SHUTDOWN_CALLERS`, `kernel/src/task/syscall.rs:1858`). Do not describe shell builtin success as proof these binaries can launch.

**Ceiling wording (revision 2026-10-25).** The rows below say "empty ceiling" as observed on 2026-10-20. Since S2-T/W (D-TW1), every reviewed non-sandbox edge called "capability-free" carries `LEGACY` = `CapSet::LEGACY_OBSERVE_MUTATE` (X/M/W only; `profiles.rs:12-14`, `kernel/src/task/cap.rs:290`). Since S2-N, `curl`/`wget`/`mqtt` carry `NET_CLIENT`, `nc` carries `NET_PEER` and `httpd` carries `NET_LISTENER` (`targets.rs:78-81`).

| Target | Observed standalone semantics/status and launch | Candidate disposition / required migration fixture |
|---|---|---|
| `ls` | first path/default `/`, kernel directory only, no flags,1 failure/0 success (`ls.rs:11-34`); admitted empty ceiling. | M shared ls; explicit callers get shell superset with immutable cwd/root context; compare old no-flag listing and migrate formatting/root merge expectations. Remove duplicate target only at D01 CP2. |
| `cat` | first required file,512-byte chunks; drops entire non-UTF-8 chunks, read errors end loop but0; missing/open failure1 (`cat.rs:11-43`); admitted empty ceiling. | M exact Bytes cat; assert arbitrary octets and split codepoint survive; documented correction, no legacy lossy implementation retained. |
| `echo` | prints all args literally separated spaces +LF, no flags,0 (`echo.rs:11-20`); admitted empty ceiling. | M shell echo semantics; migrate any caller whose leading `-n/-e` meant literal text; use documented literal argument boundary rather than keep second engine. |
| `ps` |32 rows, no flag parser, failed GetProcs prints diagnostic but0 (`ps.rs:10-36`); admitted empty ceiling. | M shared task observation, scoped X; missing rights fail, old32-row truncation not complete data; retain -p/-s superset. |
| `kill` |32-row lookup; Waiting sends0xff; Ready/Running **ignores ForceExit result but prints success**; missing/parse/not-found1, Dead0 (`kill.rs:17-69`); no reviewed target. | P shared supervisor client K; no privileged standalone artifact or false-success fallback; denied exact target fixture mandatory. |
| `env` | static PATH/SHELL/OS/VERSION,0 (`env.rs:8-14`); no reviewed target. | M snapshot env; migrate static SHELL path, no claim old Config/environment query existed. |
| `uname` | always full OS/kernel/version/arch, ignores args,0 (`uname.rs:12-21`); no reviewed target. | M shared default/`-a`; callers needing old full text pass `-a`; no hidden standalone default mode. |
| `date` | UTC `YYYY-MM-DD hh:mm:ss UTC`; no RTC `Uptime: Hh Mm Ss (no RTC)`;0 (`date.rs:77-108`); no reviewed target. | M shell human text/native truthful time; migrate precise format consumers to schema, not text parsing. |
| `free` | same validated physical-frame KiB semantics,0/1 (`free.rs:10-44`), explicit Log/MemInfo allowlist; admitted empty ceiling. | M shared physical-memory producer M; preserve meaning/denials, no heap approximation. |
| `shutdown` | syscall502, no explicit exit status (`shutdown.rs:16-19`); S2c-min: reviewed `cellbox` Path edge, `LEGACY` ceiling; the kernel accepts raw 502 only from the task name `shutdown` and denies every other caller (`raw opcode 502 denied for <name>`). Source comment x86 ACPI/debug-exit is not evidence: kernel branch hlt-loops. | P shared authorized Z client; the name-keyed gate is a legacy policy label, not a generation-bound ResetSystem grant; no poweroff qualification without provider. |
| `hotswap` | exactly2 args; service ASCII alnum/hyphen≤63, exact ASCII `/bin/<name>`≤127/no nested path/`..`; supervisor reply timeout1600 ticks;0 success/1 all errors (`hotswap.rs:32-107,138-167`); shell-only special empty-ceiling launch. | P shared ReplaceService Hs client; preserve service/path limits and bounded status; keep independent service-supervisor. Legacy CLI remains until exact authorized consumers migrate; desktop/tool-spawn continue denied. |
| `ping` | always “ICMP socket data path not yet wired”, explicit1 (`ping.rs:10-13`); no reviewed target. | P **explicit unsupported diagnostic**, never claim ICMP implementation or remove silently; candidate `cellbox ping` returns1 and no network/device effects. Existing missing standalone launch remains recorded, not widened for a stub. |
| `curl` | first URL, HTTP only; DNS/TCP service; GET request≤500 bytes;4 KiB response includes headers, status line+UTF-8 body printed; cap silently truncates; errors return from unit cell_main (`curl.rs:29-224`); admitted empty ceiling/no VfsMutate. | M specialized HTTP client N+D; retain HTTP-only/status-line profile, no TLS/redirect/auth/options inferred; overflow/timeout non-success, no VFS write. Test4 KiB boundary/malformed UTF-8 explicit rejection, no silent truncation. |
| `nc` | `host port` sends **fixed `HELLO_ViCell\n`**, receives one≤256-byte UTF-8 response; `-l port` accepts repeatedly, serial-print/echo loop (`nc.rs:20-241`); returns from unit cell_main on errors; admitted empty ceiling. | P short-client N+D with existing fixed payload, **not general netcat**; W listener Nl with independent lifetime; keep `/bin/nc` worker packaging for listener consumers. Client/listener have distinct rights; deny -l absent Nl before listen. |
| `wget` | HTTP URL + path,4 KiB total response; **writes only min(body length,440−path byte length)** in one VFS Write, may silently save truncated body; prints saved length; errors unit-return (`wget.rs:42-270`); admitted empty ceiling, VfsMutate declared. | M HTTP client N+D+W; retain download-to-path operation and limits, reject oversized/incomplete body **before VFS write**, never promote truncation to successful download. No safe editor-save/atomic/durability claim. |
| `httpd` | port+VFS path; endless sequential HTTP/1.0 server, reads VFS fresh each request,4 KiB file cap, no VfsMutate; errors unit-return (`httpd.rs:265-382`); admitted empty ceiling. | W independent `/bin/httpd`, R+Nl; **not a cellbox applet/privilege union**. Preserve dynamic-file client fixture and missing-file/error response; no unattended exposure in preparation. |
| `mqtt` | `publish host[:port] topic [payload]` or `subscribe host[:port] topic`; port fallback1883, topic needed; TCP/CONNACK occurs before unknown-subcommand rejection; QoS0/no TLS/auth; publish topic≤64/payload≤256, subscribe bounded polling10,000; unit-return errors (`mqtt.rs:23-123,258-361`). | P independent `/bin/mqtt` specialized protocol client N+D (subscription owns separate long-lived operation); preserve protocol/name, validate command/topic/port **before network effects**, apply64-byte topic bound to both modes to avoid fixed96-byte subscribe packet overflow. No removal for lack of generic suite role. |

Network unit-return errors **exit0 on normal return through the current ostd startup**, unlike ping/sys-tools' explicit `sys_exit(1)`: `libs/ostd/src/entry.rs:40-52` calls the unit entry; `startup.rs:51-54,78-81,100-103,115-121` exits0 after main on RISC-V/AArch64/x86 and generic entry. This is source evidence, not an executed target/wait result. Candidate N intentionally makes these failures nonzero. Preserve specialized worker targets until named callers/image entries are dispositioned; clean cutover removes only migrated suite duplicates, never unrelated services.

### 3.1 S2-lite disposition ledger (Phase 04, user-approved)

Source of truth: `cells/tools/cellbox/src/ledger.rs` (compiled in the production build, host-tested). Host tests require exactly one entry per name per surface, equal to `executor::BUILTINS` (60 since Phase 09: the 56 legacy names plus `edit`, `ssh`, `completion` and `rg`), the `[[bin]]` lists of `sys-tools` (11) and `net-tools` (6), and this table byte-equal to the ledger rendering. The non-default `applets` multicall module (`cellbox <applet>`, unreachable under the shell ceiling) was deleted in Phase 04 S4a. The suite's entries are `cellbox sh` and, since Phase 10 S10-0, the one-shot `/bin/cellbox COMMAND` of §3.2, which runs inside the same ELF and ceiling through one closed table. A command name is identified by (surface, name): the `cat` builtin and the `/bin/cat` ELF are distinct artifacts, so a name may legitimately appear once per surface.

- **InSuite** — runs inside the cellbox ceiling (every syscall it needs is already in the shell ceiling; VFS enforces caller/path policy; telemetry rows come from the already-held `GetProcs*`/`MemInfo`).
- **InSuiteEffectDenied** — the builtin name stays in the shell and its privileged effect was **observed to be denied** by the kernel or launch edge for this caller on the private image; each such entry names the runner check that proves it. **Deviation from the proposal's two-class text** (`phase-04-s2-authority-proposal.md` §2.1): the proposal lists blktest/kill/shutdown/snapshot/hypervisor start among retained workers although their builtin implementations already run in the shell; the shell-builtin classes record that fact without adding authority.
- **InSuiteRefused** — the builtin performs **no effect**: it prints a refusal, issues no syscall and no IPC and exits non-zero (S2c-min); each such entry names the runner check that proves the refusal and an unchanged process set. No `InSuiteEffectUngated` entry remains.
- **RetainedWorker** — a separate ELF with its own launch edge and ceiling, never in the suite ELF.

<!-- ledger:begin -->
| Surface | Name | Disposition |
|---|---|---|
| builtin | `alias` | InSuite |
| builtin | `awk` | InSuite |
| builtin | `bg` | InSuite |
| builtin | `blktest` | InSuiteEffectDenied |
| builtin | `break` | InSuite |
| builtin | `cat` | InSuite |
| builtin | `cd` | InSuite |
| builtin | `clear` | InSuite |
| builtin | `continue` | InSuite |
| builtin | `cp` | InSuite |
| builtin | `date` | InSuite |
| builtin | `edit` | InSuite |
| builtin | `ssh` | InSuite |
| builtin | `completion` | InSuite |
| builtin | `echo` | InSuite |
| builtin | `env` | InSuite |
| builtin | `exec` | InSuite |
| builtin | `exit` | InSuite |
| builtin | `export` | InSuite |
| builtin | `fg` | InSuite |
| builtin | `find` | InSuite |
| builtin | `free` | InSuite |
| builtin | `grep` | InSuite |
| builtin | `head` | InSuite |
| builtin | `help` | InSuite |
| builtin | `history` | InSuite |
| builtin | `hv` | InSuite |
| builtin | `ifconfig` | InSuite |
| builtin | `ip` | InSuite |
| builtin | `jobs` | InSuite |
| builtin | `kill` | InSuiteRefused |
| builtin | `ls` | InSuite |
| builtin | `mkdir` | InSuite |
| builtin | `mv` | InSuite |
| builtin | `ps` | InSuite |
| builtin | `pwd` | InSuite |
| builtin | `read` | InSuite |
| builtin | `rg` | InSuite |
| builtin | `rm` | InSuite |
| builtin | `rmdir` | InSuite |
| builtin | `sed` | InSuite |
| builtin | `shutdown` | InSuiteEffectDenied |
| builtin | `sleep` | InSuite |
| builtin | `snapshot` | InSuiteRefused |
| builtin | `sort` | InSuite |
| builtin | `source` | InSuite |
| builtin | `tail` | InSuite |
| builtin | `tee` | InSuite |
| builtin | `test` | InSuite |
| builtin | `top` | InSuite |
| builtin | `touch` | InSuite |
| builtin | `unalias` | InSuite |
| builtin | `uniq` | InSuite |
| builtin | `unset` | InSuite |
| builtin | `uname` | InSuite |
| builtin | `uptime` | InSuite |
| builtin | `vappend` | InSuite |
| builtin | `vcat` | InSuite |
| builtin | `vwrite` | InSuite |
| builtin | `wc` | InSuite |
| sys-tool | `ls` | RetainedWorker |
| sys-tool | `cat` | RetainedWorker |
| sys-tool | `echo` | RetainedWorker |
| sys-tool | `ps` | RetainedWorker |
| sys-tool | `kill` | RetainedWorker |
| sys-tool | `env` | RetainedWorker |
| sys-tool | `uname` | RetainedWorker |
| sys-tool | `date` | RetainedWorker |
| sys-tool | `free` | RetainedWorker |
| sys-tool | `shutdown` | RetainedWorker |
| sys-tool | `hotswap` | RetainedWorker |
| net-tool | `ping` | RetainedWorker |
| net-tool | `curl` | RetainedWorker |
| net-tool | `nc` | RetainedWorker |
| net-tool | `wget` | RetainedWorker |
| net-tool | `httpd` | RetainedWorker |
| net-tool | `mqtt` | RetainedWorker |
<!-- ledger:end -->

**Lifecycle effects (S2c-min, `phase-04-s2c-lifecycle-proposal.md`, Option B):** no builtin keeps an ungated effect; Phase 04 criterion 3 stays open only for network/telemetry/VFS-scoped rights.

- `shutdown` — the builtin issues raw syscall 502, which the kernel now accepts only for the reviewed task name `shutdown` (`RAW_SHUTDOWN_CALLERS`, `kernel/src/task/syscall.rs`); for `cellbox` it logs `raw opcode 502 denied for cellbox`, returns, prints `shutdown: use /bin/shutdown` and exits 1 (`InSuiteEffectDenied`, check `s2-builtin-shutdown-denied-shell-survives`). `/bin/shutdown` is launchable from the shell (one `cellbox → /bin/shutdown` Path edge, `LEGACY` ceiling) and is the only way to power off.
- `kill` — refused with `kill: not available from the suite shell, requires a lifecycle owner`; the cooperative IPC `0xFF` path and `ForceExit` are deleted from the builtin (`InSuiteRefused`, check `s2-kill-builtin-refused-no-ipc`).
- `snapshot` — refused with `snapshot: not available from the suite shell, requires a lifecycle owner`; the shell client is deleted and the supervisor no longer accepts the name `cellbox` (`SNAPSHOT_CALLER_NAMES` is empty), so a snapshot is unavailable until a `SnapshotTarget` owner exists (`InSuiteRefused`, check `s2-snapshot-builtin-refused-no-ipc`).

`blktest` and `shutdown` are the `InSuiteEffectDenied` builtins: the kernel denies the raw block read (`raw block opcode 500 denied … (no bit 36)`, check `s2-blktest-raw-block-denied`) and raw 502 for `cellbox` (check `s2-builtin-shutdown-denied-shell-survives`); the ledger test requires a named existing check for every `InSuiteEffectDenied` and `InSuiteRefused` entry. `hv` is `InSuite`: `/bin/hypervisor` is a reviewed cellbox launch row (`launch_profile/targets.rs`), so the builtin is a launcher whose authority is the reviewed edge, not a denied effect.

`ssh` is `InSuite` on the same reasoning as `hv`: the builtin is a launcher, and its authority is the reviewed `cellbox → /bin/cellbox-ssh` Path edge with the `NET_CLIENT` ceiling (`profiles.rs:70-75`, D79-9 revised). The suite shell `/bin/cellbox` holds no network right. `/bin/cellbox-ssh` is not in the generic reviewed table, so init autoload and the desktop cannot launch it. `edit`, `rg` and `completion` use only rights the shell ceiling already holds; the one ceiling addition since S1 is `GetRandom` for the ssh child (`main.rs:22-26`, D79-9).

### 3.2 One-shot `/bin/cellbox COMMAND` (Phase 10 S10-0, revision 2026-10-26)

Source of truth: `cells/tools/cellbox/src/oneshot.rs` `TABLE` (host-tested: every `executor::BUILTINS` name plus `[`, `.` and the six typed stage words, each exactly once; its builtin names equal the ledger's builtin rows; the block below is byte-equal to the table's rendering). Decisions DS0-1..DS0-6 of [phase-10-s0-proposal.md](phase-10-s0-proposal.md) §2 S10-0, CP1 approved 2026-10-26.

- **Entry.** A launch context's argv (it never carries the program path) is classified once: `ssh …` is the ssh client child (unchanged); with a terminal frontend only `[]`/`["sh"]` start `cellbox sh`, any other argv exits 2; without a frontend `sh` exits 125 (needs a terminal), `[]` or another option-like word prints the usage on stderr and exits 2 (`-h`/`--help`: stdout, 0), a table name runs or is refused as below, and every other name, including an absolute path or a retained worker name such as `/bin/ls`, exits 127 (`cellbox: NAME: unknown command`). There is no external fallback: a one-shot never launches a Cell (no `spawn_external`, `exec`, `hv` or ssh launch), so its authority is a strict subset of `cellbox sh` on the same ELF, path and launch edges. No new launch edge (DS0-1): on canonical images no production launcher reaches the process-level form (init has no context, the Terminal passes `sh`); the in-shell form below serves them.
- **Arguments** are literal: no parsing, expansion, globbing, redirection, `|`, `;` or `&&`; empty and space-containing words survive. The context environment is not imported and `CELLBOX_STREAMS` is ignored; the cwd is the context's, `/` when unset.
- **Streams.** stdout and stderr are the context's Stdout/Stderr grants, captured before anything is written; without both, the one-shot exits 1 and writes nothing anywhere, the UART included. Each write has a 30 s deadline. A failed stdout (closed, revoked, deadline) stops typed producers at once and makes other commands exit 1 after they return; one line goes to stderr if it is still writable (DS0-4). Typed producers (`ps|ls|top --records`, `rg`) stream typed JSONL by default, `--text` the plain renderer; no command draws, reads keys or writes screen control.
- **stdin.** Only the rows marked "reads stdin" read the Stdin grant, and only when the command itself asks for it (it got no file operand, or `rg … -`): the grant is read to end of stream once, bounded by 4 MiB (`typed::MAX_SINK_BYTES`), and a larger input exits 1 with `cellbox: stdin exceeds 4194304 bytes` before the command uses any of it (DS0-5). No Stdin grant is empty input.
- **Status.** A Run command keeps its catalog status; refusals exit 2 (DS0-3) with one stderr line and no effect; unknown names 127; cancellation of the parent's operation ends the child (the parent reports 130).
- **In `cellbox sh`**, a simple command whose first word is `cellbox` or `/bin/cellbox` goes through the same table in process (DS0-2), before typed admission: a Run name (and `ssh`, the shell's own builtin) drops the prefix and keeps the line's grammar (`cellbox ps --records | select tid` admits as typed); every other answer is the one-shot's, without effect (`cellbox cd /` → 2, cwd unchanged; `cellbox` → usage, 2; `cellbox sh` → 2).

<!-- oneshot:begin -->
| Name | `/bin/cellbox NAME` |
|---|---|
| `awk` | Run (reads stdin) |
| `blktest` | Run |
| `cat` | Run (reads stdin) |
| `cp` | Run |
| `date` | Run |
| `echo` | Run |
| `edit` | Run |
| `env` | Run |
| `find` | Run |
| `free` | Run |
| `grep` | Run (reads stdin) |
| `head` | Run (reads stdin) |
| `help` | Run |
| `ifconfig` | Run |
| `ip` | Run |
| `kill` | Run |
| `ls` | Run |
| `mkdir` | Run |
| `mv` | Run |
| `ps` | Run |
| `pwd` | Run |
| `rg` | Run (reads stdin) |
| `rm` | Run |
| `rmdir` | Run |
| `sed` | Run (reads stdin) |
| `shutdown` | Run |
| `sleep` | Run |
| `snapshot` | Run |
| `sort` | Run (reads stdin) |
| `tail` | Run (reads stdin) |
| `tee` | Run (reads stdin) |
| `test` | Run |
| `[` | Run |
| `top` | Run |
| `touch` | Run |
| `uname` | Run |
| `uniq` | Run (reads stdin) |
| `uptime` | Run |
| `vappend` | Run |
| `vcat` | Run |
| `vwrite` | Run |
| `wc` | Run (reads stdin) |
| `alias` | Refused 2: shell state |
| `unalias` | Refused 2: shell state |
| `cd` | Refused 2: shell state |
| `export` | Refused 2: shell state |
| `unset` | Refused 2: shell state |
| `exit` | Refused 2: shell state |
| `source` | Refused 2: shell state |
| `.` | Refused 2: shell state |
| `read` | Refused 2: shell state |
| `history` | Refused 2: shell state |
| `jobs` | Refused 2: shell state |
| `fg` | Refused 2: shell state |
| `bg` | Refused 2: shell state |
| `break` | Refused 2: shell state |
| `continue` | Refused 2: shell state |
| `completion` | Refused 2: shell state |
| `clear` | Refused 2: needs a terminal |
| `exec` | Refused 2: launches a Cell |
| `hv` | Refused 2: launches a Cell |
| `ssh` | ssh client child entry |
| `select` | Refused 2: pipeline stage |
| `where` | Refused 2: pipeline stage |
| `sort-by` | Refused 2: pipeline stage |
| `take` | Refused 2: pipeline stage |
| `to` | Refused 2: pipeline stage |
| `from` | Refused 2: pipeline stage |
<!-- oneshot:end -->

Run rows keep the shell's implementation and its known gaps: `kill`/`snapshot` stay refused (1), `shutdown` and `blktest` keep their denied effects (1), `edit` without a terminal exits 1, interactive `top` without a terminal exits 2 naming `-b` (`top -b` and `top --records` run). The builtin `cat`/`tee` print stdin only when it is valid UTF-8 (NUL included) and a non-UTF-8 stdin prints nothing, exactly as in the shell; the §3 exact-bytes `cat` correction is not part of S10-0.

## 4. Authenticated caller-edge × backend-right matrix

### 4.1 Right vocabulary and actual enforcing owners

The command tables' rights cells and the caller table below define a **closed matrix**: a command requires every listed right; a caller receives only an explicitly issued resource-scoped grant on that exact invocation edge. Every absent edge/right is denied. Being a known command, shell mode, common executable, GUI button, string path or native record does not authenticate a caller. Grant sources/owners below are **proposed** where current code has only ambient gates; their allocation/ABI changes remain CP1-blocked.

| Code / resource | Observed enforcement and exact evidence | Candidate enforcing owner / restricted-caller denial |
|---|---|---|
| R file read/list/stat | service-VFS receives kernel attestation (`main.rs:133-143`), denies unattested; directory admission/path sealing before dispatch; AccessTable and handle ownership (`dispatch.rs:28-79`, `access.rs:58-144`). Legacy kernel BootFS read is a separate syscall path, not proof VFS grants constrain it. | service-VFS existing directory/file refs scoped read/list/stat; common code must not bypass denied VFS via BootFS fallback. Deny protected KMS paths, stale/foreign handle, sealed raw paths, read outside delegated directory; no existence leak. Kernel loader/BootFS owner must preserve exact admitted executable reads separately. |
| W create/replace/append/rename/remove | VFS requires attested VfsMutate flag **before** backend work (`dispatch.rs:46-56`), plus path policy. Bit63 originates kernel attestation (`syscall.rs:2812-2816`). `/bin`/root read-only, `/data /tmp /srv /mnt/sd` mutable prefixes; boot config protected; KMS/live-hypervisor exceptions (`access.rs:108-132`, `access/rules.rs:38-104`). | service-VFS scoped directory mutation rights **and** admitted mutation transport; no ambient bit63 for every common invocation. Deny read-only grant, foreign generation, `/bin`, boot configuration, KMS, guest-disk mutation by non-hypervisor, or source/dest rights mismatch. Existing flags alone are broader than per-command attenuation. |
| C cwd | Kernel Chdir/Getcwd allowlist bit60 (`syscall.rs:9766-9777`); shell VFS stat then chdir (`cmd_cwd.rs:31-68`). | Existing VFS directory owner + D01 explicit cwd grant and shell context; printed cwd does not delegate access. Deny forged/unreadable directory or child context not admitted; no ambient inherited cwd. |
| Q Config key set | `export` calls service-config; Config main.rs:63-94 decodes Get/Set/Delete on receive and mutates store without attested per-key checks. **No observed per-key authorization**; not a secret store. | service-config must authorize attested principal + exact key/write operation under owner-approved delta before common export is enabled. Deny absent/wrong key grant and forged Config identity. Do not invent enforcement from ConfigClient. |
| O clock/delay | Existing GetTime/GetWallSecs and own-operation yield; shell date/uptime/sleep use10 MHz conversions (Y:101-143,268-275). | Kernel admitted clock syscalls, explicit units; no time-setting right. Deny absent allowlist/provider; sleep cannot consume another operation's input or authority. |
| M global physical memory | Kernel MemInfo allowlist bit56 and32-byte output (`syscall.rs:6900-6923,9719-9737`); no scoped Cell heap allocation fact. | Kernel MemInfo for admitted aggregate snapshot, never ambient cross-Cell detail; telemetry owner if restricted profile excludes global snapshot. Deny missing bit/scope; no fallback heuristic. |
| X cross-Cell/task observation | GetProcs/GetProcs2 expose diagnostic task rows under syscall allowlists; GetProcs2 bit55 (`syscall.rs:6864-6898,9673-9685`); no caller-scope filtering in those branches. | D04 kernel telemetry owner with exact scoped subscription/ref; common artifact cannot hold ambient cross-Cell syscall bit merely for ps/top/kill. Own-only caller must be denied other-Cell rows, forged generation and scope escalation. Authoritative Cell-incarnation source remains prerequisite. |
| I local interface query | service-net `GetLocalIp` returns address (`handlers.rs:177-178`); caller attestation at service-runtime:540-569. | service-net query-only operation, no N/Nl/D implied; absent query grant/provider denial. No interface-set route exists. |
| D DNS | service-net Resolve; attested live Cell generation, bounded pending resolver (`service-runtime.rs:540-602`, `handlers.rs:191-198`). **No separate per-invocation DNS grant observed.** | service-net must enforce scoped resolve permission; reject DNS without D even if N socket grant exists; no direct resolver/device fallback. Owner-side attenuation is pending design allocation/CP1, not a present feature. |
| N TCP egress/owned stream | service-net attests CellOwner, socket table owns cap by Cell+generation (`service-runtime.rs:548-602`, `socket_table.rs:111-120,203-217`); create/connect checks quota/owner, not destination grant. NetworkCap belongs service/driver, **not clients** (`launch_profile/targets.rs:53-57`). | service-net exact destination/port and operation-bound connect/send/recv/close authority; foreign/stale cap and no-destination-grant denial. Current ownership protects cross-owner sockets but does **not** enforce candidate destination attenuation. No common NetworkCap. |
| Nl listen/accept | service-net TcpListen/TcpAccept owner/quota checks (`handlers/tcp.rs:120-164`); no distinct authenticated listen grant observed. | service-net explicit port/listen/accept grant to independent worker; deny `nc -l`/httpd from egress-only/observation callers before binding. Listener creation is never justified by curl/SSH egress. |
| E reviewed child launch/wait | Kernel `authorize` selects caller-name/route/path; separate shell/desktop/init/tool-spawn rows; no arbitrary target; Elf cannot borrow nonempty child ceiling from advisory bytes (`launch_profile/mod.rs:54-94`, `profiles.rs:13-94`). Names/paths are existing policy labels, not new measured-image attestations. | Kernel launch owner D01 binds exact real executable edge + prepared child + argv/cwd/grants. Deny unknown `/bin/cellbox` until approved; deny raw/Elf authority borrowing, Mem/Pinned shell launch and undelegated child rights. Wait/reap only owned children. |
| H hypervisor start | Exact `/bin/hypervisor` Path ceiling Hypervisor; shell Elf route denied for nonempty ceiling (`targets.rs:66-74`, mod.rs:82-90). | Kernel loader and independent hypervisor; granted exact launch only, never HypervisorCap in common cellbox. Deny ordinary ELF/advisory substitution or missing controlling launch grant. |
| K lifecycle kill | Kernel ForceExit SpawnCap, no self, protected block/network system cells and Frozen (`syscall.rs:5498-5535`). Legacy Waiting `0xff` IPC is **not** owner-enforced lifecycle delegation. | Existing service-supervisor D01 KillTarget exact Cell lifetime + kernel supervisor mechanisms; deny no/wrong/stale target grant and raw/cooperative IPC bypass. Do not grant common SpawnCap just for kill. |
| Z system reset/poweroff | **Enforcement gap:** Shutdown excluded from syscall-to-allowlist mapping (`syscall.rs:4102`); branch7735-7780 has no capability check; x86 hlt loop, not evidenced ACPI S5. Current standalone launch absence is not syscall authorization. | Existing service-supervisor D01 ResetSystem + kernel gated platform provider; deny restricted raw502 as well as service request. Physical poweroff/reboot unsupported until real platform backend exists; no reboot implementation inferred. |
| Hs service replacement | Supervisor checks sender TID resolves exact name `hotswap` using GetProcs64 (`supervisor/src/main.rs:54-79,132-152`); launch special shell-only hotswap edge; SupervisorCap syscalls control swap (`hotswap.rs:95-118`). **Name lookup is legacy policy, not a generation-bound replacement grant.** | service-supervisor D01 ReplaceService exact registered incarnation/path+image; deny desktop/tool-spawn/no/wrong/stale grant, arbitrary service/path, forged hotswap name/argv and raw Elf label. Preserve 63/127 legacy CLI limits until explicit migration. |
| S global snapshot | Supervisor accepts exact `shell` name via GetProcs; kernel Snapshot SupervisorCap (`supervisor/main.rs:82-91,132-152`, `syscall.rs:7843-7855`). Current mechanism serializes global allocated frames. | service-supervisor system-scoped SnapshotTarget; reject narrower target-Cell grant as unsupported by current global provider. Deny common caller/name spoof/stale grant; no raw Snapshot capability in common artifact. |
| T terminal reader/control | Current input focus exists; shell read/top use it. GUI `has_reader_lease` is local boolean, not authentication (`terminal/src/session.rs:1-140`). | D02 existing service-input + kernel endpoint owner enforce exact session/foreground epoch/ref/mode lease; deny wrong session/background/stale principal/key-read and redirected control. Only granted endpoint subtree cancelled; no global focus stealing. |
| B raw block/device | Kernel raw block allowlist bit36 plus BlockIoCap (`syscall.rs:7781-7788,9375-9377,9433-9437`). | Existing kernel block/device owner; **no catalog command receives B**. blktest requires denial, clients use services; no union device capabilities. |
| A SSH credential/private-key signing | No outbound SSH credential operation is evidenced in the current suite. VFS KMS subtree is service-only, not a generic identity-file directory. | Existing KMS/credential owner, under separately approved credential handoff, owns password/unlock admission and selected Ed25519 identity/session-bound signing. Common cellbox receives only exact operation refs/results, no raw key export or ambient secret VFS access. Deny missing/wrong/foreign/stale key/auth ref, missing secret-input lease, untrusted-host credential release and unrestricted signing requests. A does **not** authorize host-trust lookup/mutation or a D03 trust decision. No key generation/import action authorized here. |
| Vt SSH host-trust lookup/decision | No outbound host-trust backend is evidenced in the current suite. | **D03's existing VFS service-hosted narrow trust worker is the sole trust-state owner/enforcer**, not KMS and not a new competing trust service. Exact host+port+algorithm+complete-key lookup and opaque connection-bound decision; unknown requires separately scoped trust-create authority, D02 exclusive confirmation challenge and D03 durable conditional commit. KnownExact can use lookup-only Vt. Deny unknown without authorized confirmation, changed key unconditionally, stale/foreign/replayed receipts and failed/uncertain persistence; no raw known-host mutation or key-signing authority. Protected bootstrap/storage/codec handoff remains CP1 pending. |

### 4.2 Caller edges (apply to every command row, no argv-selected privilege)

| Authenticated edge / caller | Current observation | Candidate allowed-right rule and exact denial fixtures |
|---|---|---|
| init → shell/startup | init has SpawnCap and exact boot/autoload rows; `/bin/shell` admitted, `/bin/cellbox` absent (`profiles.rs:13-53`). | Proposed init→`/bin/cellbox ["sh",…]` only after D01; base own context plus explicitly owner-issued R/W/C/Q/O/M/X/I/D/N/E/H/K/Z/Hs/S/T/A/Vt grants as needed, **not a union grant or init's capabilities**. A and Vt must come from their independent enforcing owners, never from argv. Startup argv `kill`/`sh` cannot mint any right. Unknown path/route/missing grant denial before publication. |
| existing shell → in-process builtins/functions/source | Builtins execute as shell identity/manifest, including VfsMutate/observation syscalls and direct raw shutdown; sourced command is not a fresh authenticated principal. | Every command checks its rights against the invoking shell's operation grant, not its name; functions/source/aliases cannot amplify. Check R/W on redirects too. Read-only shell must fail mkdir/rm/vwrite/wget/kill/shutdown/hotswap/snapshot, missing E fails exec/hv, all before unauthorized backend effect. |
| shell → standalone suite or independent tool | Exact Path/Elf rows and ceilings (§3); `/bin/hotswap` special shell-only; `/bin/ping`, `/bin/kill`, etc. absent, despite source binaries. | Migrated suite uses canonical cellbox command; independent nc listener/httpd/mqtt/hypervisor keep their own paths/ceilings. Pass only command-required scoped refs through D01; child receives no ambient parent endpoint/secret. Absent reviewed edge remains denied, not reopened with wildcard or common artifact rights. |
| desktop → app path / future tooling action | Separate reviewed Path/Elf profile, empty ceiling for files/terminal/desktop; no cellbox/hotswap special row (`profiles.rs:64-69`, targets.rs:49-57). | Desktop chooses real typed operation with user's scoped R/W/X/etc. resource grants, never parses table/JSON to obtain authority. Shutdown button requires Z; observation view X/M; Files R/W. A path launch does not convey them. Deny desktop hotswap, undelegated lifecycle/device/key/egress rights and forged “authorized” UI state. |
| GUI Terminal frontend → real shell | Current local fake executor, not existing shell transport. | D02 session frontend grant permits only its own byte/input/resize attachment T; it does not confer R/W/E/K/Z/Hs/S/X/N/D/A/Vt on shell. Shell gets separate grants from trusted controlling owners. A frontend confirmation view cannot issue its own trust receipt or key grant. Wrong tab/session/lease generation denial, no keys consumed by other foreground operation. |
| restricted cellbox invocation / untrusted app | No cellbox executable or launch edge exists. App prefixes `/apps/`, `/data/bin/`, `/home/` receive EMPTY ceiling, not a cellbox rule (`targets.rs:99-105`). | Base own-operation commands only; R-only can cat/ls/grep/rg within grant but no W/Q/K/Z/Hs/S/N/D/Nl/X/A/Vt; N-only cannot DNS/listen/write files/authenticate/accept host trust; X-only cannot kill; T-only cannot open files/control lifecycle/authenticate/accept host trust. A-only cannot lookup/accept host trust; Vt-only cannot release credentials/sign/authenticate. Every known command remains discoverable but unavailable rights yield nonzero no-effects denial. Forged mode, path, record, owner ref and replayed generation do not authorize. |
| tool-spawn/hypha → children | Separate exact caller profiles; tool-spawn uses reviewed targets, no special hotswap; hypha only llm-gateway/tool-fs/tool-sys/tool-spawn (`profiles.rs:72-94`). | No inferred cellbox edge or bootstrap rights. Any admitted automation invocation has explicit resource grants per command; denial of hotswap/kill/reset/snapshot/keys even if E is granted. This catalog does not widen unrelated automation profiles. |
| service worker → backend | VFS/net/input/supervisor/KMS retain separate providers/identities; ordinary client connectivity is not authority delegation. | Owner authenticates kernel principal+generation, resource+rights+live operation; only owner handles privileged kernel/device action. Refuse malformed/stale/foreign/revoked grant, missing attestation and denied child context. Worker restart cannot revive previous incarnation's refs. |

**As-built edges (revision 2026-10-25).** The "Current observation" column above records 2026-10-20. Where the table below states an edge, it supersedes those cells; the candidate rules above still apply to every edge.

| Caller → target | Route / ceiling | Evidence | Approval |
|---|---|---|---|
| init → `/bin/cellbox` `["sh"]` | Path; boot ceiling `LEGACY` (`boot_ceiling.rs:119`), init launch edge (`profiles.rs:24-54`) | `config/autoload.toml:23-25`; `/bin/shell` is gone from profiles, init, desktop and recipes | P04 S1 CP1 2026-10-21; CP2 requested in [phase-04-s1-s3-cp2-dossier.md](phase-04-s1-s3-cp2-dossier.md) |
| terminal → `/bin/cellbox` | Path only; `LEGACY` (`profiles.rs:148-156`); Elf/Mem/Pinned denied (`tests.rs:546-558`) | GUI lane 290/290, 2026-10-25 | P04 S3 CP1 2026-10-21; CP2 requested in the same mini-dossier |
| cellbox → `/bin/shutdown` | Path only; `LEGACY` (`profiles.rs:66-69`) | §3.1 | P04 S2c-min Option B CP1 2026-10-21; CP2 requested in the same mini-dossier |
| cellbox → `/bin/hotswap` | Path or Elf; `LEGACY` (`profiles.rs:65`) | legacy special edge, unchanged | pre-Cellbox |
| cellbox → `/bin/cellbox-ssh` | Path only; `NET_CLIENT` (`profiles.rs:70-75`); Elf denied; not reachable from init, desktop, terminal or `cellbox-ssh` itself (`tests.rs:155-157`) | Phase 09 lane 103/103 | P09 D79-9 revised, CP1 2026-10-25; S9-2 CP2 2026-10-25 |
| cellbox → reviewed user targets | Path or Elf; the target's reviewed ceiling (`profiles.rs:76`, `targets.rs`) | — | S1 rename of the existing shell edge |
| phase04-terminal-driver → `/bin/cellbox` | Path; `LEGACY`; only under kernel feature `phase04-driver-profile` (`profiles.rs:158-175`) | P04 terminal lane | private fixture only (S3) |

`/bin/cellbox-ssh` is not installed by any canonical image recipe; only `scripts/sign-policy.py` and the private P04/P09 runners name it. That is a Phase 04 criterion 4 / Phase 10 packaging item (closure dossier C1-f), not a catalog gap.

The cross product is explicit: e.g. `wget` requires N+D+W for each caller above; an egress-only shell lacks W and must not write, an R-only desktop lacks all three, and a T-only terminal frontend lacks all three. `top -b` task sampling requires X but no T; interactive top additionally T; a global physical-memory snapshot independently requires M. `test -z` needs no R, `test -f` does. `rm -i` additionally T only when terminal confirmation is needed. For **every caller edge**, SSH requires N+A+Vt+O (D additionally for a DNS hostname, T additionally for PTY/secret/first-trust confirmation). A without Vt fails before credentials/signing; Vt without A cannot authenticate; N+D without both cannot perform either protected operation. A does not mint trust, Vt does not mint key authority, R/W do not bypass protected stores, and T does not authenticate a client-supplied confirmation boolean. KnownExact lookup-only Vt cannot persist an unknown host. Both owner refs are validated independently against the same live operation and host/connection before credential release. This is not an implicit all-tools/common-root permission. New typed transforms `select/where/sort-by/take/to/from` require only their supplied data/own-operation budgets, not R/W/X merely because rows contain paths/Cell IDs (spec.md §2).

## 5. Desktop/backend operations and missing CLI paths

| Observed GUI/backend edge | Actual CLI relation / candidate disposition |
|---|---|
| Terminal fabricated `uname`, `pwd`, `ls`, `ps`, `date` | **Historical (2026-10-20):** `cells/apps/terminal/src/session.rs:1-140` locally split text and fabricated responses. **As built:** the private matcher and fixed responses are deleted; the GUI Terminal launches the real `cellbox sh` through `TerminalHost` (P04 S3, S3-output, S3-gui and S5-3c CP2s; GUI lane 290/290 on 2026-10-25). |
| Desktop Taskbar/Spotlight launches | `cells/apps/desktop/src/main.rs:153-158,225-229,272-275`, `apps.rs:13-110`: app-path launch, not typed backend or CLI-output parsing. Keep independently launchable apps; E authorization remains kernel-side. |
| Desktop `/bin/app-sys-tools` | **Removed (C5c, CP2 2026-10-22).** It was a broken edge: no reviewed target, and sys-tools ships individual binaries. Shared observation actions link to ps/top/free/etc., not to a privileged catch-all app. |
| Desktop `/bin/service-httpd` | **Removed from the desktop catalog (C5c).** The independent `/bin/httpd` worker (R+Nl, `NET_LISTENER`) is retained. A GUI entry would need an explicit listener/file grant and exposure approval. |
| Desktop `/bin/app-c-math-smoke` | **Removed from the desktop catalog (C5c).** It stays a verification fixture outside the suite. |
| Spotlight ShutDown / Reboot | **Removed (C5c, D-C5-2):** the fake "Authorized system shutdown initiated" actions are gone until a provider exists. The only poweroff path is `/bin/shutdown` (§3.1). No reboot CLI or provider exists, and a desktop typed Z invocation is still missing. Do not fabricate `reboot` support. |
| Files hard-coded directory/file listing and `/bin` list | **As built (C5b, CP2 2026-10-22):** Files uses `ostd::clients::VfsClient` (`ListDir`/`Stat`/`Write`/`Append`/`Rename`/`Unlink`/`Rmdir`/`RmdirRecursive`) against the same VFS service as cellbox `ls`/`cat`/`mkdir`/`cp`/`mv`/`rm`, with a `+VfsMutate` ceiling. Parity means the same service and request types, not shared client code. |
| New search/editor/SSH | Product names are `cellbox rg`, `cellbox edit` and `cellbox ssh` (`docs/cellbox-cli.md:34-49`). **All three dispatch as built** (§2 rows `rg`, `edit`, `ssh`, `completion`). `rg` has existed since S7-3 ([proposal](phase-07-s3-proposal.md) §10): an always-typed row producer (`typed.rs` `Class::Search` → `native_rows::row_producer` → `rg::producer`, VFS through `rg/tree.rs` `VfsTree`), and `rg -a PATTERN -` searches the head's bytes (`Class::SearchStdin`). `edit` is a foreground Session (P08) that saves only through the D03 conditional save on `/srv`. `ssh` is a Session launcher (P09) that re-launches the cellbox ELF as `/bin/cellbox-ssh` on the reviewed `NET_CLIENT` edge. The contracts are [command-contracts.md](command-contracts.md) §2–§4, with recorded per-phase deviations. Rights as built: rg R for paths/traversal, none for explicit byte stdin; edit R+W+T with D03 save; ssh N (+D) on the `/bin/cellbox-ssh` edge, Vt from the D03 VFS trust worker, T, O. KMS credential A is deferred (`--auth publickey` refused, D79-14). A and Vt remain independent owners and rights. There is no legacy grep substitute, no fake editor and no inbound sshd. |
| New Cell-first top | Existing command is task telemetry only; no authoritative Cell lifetime/provider proven. Keep current task options/drill-down; D04 Cell observation API + CLI schema is missing provider work, not complete by changing title/grouping task names. |

## 6. Image/startup evidence and retained gates

- **As built (2026-10-29).** Init autoloads `/bin/cellbox` with argv `["sh"]` (`config/autoload.toml:23-25`). The x86 recipe is `scripts/assemble-x86-image.py` (`--profile headless|desktop`), the single source behind `scripts/build-x86_64-cells.ps1`, `scripts/package-desktop-x86.py` and the CI x86 job; it packages `cellbox` as `/bin/cellbox` and the same signed file as `/bin/cellbox-ssh` (D10-5), assembles fresh (no overlay), and refuses `/bin/shell`, `/bin/sshd` and the quarantined hashes. `scripts/gen-disk-ci.sh` and `scripts/build-aarch64-cells.ps1` still package `cellbox` for the non-Intel images (refused, D10-10). **Canonical state (S10-5, user CP2 pending):** `kernel/src/embedded-x86_64/{kernel_fs.img, init, boot-config/, manifest.json}` hold the qualified **desktop** profile (`kernel_fs.img` `3848cefb…`, `init` `c0bd1458…`, hash-identical to candidate `4fae8b20…`) and the default kernel path holds the qualified kernel `dfbe8c55…`; the headless qualified bytes stay in the candidate and CI builds a fresh headless image. Dev posture, Intel x86 QEMU/TCG only; see [rollout-containment.md](reports/rollout-containment.md). A policy row is still not packaging/startup proof.
- **Historical (2026-10-20 preparation).** At that time the x86 builder packaged `shell` and only `ls/cat/echo/ps/kill` (`build-x86_64-cells.ps1:158-177` of those bytes). `run-desktop.sh:18-24,67-81` kills current image users and exposes guest SSH on host port 2222: it is not an unattended verification action. `tests/integration/tests/desktop-x86.rs:1-187` exercised boot, the serial prompt and help only.
- **Source changes since preparation.** The preparation step itself changed no source. Phases 02–10 later changed source, launch rows and images under their own recorded CP1/CP2 (see the reconciled index in [contract-deltas.md](contract-deltas.md)). The canonical x86 image and default kernel are promoted as described above (2026-10-29; the rollout guards G1/G3 and the canonical assembler route were reopened by S10-2b; the quarantine hashes stay refused forever). Final migration must still reconcile actual ELF paths, image entries, signing lists and launch rows; do not infer them from build-script equivalence.

### Precisely unresolved external/owner decisions

1. **Maintainer candidate approval and digest binding:** these selected compatibility changes (N normalization, byte-preserving cat/tee, parsed REPL state commands, strict bracket/flags, explicit incomplete errors, canonical text formats and independent nc/httpd/mqtt roles), plus the exact new-command contracts in command-contracts.md, need retained contract review and script/caller migration evidence. They are proposed decisions, not behavior silently left to an implementer; this document does not supply maintainer signatures.
2. **D01/D02/D04 allocation and file-owner handoff:** exact `/bin/cellbox` identity/Path/Elf ceilings, admitted syscall list, boot/desktop/automation edges, service-right codecs/selectors and controlling source owners remain pending CP1. Existing exact-name supervisor checks are not sufficient new authentication. CP2 requires actual diff, exhaustive migrated caller/image census and real denied-edge/runtime evidence before retirement.
3. **Unenforced attenuation:** service-net lacks destination/DNS/listen grant gates; service-config lacks per-key grants; legacy global observation syscalls have no scoped view; raw Shutdown lacks authorization and x86 real reset provider. Their named existing owners must ratify concrete enforcement deltas; a catalog cannot grant or pretend them. Do not bypass by adding ambient rights to common cellbox.
4. **Global snapshot versus target-scoped control:** current kernel snapshot is global. Candidate preserves global operation with a system grant; any future per-Cell snapshot requires its own real mechanism/approved delta, not reuse of global snapshot under a narrow target grant.
5. **Unavailable providers/qualification:** authoritative Cell-incarnation telemetry, D02 real terminal streams, conditional/durable VFS save, SSH entropy/key/trust runtime and admitted upstream ports require the separate contract/dependency artifacts and authorized target evidence. Source reads cannot resolve hardware/server availability, sign policy, human approvals or retained locks.

**Coverage conclusion:** every completion name, the two noncompletion semantic aliases, all eleven sys-tools binaries, all six net-tools binaries (with distinct nc modes and MQTT modes), current shell script/launch behavior, named GUI edges and new scoped CLI gaps has a candidate disposition and acceptance scenario. Every command maps to a closed caller/right rule with its enforcing owner and restricted-caller denial; absent current enforcement is explicitly identified. This completes the reachable **catalog preparation**, not implementation, runtime compatibility qualification or approval.

2026-10-20 — Added the exact new-command candidate annex; separated KMS credential/signing A from D03's sole VFS-hosted trust-worker Vt in owners, caller denials and every SSH crossproduct. No source/ABI edit, build/test/formatter/probe, signing/key action or network operation; CP1 remains pending.

2026-10-25 — Catalog revision for the Phase 01 closure dossier gaps C1-a..d. Added §2 rows for `rg`, `edit`, `ssh` and `completion`; a typed-stage/record-mode subsection; the `ssh` launcher note in §3.1; the as-built caller-edge table in §4.2, including `cellbox → /bin/cellbox-ssh`; and as-built text in §1, §3, §5 and §6. The §2 legacy observed columns are marked as the historical 2026-10-20 baseline. The ledger block between the `ledger:begin`/`ledger:end` markers is unchanged. Doc-only: no source, build or runtime action. The new digest is recorded in [phase-01-closure-dossier.md](phase-01-closure-dossier.md) and [contract-deltas.md](contract-deltas.md). It needs user acknowledgement; this revision is not an approval.

2026-10-26 — Catalog revision for Phase 10 S10-0 (DS0-6, CP1 approved 2026-10-26): added §3.2, the one-shot `/bin/cellbox COMMAND` table between the `oneshot:begin`/`oneshot:end` markers (host-tested against `cells/tools/cellbox/src/oneshot.rs`), with its entry, stream, stdin, status and in-shell rules; revised the §1 as-built paragraph and the §3.1 entry sentence. The ledger block is unchanged. The new digest is recorded in the S10-0 results of [phase-10-s0-proposal.md](phase-10-s0-proposal.md). It needs user acknowledgement at the S10-0 CP2; this revision is not an approval.

2026-10-29 — Catalog revision for Phase 10 S10-5 (user CP1 approved 2026-10-29): §6 "As built" and "Source changes since preparation" updated to the promoted state. No ledger block, §3 table or command contract changed. The catalog digest changed (the new value is recorded in the S10-5.13 results of [phase-10-s0-proposal.md](phase-10-s0-proposal.md)); it needs user acknowledgement at the S10-5 CP2; this revision is not an approval.
