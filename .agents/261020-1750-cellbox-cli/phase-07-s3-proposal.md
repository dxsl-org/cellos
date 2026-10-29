# Phase 07 slice S7-3 — text-engine hooks, `cellbox rg` and deletion of `text_engine::search` (CP1 proposal)

Status: **APPROVED (user, 2026-10-24): CP1 per every §9 recommendation — D76-1..D76-18, including the vigrep delta (D76-2), the `RG_PROFILE` deviation DV7-10 (D76-3 M1) and the DV7-8 clarification (D76-5 S1).** Implementation results: §10. Drafted as a read-only investigation of the dirty working tree (HEAD `3471028bc`, 2026-10-24). Accepted inputs, not reopened here: D70-6 (delete `libs/text-engine/src/search.rs` in S7-3), D74-1 C1 shared interrupt latch, D74-2 producer completion status, D74-3 streaming text renderer, D74-4 per-entry diagnostics plus one end summary, D74-5 H1 per-directory reopen, D74-6 keep Path/Bytes and fix the docs example, D74-7 DV7-1..9 ratified. Done before this slice: S7-0 admission, S7-1 `libs/vigrep` ([admission record](phase-07-s0-admission.md) §10), S7-2 VFS paging ([entry dossier](phase-07-entry-dossier.md) §14; private lane `reports/s7-2-evidence`: kernel AP 39/39 + 3 dir-page markers, terminal 430, P03 62, P02 48, GUI 279). Tests are behavioral only; no source-text or wording pins.

## 1. Facts

### 1.1 Contract surface (normative: `command-contracts.md` §2)
- **Argv** (`:17-22`): `rg [OPTIONS] PATTERN [PATH…]` or `rg [OPTIONS] -e PATTERN [-e …] [PATH…]`.
  - No path means the shell cwd, never stdin. A sole `-` path means stdin and must be the only path.
  - At most 16 paths, 32 patterns and 4,096 aggregate pattern bytes. An empty pattern is valid.
- **Supported options** (`:24-39`): `-e/--regexp` (repeatable), `-F/--fixed-strings`, `-i/--ignore-case`, `-w/--word-regexp` (not with `-x`), `-x/--line-regexp`, `-n/--line-number` (default; changes no record), `-q/--quiet` (full scan; not with context), `-A/-B/-C N` (N = 0..20; `-C` not with `-A/-B`), `-g/--glob` (≤ 32, ≤ 4,096 B), `--hidden`, `--no-ignore`, `-a`, `--encoding none`, `--text`.
  - `--text` is the Cellbox renderer selector, **not** upstream's alias for `-a`. Help must say so.
  - Every other upstream flag is unsupported, including `-u`, `-L`, `-P`, `-U`, `-z`, `--pre`, `--json`, `-o`, `-f`, `--type*`, count/list/sort flags (`:41`).
  - Backreferences and look-around reject with status 2, never falling back to `grep`. A literal LF in a pattern rejects (`:41`).
- **Shared rules** (`:7-11`):
  - Options precede operands; `--` ends options. No combined short options, `--name=value`, abbreviations or repeated scalar options.
  - A sole `-h`/`--help` returns 0 with no effect. Validation errors come before any file open.
  - The shared encoded argv limit is 512 bytes.
- **Traversal and bytes** (`:47-51`): roots in argv order, depth first, children byte-sorted, `.rgignore > .ignore > .gitignore`, hidden excluded by default, no links followed, NUL means binary with a stable preflight, LF is the only separator (CR is content), offsets count original bytes.
- **Row** (`:55`), field order fixed:
  - `schema` "cellbox.search.line", `schema_version` UInt 1, `kind` "match"|"context";
  - `path` Path or Null (stdin), `line_number` UInt (1-based), `absolute_offset` UInt;
  - `line_bytes` Bytes (no LF), `text` String|Null, `terminator` Bytes (LF or empty);
  - `submatches` List of `{start, end}`: half-open, relative to `line_bytes`, ordered, non-overlapping. Context rows have none.
  - The completion summary is separate control, not a row: `files_scanned`, `files_binary_skipped`, `links_skipped`, `lines_scanned`, `matching_lines`, `matches`, `bytes_read`, `complete`, `reason`.
- **Text renderer** (`:57`):
  - Rows render as `ESCAPED_PATH:LINE:ESCAPED_LINE` (match) or `ESCAPED_PATH-LINE-ESCAPED_LINE` (context), path `<stdin>` for stdin.
  - `\\`, `\n`, `\r`, `\t`, `\0` are escaped as shown. Other controls, invalid UTF-8 and path `:`/`-` become `\xHH` (uppercase hex).
  - No ANSI.
- **Limits** (`:59`): 16 KiB read scratch; line ≤ 65,536 B; ≤ 4,096 submatches per line; compiled regex ≤ 2 MiB; depth 64; 65,536 visited entries; per ignore file ≤ 65,536 B and 1,024 rules; ≤ 256 KiB retained ignore bytes; ≤ 64 KiB retained context; "backpressure stops reads".
- **Status** (`:61`):
  - 0: ≥ 1 matching line and complete. 1: complete with none. 2: any error, even after matches.
  - 130 on Ctrl-C. A per-entry error continues the walk.
  - `take` close releases every handle "without an error".
- **Engine rules** (spec):
  - At most 8 queued native records and 256 KiB resident payload per edge; "every producer is pull/backpressure-aware" (`spec.md:14`).
  - The pipeline status is the final stage's. An upstream failure is kept in the member status and reported on stderr. Search is the cataloged 0/1/2 exception (`spec.md:50`).
  - Nonterminal sinks default to typed JSONL; `--text` opts into plain text (`spec.md:40`).

### 1.2 Existing `grep` relation: unchanged, kept distinct
- The catalog row `grep` says "retain bounded engine/flags and 0/1/2, **no fake `rg`**" (`command-catalog.md:47`).
- The Phase 07 overview says "Keep existing grep's contract distinct; renaming bounded ERE-lite matching is not this phase" (`phase-07-ripgrep-search.md:13`).
- The new-command row says not to substitute legacy grep (`command-catalog.md:276`), and the contract says backreferences "never fall back to legacy grep" (`command-contracts.md:41`).
- No document deprecates or aliases `grep`.
- **S7-3 therefore leaves `grep` alone:**
  - its flags, ERE-lite matcher (`libs/text-engine/src/matcher.rs`), 65,536-byte input bound and 0/1/2 stay as they are;
  - the help line lists both commands;
  - the `grep -r` listing-truncation defect stays separate (D70-5).
- The new Unicode features that vigrep brings to `regex-automata` do not change `grep`: text-engine builds its regex with `.unicode(false).utf8(false)` (`libs/text-engine/src/matcher.rs:63-66`).

### 1.3 Engine today (Phase 03/06 code)
- **`RowSource`** is `{ next_item, close }` (`libs/text-engine/src/pipeline.rs:1597-1603`). It has no completion channel.
  - `Upstream` closes and **drops** its source on EOF, on error and on close (`:1631-1662`).
  - Six text-engine wrappers sit on it: OrderGuard `:1673`, Select `:1738`, Where `:1769`, Take `:1797`, Sort `:1886` and SortFrame `:1924`. Cellbox adds `Tap` (`typed.rs:656-662`).
  - `build_chain` wraps the producer once with the order guard and taps every stage (`typed.rs:698-721`).
- **Ctrl-C** has three independent pollers of the same session queue, and each one discards non-interrupt events:
  - `TerminalSink::poll` before every pull and during backpressure (`executor.rs:1346-1388`; the pull-side poll is `typed.rs:873`);
  - `ExtRun::poll_interrupt` every 16 pump steps (`executor.rs:3508-3521`);
  - nothing in the Collected path (redirect, capture, byte consumer: `typed.rs:730-765`).
  - One poll is `drain_control` plus a clock read plus a `PollEvents` IPC to the input owner (`libs/terminal/src/session.rs:97-138`).
- **Statuses.**
  - `StreamEnd` maps only to 0 or 130 (`typed.rs:789-802`).
  - A typed failure prints `shell: STAGE: msg` and returns **1** (`executor.rs:1649-1653`).
  - Prior nonzero upstream statuses are reported as "`X` exited with status N" (`typed.rs:1198-1210`).
- **Default sink.**
  - The terminal or `--text` gets `DefaultSink::Table` (`executor.rs:1599-1604`). For Rows, Table buffers the whole stream, up to 4,096 rows and 512 KiB (`typed.rs:44-49`, `:902-905`).
  - Otherwise the default is typed JSONL.
  - Collected outputs are bounded only by `MAX_SINK_BYTES` = **4 MiB** (`typed.rs:43`, checked at `:759`).
  - Explicit serializers stream (`typed.rs:865-883`). External tails get typed JSONL through `RowFeed` (`typed.rs:938-968`); `text_display` is not passed there.
- **`--text`.** Admission strips **every** `--text` word from a producer argv (`typed.rs:544-550`). For `rg` that would delete a pattern or path spelled `--text` (e.g. `rg -e --text f`).
- **Byte heads.**
  - `Cat`, `Capture` and `FromFile` load the bytes whole (`executor.rs:1480-1495`; `cat` ≤ `CAT_READ_MAX` = 1 MiB, `cmd_fs.rs:828`) and must be followed directly by `from` (`executor.rs:1545-1566`).
  - External heads stream through `ByteSource`/`StreamRows` (`typed.rs:1032-1143`, 512-byte chunks).
- **Registry and lists.**
  - Native producers are `ps|ls|top` with `--records` (`native_rows.rs:30-55`, `typed.rs:175`, `might_be_typed` `:185-189`).
  - Executor `BUILTINS` (`executor.rs:788`) must equal the ledger builtins (`ledger.rs:224-228`), whose count is pinned at **56** (`ledger.rs:216-220`).
  - The catalog ledger block is generated from the ledger (`ledger.rs:328-336`).
  - The help Files line is `commands.rs:9`. `grep` is dispatched at `executor.rs:1950`.
  - Cellbox has a single shell entry; argv is not readable (`main.rs:116-119`).
- **`text_engine::search`.** It is untracked (`?? libs/text-engine/src/search.rs`, 362 lines, 3 tests) and exported at `libs/text-engine/src/lib.rs:46`. A repository grep finds no Rust consumer. The defects that make it unusable are listed in dossier §2.1.

### 1.4 vigrep (S7-1)
- **`ViSearchTree`** (`libs/vigrep/src/walk.rs:121-157`): `open_dir`, `next_page`, `open_file`, `read_chunk`, `file_state`, `close(Handle::{Dir,File})`. Paths are canonical bytes, and `open_dir` on a file must return `NotADirectory`.
- **`WalkBuilder`** (`:384-468`): roots, `hidden`, `ignore_files`, `overrides`, `binary`, `limits`.
  - `WalkLimits::default()` (`:194-208`): depth 64, 65,536 entries, 4,096 per directory, **1 MiB pending names**, path 1,024, ignore file 65,536 B / 1,024 rules, **256 KiB** retained ignore bytes, scratch 16 KiB.
- **`Walker::next(tree, cancel)`** (`:523-595`) yields `File{path, explicit}`, `MountSkipped`, `Error(WalkError)` or `Cancelled`. The cancel callback is polled per entry, per listing page and per read chunk (`:39-41`).
- **`Walker::search_file(tree, path, searcher, matcher, sink, cancel)`** (`:607-711`) does **open → preflight (whole file) → state check → `search_reader` → state check → close** in a single call. The file handle never outlives the call.
- **Errors.** `WalkError{path, kind}` (`:251-256`), 13 kinds (`:213-247`). `SearchFileError::{Walk, Sink, Cancelled}` (`:342-349`). `WalkStats` (`:363-380`) has no `lines_scanned`, `matching_lines` or `matches`.
- **The searcher is push-based.** Each row is a call to `Sink::matched`/`context` inside `search_reader`.
  - A sink returning `Ok(false)` **stops** the search. The core does not commit its state for that row: `last_line_visited`/`after_context_left` are updated only after `keepgoing` (`searcher/searcher/core.rs:544-561`). On the slow path the position is advanced before the sink call (`:365`).
  - So upstream cannot be paused and resumed mid-buffer as written.
- **Submatches.** The "upstream printer" submatch rule (search from the line start inside the buffer so look-behind sees preceding bytes, pop a terminal zero-width match) exists **only in the differential test** (`libs/vigrep/tests/differential/main.rs:425-461`).
- **Regex defaults.**
  - `size_limit` is 100 MiB and the lazy-DFA cache (`dfa_size_limit` → `hybrid_cache_capacity`) is **1000 MiB** (`libs/vigrep/src/regex/config.rs:66-67, 253, 263`). The onepass limit is a fixed 10 MiB (`:257`).
  - Builder knobs exist: `size_limit`, `dfa_size_limit`, `case_insensitive`, `word`, `whole_line`, `fixed_strings`, `line_terminator`, `build_many` (`regex/matcher.rs:61, 111, 217, 231, 289, 351, 359, 366`).
- **Searcher buffer.** Default capacity is 64 KiB (`searcher/line_buffer.rs:14`); `buffer_capacity`/`heap_limit` are exposed (`searcher/searcher/mod.rs:398, 415`).

### 1.5 VFS client (S7-2)
- **`ostd::clients::VfsClient`** (`libs/ostd/src/clients/vfs.rs`):
  - `open_root_dir(&str)` `:60`, `list_page(dir, cursor, out) -> VfsDirPage{next, complete, changed, entries()}` `:68`;
  - `open_file_at(dir, name)` `:78`, `read_chunk(file, offset, buf)` (≤ `VFS_READ_CHUNK_MAX` = 4,000) `:84`;
  - `file_state -> {size, modified}` `:90`, `close_file` `:95`, `close_dir` `:100`, path `stat` `:135-143`;
  - `VFS_DIR_PAGE_MAX` is at `stream.rs:22`.
  - Wire errors map 1→IO, 2 (quota)→OutOfMemory, 3→PermissionDenied, 4 (handle)→NotFound (`clients/vfs/read_file/wire.rs:3-17`).
- **`OpenRootDir`** (`cells/services/vfs/src/dispatch_dirs.rs:121-141`) validates the path (invalid → ERR_DENIED) and checks `can_read` (→ ERR_DENIED). **Missing and not-a-directory both answer ERR_IO.** `Stat` is R-checked, and a missing path is also ERR_IO (`dispatch_paths.rs:29-37`). VFS cannot say "not found".
- **`CloseDir` revokes every derived handle** (`libs/api/src/services/ipc.rs:151-158`). So a file opened with `OpenFileAt` dies with its directory handle.
- **Budgets:** 32 directory handles per Cell (`cells/services/vfs/src/dirs.rs:37`) and 32 file handles per caller (`file_handles/table.rs:11`).
- **Current use.** Cellbox file commands call `VfsClient::new()` per operation (`cmd_fs.rs:26-41, 773-825`). `cat` has a BootFS fallback (`bootfs_fallback_allowed`, `cmd_fs.rs:37-40`).
- **Paths.** `resolve_shell_path` canonicalises `.`/`..`/empty components, but **falls back to `/` when the cwd is unreadable** (`cmd_fs.rs:200-207`).

### 1.6 Memory and size
- **Heap.** Cellbox runs on the default 1 MiB `linked_list_allocator` arena (`libs/ostd/src/heap.rs:28`); there is no fallible allocation (`typed.rs:46-49`). Exhaustion logs `OOM: cell heap exhausted` and exits the Cell, which takes down the shell (`heap.rs:109-113`). The kernel quota default is 16 MiB (`kernel/src/memory/cell_quota.rs:26-43`).
- **Current binary.** The S7-2 private-lane build (`/tmp/cellbox-phase02-39EYAx/.../release/cellbox`, 2026-10-24 07:49, `size`) is file **1,535,720** B, text **1,330,508**, data 27,720, bss 1,048,824.
- **S7-1 signal.** A throwaway LTO staticlib (`opt-level="s"`, walker + searcher + `RegexMatcher`, x86_64-unknown-none) measured text 451,000 + data 114,352 B (admission §10).
- **Shared code.** Cellbox already links `regex-automata [meta, hybrid]` and `regex-syntax` through text-engine (`libs/text-engine/Cargo.toml:16-17`), without Unicode tables. Probe A2 put the Unicode-table share at ≈ 360 KB (dossier §3.4).

## 2. The pull/push gap (new finding; D76-1)

The engine is pull-based: one `next_item` per row, with the spec edge budget of 8 records / 256 KiB. The searcher is push-based and cannot pause (§1.4). For a single file, the number of rows `search_file` pushes has no bound: a 24 MiB file whose every line matches yields millions of rows inside one call. Buffering them breaks the 1 MiB arena long before the spec's edge budget is reached. Neither D74-1 nor dossier §7 addressed this, and S7-3 cannot be built without a choice.

| Option | What | Verdict |
|---|---|---|
| **P1 Segmented restart (recommended)** | A new Cellos-owned resumable `FileSearch` in `vigrep::walk`. A row sink that wants to pause **accepts** the row and returns `Ok(false)`; that searcher run is discarded. The next step starts a fresh `search_reader` at a line boundary chosen so that every later row is exactly what one uninterrupted run would emit. Upstream searcher code is untouched | Exact by construction (proof below) and testable against the existing differential corpus with a forced pause after every row. Costs one bounded re-scan per pause |
| P2 Re-entrant core | Patch upstream `core.rs`/`glue.rs` with a tri-state sink result that commits state and returns; re-enter with the same buffer. Needs an owned (non-borrowing) `ReadByLine` | Edits ~10 upstream call sites (`core.rs:274-421, 530-652`); the slow path advances `pos` before the sink (`:365`) and would need reordering. Port drift in adapted code |
| P3 Unbounded per-file queue | Collect each file's rows, then drain | Rejected: unbounded heap; the `OOM:` exit kills the shell |

**P1 resume rule.** The resume state is:
- E = the last emitted line;
- M = the last emitted match;
- A, B = the after/before context counts;
- the start offsets of the last max(B, 1) emitted lines, plus M's offset.

The next segment starts at line L0 = min(M if E < M + A, E + 1 − B), clamped to 1. With no context, L0 = E + 1 at the end offset of E. The new searcher's line numbers and offsets are shifted by (L0 − 1, offset(L0)), and rows with line ≤ E are suppressed.

**Why it is exact:**
- For any line x > E, the original decision (match, before-context, after-context or nothing) depends only on matches in [x − A, x + B] and on the most recent match before x.
- Matches > E are seen by the new run.
- If x is after-context of a match m ≤ E, then m > E − A, so E < M + A and L0 ≤ M. The most recent match before x is therefore the same in both runs.
- Before-context lines of a match m > E are ≥ E + 1 − B ≥ L0.
- Lines E + 1 − B … E were all emitted, so their offsets are known: E is either a match whose B before-lines were emitted, or after-context contiguous with such a match.
- Multi-line search is excluded and L0 is always a line start. A restart is therefore equivalent to upstream's own buffer roll at a line boundary.
- `\A`-style haystack anchors are the one edge. They are asserted by the forced-pause differential run (§7.1).

**Read cost.** A restart re-scans from L0. For VFS files, bytes are re-read with `read_chunk`; a resume window of ≤ 96 KiB serves them from memory when L0 is still inside it. In-memory stdin is free. A **streamed** stdin (external head) cannot rewind: a resume that needs bytes older than the window fails that input with error 2, never with a skewed result (D76-10).

**Pause cadence.** The row queue pauses at 8 rows or 32 KiB of queued payload, whichever comes first, and always admits at least one row (D76-3). Sparse matches cause no pauses. `-q` queues nothing and never pauses.

## 3. Design

### 3.1 Files and APIs

| File | Change |
|---|---|
| `libs/text-engine/src/pipeline.rs` | (a) `RowSource::completion(&self) -> Option<i32> { None }` (default). (b) `Upstream` remembers `completion()` of its source **after** calling `close()` and before dropping it; `Upstream::completion()` returns the live or remembered value; OrderGuard, Select, Where, Take, Sort and SortFrame forward it. (c) `pub struct InterruptLatch` (§3.3). (d) If D76-5 = S1: `pub fn typed_json_len(&Value) -> Result<usize, PipelineError>`, allocation-free and equal to `value_to_json(v, true)?.len()` |
| `libs/text-engine/src/lib.rs` | delete `pub mod search;` (`:46`) |
| `libs/text-engine/src/search.rs` | **delete** (D70-6; its 3 tests go with it) |
| `libs/vigrep/src/walk.rs` (+ `walk/tests.rs`) | **Only with D76-2.** `Walker::begin_file(tree, path, cancel) -> Result<Begun<T::File>, SearchFileError<Infallible>>` (open, preflight in Skip mode, state check; `Begun::{Binary, Search(FileSearch<F>)}`). `FileSearch::step(&mut self, walker, tree, searcher, matcher, &mut sink, cancel) -> Result<Step, SearchFileError<S::Error>>` with `Step::{Paused, Finished}` (§2 resume rule; final state check on `Finished`). `FileSearch::abandon(self, tree)` releases the handle. `search_file` is re-expressed as `begin_file` + a `step` loop, so every S7-1 walker test and the differential corpus keep covering it. `pub fn line_submatches(matcher, &SinkMatch, max) -> Result<Vec<(usize, usize)>, SubmatchError>`, moved from the differential test (`tests/differential/main.rs:425-452`) and capped at 4,096. `WalkLimits::max_pending_entries` (default 65,536). Pending children stored packed (`kind, len, name`) so the name limit measures real memory (D76-4) |
| `libs/vigrep/src/regex/config.rs` | **Only with D76-2.** The onepass limit follows `size_limit` instead of the fixed 10 MiB (`:257`). Recorded in `PROVENANCE` as a Cellos adaptation |
| `libs/vigrep/tests/differential/main.rs` | Uses `line_submatches`. Adds a forced-pause run (queue cap 1 and 3) that must reproduce every committed record |
| **new** `cells/tools/cellbox/src/rg.rs` | Module root. `pub fn producer(args: &RgArgs, latch: &Rc<InterruptLatch>) -> Result<Box<dyn RowSource>, i32>` (tree mode). `pub fn stdin_producer(args, input: StdinInput, latch) -> …` (`StdinInput::{Bytes(Vec<u8>), Stream(Box<dyn ByteSource>)}`). `RG_PROFILE` limits (§6) |
| **new** `rg/args.rs` | Pure parser. `pub fn parse(argv: &[String]) -> Result<RgArgs, Usage>`, where `RgArgs { patterns, paths, stdin, fixed, ignore_case, word, line, quiet, before, after, globs, hidden, no_ignore, search_binary, text_display, help }` (§3.2) |
| **new** `rg/tree.rs` | `VfsTree: ViSearchTree` over `VfsClient` (§3.4) |
| **new** `rg/source.rs` | `RgSource<T: ViSearchTree>: RowSource` (§3.5): matcher/searcher/walker construction, row sink, queue, diagnostics, summary, completion |
| **new** `rg/text.rs` | `escape_path`, `escape_line`, `render_row(&Value, &mut Vec<u8>) -> Result<(), PipelineError>` (§3.6) |
| **new** `rg/tests.rs` | Host tests (§7.1) |
| `cells/tools/cellbox/src/typed.rs` | `classify_stage`: `"rg"` → `rg::args::parse`; `Ok(stdin)` → `Class::SearchStdin`, `Ok(_)` → `Class::Producer`, `Err` → `Class::Bad`. `might_be_typed("rg", _)` is always true. Byte heads accept `SearchStdin` wherever they accept `From`. `text_display` comes from the parsed flag for `rg` (no argv stripping). `DefaultSink::SearchText` (§3.6) in `stream_rows`, `run_rows` and `RowFeed::new` (new `default_sink` parameter; non-rg producers keep typed JSONL there). `Tap` forwards `completion`. The rg Collected bound (D76-6). `ChunkSink::interrupted` reads the job latch |
| `cells/tools/cellbox/src/executor.rs` | One `InterruptLatch` per typed job, fed by `poll_session_interrupt` (`:1315`). `TerminalSink` and `ExtRun::poll_interrupt` (`:3508`) use it instead of private flags. The latch is passed to producers. Byte heads feed `rg -` (§3.7). Status mapping (§3.8). `BUILTINS` gains `"rg"` |
| `cells/tools/cellbox/src/native_rows.rs` | `row_producer(argv, latch)`: the `"rg"` arm delegates to `rg::producer` (no `--records` gate); `ps`/`ls`/`top` ignore the latch. `producer_category("rg")` = Rows |
| `cells/tools/cellbox/src/ledger.rs` | `e(Builtin, "rg", InSuite)`; `counts_match_the_s2_gate` 56 → 57 |
| `cells/tools/cellbox/src/commands.rs` | Files help line lists `rg` next to `grep` |
| `cells/tools/cellbox/Cargo.toml`, `Cargo.lock` | `vigrep = { path = "../../../libs/vigrep" }`. The lock gains only the `app-cellbox → vigrep` edge; no new crate (bstr 1.13.1 came in with S7-1) |
| Docs | `docs/cellbox-cli.md:78` → `rg "timeout" /data/logs \| select path line_number text \| to jsonl --typed` (D74-6), plus the search section (renderer, `--text` vs `-a`, absolute paths, statuses). `CHANGELOG.md` (Unreleased). `command-catalog.md`: the new-command row `:276` gets dispatch facts, and the ledger block is regenerated. `phase-07-ripgrep-search.md` deviation log |

`main.rs` `declare_syscalls!`, the cellbox launch profile, VFS, `api` and `ostd` are unchanged. No new right, syscall or IPC variant is added. G-S1a must still PASS.

### 3.2 `rg` argv (`rg/args.rs`)
- **Scan order.**
  - Words are scanned left to right until `--` or the first operand.
  - A bare `-` is an operand, never an option.
  - Any other word starting with `-` before `--` must be an exact supported spelling; anything else gives `rg: unsupported option: WORD` and status 2.
  - Value options consume exactly the next word, which may start with `-` (so `-e --text` is a pattern).
- **Repetition.** `-e` and `-g` are repeatable. Each scalar flag (`-F -i -w -x -n -q --hidden --no-ignore -a --text --encoding -A -B -C`) at most once. These are rejected: combined shorts (`-iw`), `--name=value`, upstream abbreviations, and the explicit-reject list of `:41` (each named in the message).
- **Conflicts.** `-w` with `-x`; `-q` with any of `-A/-B/-C`; `-C` with `-A`/`-B`; N not decimal or > 20; `--encoding` value other than `none`.
- **Operands.**
  - Without `-e`, the first operand is the pattern; with `-e`, all operands are paths.
  - Limits: ≤ 16 paths, ≤ 32 patterns, ≤ 4,096 aggregate pattern bytes, ≤ 32 globs, ≤ 4,096 glob bytes.
  - Empty path, or NUL in any word, rejects. An empty pattern is allowed.
  - `-` must be the only path and requires `-a`, so the stdin form is rejected before any input is read (`:50`).
- **Argv size.** Encoded argv `Σ(len + 1)` must be ≤ 512 including `rg` (D76-13).
- **Help.** A sole `-h`/`--help` sets `help`. `producer` then prints the usage, which states that `--text` is not `-a`, and returns an empty source with completion 0. `-h` together with anything else is status 2.
- **When.** Parsing runs at **admission**, so every argv error is status 2 before any effect, including the start of external stages (`executor.rs:1508-1515`).
- **Construction.** Regex and glob compilation happen at producer construction. That is still before any VFS request, but after tail externals were started, which is the same as `ps/ls/top` usage errors today (`native_rows.rs:37-55`). Compile errors (syntax, backreference, look-around, literal `\n`, size limit) are status 2 with `rg: regex: …` / `rg: glob: …`.
- **Matcher.** `RegexMatcherBuilder` gets:
  - `line_terminator(Some(b'\n'))`, `case_insensitive(-i)`, `word(-w)`, `whole_line(-x)`, `fixed_strings(-F)`;
  - `size_limit` and `dfa_size_limit` from §6;
  - `build_many(patterns)` in argument order (leftmost-first; argument order breaks ties).
  - No smart case.
- **Searcher.** `SearcherBuilder` gets `line_number(true)`, `before_context(B)`, `after_context(A)`, `buffer_capacity(16 KiB)` and `heap_limit(Some(16 KiB + 65,536))`.
- **Walker.** `WalkBuilder` gets:
  - `hidden(--hidden)` and `ignore_files(!--no-ignore)`;
  - `binary(Search if -a else Skip)`;
  - `overrides(OverrideBuilder::new(cwd) + -g globs)`;
  - `limits(RG_PROFILE)`.
- **Roots.**
  - Each path goes through `resolve_shell_path`.
  - With no path, the root is `get_shell_cwd()`. If the cwd cannot be read, rg fails with status 2; the `/` fallback (`cmd_fs.rs:202`) is never used for a search root.
  - A resolved path over 1,024 bytes is a per-root error, which the walker already reports.

### 3.3 Interrupt latch (C1, D74-1) and where checks happen
```rust
pub struct InterruptLatch { set: Cell<bool>, checks: Cell<u32>, poll: RefCell<Box<dyn FnMut() -> bool>> }
impl InterruptLatch {
    pub fn new(poll: Box<dyn FnMut() -> bool>) -> Rc<Self>;
    pub fn inert() -> Rc<Self>;     // never polls (no session; tests)
    pub fn is_set(&self) -> bool;   // sticky, no poll
    pub fn poll_now(&self) -> bool; // sinks and ExtRun: poll at today's cadence
    pub fn check(&self) -> bool;    // producers: poll on every 16th call (D76-7)
}
```
- **Ownership.** `run_typed` creates one latch per typed job with `poll = poll_session_interrupt(&mut enabled)`. The same `Rc` goes to the `TerminalSink`, to every `ExtRun` of the job (head and tail) and to the producer.
- **Why one shared latch.** Today the three pollers each discard every event they do not recognise (`executor.rs:1312-1340`). Without one shared latch, a Ctrl-C consumed by the producer's poll would be lost to `ExtRun`. `rg x | /bin/cat` would then finish with `cat`'s 0 instead of 130.
- **Existing cadence unchanged.** Sinks still poll before every pull and during backpressure; `ExtRun` still polls every 16 steps.
- **Checks inside a long search.** `RgSource` passes `&mut || latch.check()` as vigrep's cancel callback. It is therefore checked:
  1. at the top of every `Walker::next` iteration, i.e. per visited entry, ignored ones included (`walk.rs:532`);
  2. per listing page (`walk.rs:875`);
  3. per ignore-file chunk (`walk.rs:921`);
  4. per preflight chunk (`walk.rs:659`);
  5. per searcher read through `TreeReader::read` (`walk.rs:1001`) and its `FileSearch` successor;
  6. at the start of each file, and at the start of each `RgSource::next_item`;
  7. in stdin mode, per reader fill.
- **Poll cadence.** One real session poll per 16 checks: about 64 KB of reads, or 16 entries. This matches `ExtRun`'s 16-step precedent (`executor.rs:3519`), and the Ctrl-C latency stays a few chunk round trips. Each check in between is a `Cell` read.
- **On interrupt.**
  - `RgSource` abandons any open `FileSearch` (closing the file handle and then the cached parent), drops the walker and queue, sets its completion to `None`, and returns `Ok(None)`.
  - Drivers re-check `latch.is_set()` after the stream ends: Streamed prints `^C` and returns 130. Collected also returns 130 and writes no redirect, which matches the external-head path at `executor.rs:1635-1638`.

### 3.4 `VfsTree` (`rg/tree.rs`; H1, D74-5)
```rust
pub struct VfsTree { client: VfsClient, parent: Option<(Vec<u8>, ViDirHandle)>, page: Vec<u8> /* VFS_DIR_PAGE_MAX */, open: HandleCount }
type Dir  = VfsDir  { handle: ViDirHandle, cursor: u32 };
type File = VfsFile { handle: ViVfsFileHandle };
```
- **`open_dir(path)`.** Calls `open_root_dir(path)`. On `PermissionDenied` it reports that. On `IO` it calls `stat(path)` to classify: a file gives `NotADirectory`; anything else gives `Other("not found or I/O error")`, because VFS answers ERR_IO for both (§1.5). On `OutOfMemory` it reports "VFS handle quota".
- **`next_page`.** Calls `list_page(handle, cursor, &mut page)`. Entries map to `DirEntry{kind, name}` with File/Dir/Mount/Other → the same vigrep kinds. It sets `cursor = next` and returns `{complete, changed}`.
- **`open_file(path)`.**
  - Splits off the last component. The parent handle is reused if it is the cached one; otherwise the cache is evicted (`close_dir`) and `open_root_dir(parent)` is opened.
  - Then `open_file_at(parent, name)`.
  - D76-14 recommends this one-entry parent cache: consecutive siblings cost 2 IPCs instead of 4.
  - Because of `CloseDir` revocation (`ipc.rs:151-158`), the cached parent is never closed while a file derived from it is open. `close(File)` runs before any eviction.
- **`read_chunk` / `file_state` / `close`.** Pass through. `NotFound` on a held handle maps to "handle revoked".
- **Handle peak.** One listing directory + one cached parent + one file, i.e. ≤ 2 of the 32 directory handles and ≤ 1 of the 32 file handles. The file handle is the only one held **between pulls**, during a paused `FileSearch`. That is required so that `modified` (DV7-4) covers the whole search. `RgSource::close` and `Drop` release file, then parent, then any listing directory. Host tests count this through the fake tree; S7-4 proves it at runtime.
- **No fallback.** There is no BootFS fallback (unlike `cat`) and no grants or `SealPaths`. Authority is exactly the per-Cell VFS table that `ls`/`cat`/`grep` use (DV7-2).

### 3.5 `RgSource` (`rg/source.rs`)
```
next_item():
  loop
    queue.pop_front() → Some(row) => return Row
    latch.check() → interrupted path (§3.3)
    match phase
      Done                 → return Ok(None)
      InFile(fs, path)     → fs.step(…, &mut RowSink) → Paused: continue
                             Finished(Searched) | (Binary in begin_file) → stats; phase = Walking
                             Err(Walk(e)) | Err(Sink(limit)) → diagnostic; errors += 1; phase = Walking
                             Err(Cancelled) → interrupted path
      Walking              → walker.next(tree, cancel):
                             File{path}      → begin_file → Binary: count | Search(fs): InFile
                             MountSkipped    → mounts_skipped += 1 (no line; summary)
                             Error(e)        → diagnostic; continue
                             Cancelled       → interrupted path
                             None            → summary if errors + mounts > 0; completion = status(); Done
```
- **`RowSink` (vigrep `Sink`).** Builds one `Value::Record` per match/context in the §1.1 field order:
  - `path` is `Value::Path(canonical bytes)`, or `Null` for stdin;
  - `line_bytes` is the line without its LF, CR kept;
  - `text` is `String` only when `core::str::from_utf8` succeeds, otherwise `Null`;
  - `terminator` is `b"\n"` or empty;
  - `submatches` come from `vigrep::walk::line_submatches`; more than 4,096 is a limit error for the file.
  - Before queuing, it checks the record bound (D76-5). It increments `matching_lines` and `matches`. When the queue reaches the cap it returns `Ok(false)` (pause after this row).
  - Under `-q` it counts and never queues.
- **Status.** 2 if any error or limit was reported. Otherwise 0 if `matching_lines > 0`, else 1. Mount skips do not affect it (DV7-7).
- **Close.** `close()` before `Done` is the `take` short circuit: release everything, then completion = 2 if an error had already been reported, else 0. It is idempotent, and `next_item` afterwards returns `Ok(None)`.
- **Diagnostics** (D74-4). The format is `rg: ESCAPED_PATH: reason` on stderr through `shell_stderrln`, one per failed entry, file or ignore file, emitted as it happens. Rows already delivered stay delivered. Streamed rows are flushed per row (`typed.rs:879`), so terminal order is preserved. Reasons are the `WalkErrorKind` Display texts (`walk.rs:264-299`) and the §3.4 mappings.
- **End summary.** One line when `errors + mounts_skipped > 0`, as key=value pairs:
  - `files_scanned`, `files_binary_skipped`, `links_skipped=0` (DV7-5), `mounts_skipped`;
  - `lines_scanned` (LFs counted in newly read search bytes plus a final unterminated line, with a high-water mark so restarts do not double-count);
  - `matching_lines`, `matches`, `bytes_read`, `errors`;
  - `complete=false`, `reason`.
  - `RgSource::summary()` returns the same struct, so tests assert counters, not text.
- **Stdin mode.** One `FileSearch`-equivalent over a reader:
  - `Bytes`: an in-memory slice; restarts are free.
  - `Stream`: a `ByteSource` behind the resume window (§2, D76-10).
  - Binary mode is always Search (`-a` is required). There is no modified check, because the bytes have no VFS file.

### 3.6 Streaming text renderer (D74-3, DV7-9; `rg/text.rs`)
- **`render_row`** writes `ESCAPED_PATH (':' | '-') LINE (':' | '-') ESCAPED_LINE '\n'`, with `:` for a match and `-` for context. A Null path renders as `<stdin>`. LINE is decimal `line_number`. The escaped line comes from `line_bytes`, not `text`.
- **Escapes.**
  - `\` → `\\`; LF → `\n`; CR → `\r`; tab → `\t`; NUL → `\0`.
  - Other C0, DEL and C1 (Cc) controls, and every byte of an invalid UTF-8 sequence → `\xHH` (uppercase).
  - In paths, `:` and `-` → `\x3A`/`\x2D`.
  - Printable valid UTF-8 is kept.
  - D76-11 also proposes escaping the bidi/format controls U+061C, U+200E/F, U+202A–U+202E and U+2066–U+2069 per byte.
  - The output is ASCII plus valid UTF-8, so `Delivery::flush`'s lossy conversion (`typed.rs:841`) is a no-op.
- **`DefaultSink::SearchText`** applies when the head is `rg` and the typed segment after it contains only schema-preserving stages (`where`, `take`, `sort-by`) and no explicit `to` (D76-9):
  - at the terminal (replacing Table);
  - with `--text` at redirect, capture and byte-consumer sinks;
  - with `--text` for external tails through `RowFeed`.

  With `select` present the old defaults apply (Table at the terminal, typed JSONL elsewhere). Explicit serializers always win. A row that is not a `cellbox.search.line` record (impossible under the static rule) is a stage error, not a guess.
- **Streaming.** Each row is rendered and flushed before the next pull. Nothing is buffered, unlike the Rows table (`typed.rs:902-905`).
- **Paths are absolute** canonical VFS paths, the same bytes as the record (D76-12). Upstream prints operand-relative paths; this is listed as an intentional difference in help and docs.

### 3.7 Admission and heads
| Pipeline | Head | Notes |
|---|---|---|
| `rg P [PATH…]` alone or followed by row stages / `to` / externals | `Head::Producer` (`rg` is always typed) | `producer_argv` is passed unchanged; `text_display` comes from the parser |
| `cat F \| rg -a P -` | `Head::Cat` → rg stdin (`Bytes`) | `cat` bound 1 MiB, as today |
| `rg -a P - < F` | `Head::FromFile` → rg stdin (`Bytes`) | |
| `echo … \| rg -a P -` (other builtin) | `Head::Capture` → rg stdin (`Bytes`) | |
| `/bin/x \| rg -a P -` | `Head::External` → rg stdin (`Stream` over `ChildBytes`) | D76-10; resume window ≤ 96 KiB |
| `rg P -` without `-a`; `ps --records \| rg …`; `rg … \| from json`; `rg … &` | rejected at admission, status 2 | records into rg are refused before effects; background typed stages are refused today (`typed.rs:630`) |

### 3.8 Completion status (D74-2) and reporting
- **Driver.** After the chain is closed, `run_typed` reads `chain.completion()` (and `RowFeed`'s chain completion for external tails) and stores it as member 0's status. `jobs` shows it (`executor.rs:2047-2068`).
- **Pipeline status:**
  1. Latch set → 130 (`^C`).
  2. Otherwise a typed failure → today's mapping (`shell: STAGE: msg`, status 1). The rg Collected-bound failure is status 2 (D76-6).
  3. Otherwise, if no row stage follows `rg` (only an optional explicit `to`), the status is rg's completion (D76-8).
  4. Otherwise the final stage's status (0 for typed stages, the external's exit for tails). A nonzero rg member status is reported through the existing `earlier_failures` rule (`typed.rs:1198-1210`, D76-15).
- **`take` short circuit.** The rg member gets 0 (or 2 after an error); the pipeline gets take's 0.

### 3.9 Deletion of `text_engine::search` (D70-6)
- Delete `libs/text-engine/src/search.rs` and `pub mod search;` (`lib.rs:46`). There is no consumer (§1.3) and nothing to migrate.
- `cellbox.search.line` is produced only by `rg/source.rs`.
- Size impact is nil: the module has no caller, so it was never linked.
- The text-engine host count drops by its 3 tests.

## 4. Error and denied rendering

| Condition | Rendering | rg status |
|---|---|---|
| argv/usage error, unsupported flag, `-` without `-a`, records input | `rg: …` usage line on stderr, no VFS request, no input consumed | 2 |
| regex/glob compile error, size limit, look-around, backreference, literal `\n` | `rg: regex: …` / `rg: glob: …`, before any VFS request | 2 |
| cwd unreadable with no path | one diagnostic; never `/` | 2 |
| root or child directory denied (`ERR_DENIED`, e.g. `/srv/cellos`) | `rg: PATH: permission denied`; walk continues | 2 |
| missing or unreadable root (`ERR_IO`) | `rg: PATH: not found or I/O error` | 2 |
| ignore file unreadable, malformed or over a limit | per-ignore-file line; that directory is skipped (S7-1 semantics) | 2 |
| listing `CHANGED`, file `modified` (DV7-4) | per-entry line; rows already given for that file stay delivered and are covered by status 2 | 2 |
| handle quota (`ERR_QUOTA`), revoked handle | per-entry line | 2 |
| line > 65,536 B (searcher heap limit), > 4,096 submatches, record over bound (D76-5) | per-file line; continue | 2 |
| visited-entry limit | line; walk ends (`walk.rs:551-554`) | 2 |
| `Other` kind | per-entry line (DV7-5) | 2 |
| mount child | none per entry; counted in the summary (DV7-7) | unchanged |
| binary file (Skip mode) | none; counted | unchanged (all-binary → 1) |
| `take` close | none | 0, or 2 after an error |
| Ctrl-C | `^C` | 130 |
| rg Collected output over the bound (D76-6) | `shell: rg: output exceeds the N-byte collect bound`; no redirect written | 2 |

## 5. Security
- **Authority.** Read authority is the existing per-Cell VFS table. R is re-checked by VFS on every page, open, chunk and state request (S7-2 §14.1). Cellbox gains no right, grant, seal or syscall; G-S1a is unchanged.
- **No widening.**
  - There is no BootFS fallback.
  - The `/` cwd fallback is never used for a search root.
  - Explicit roots go through `resolve_shell_path`, and VFS validates them again (`dispatch_dirs.rs:125-130`).
- **Visibility.** Names under a denied directory are never read. A denied directory's name is visible only where its parent listing already shows it, as with `ls`.
- **Terminal injection.** Every displayed byte from file content, path or diagnostic goes through §3.6 escaping. Typed JSONL is unchanged and lossless.
- **Resource exhaustion.** The §6 profile bounds every allocation that input can drive. Exceeding a bound is error 2, never an `OOM:` exit.
- **Disclosure.** Output can disclose file contents to whoever reads the terminal or redirect. That is the same exposure as `cat`. The R check precedes every read.

## 6. Bounds (1 MiB arena; D76-3, D76-4, DV7-10)
Contract numbers are ceilings ("at most", `command-contracts.md:11`). The `RG_PROFILE` below tightens the ones that cannot fit the 1 MiB arena next to the shell. **Target: peak rg-attributable heap ≤ 512 KiB**, the same headroom rule as `MAX_TABLE_HEAP_BYTES` (`typed.rs:46-49`), measured in host tests (§7.1).

| Item | Contract / vigrep default | `RG_PROFILE` | Worst-case bytes |
|---|---|---|---|
| Regex NFA (`size_limit`) | ≤ 2 MiB total / 100 MiB | 128 KiB | 128 KiB |
| Lazy-DFA cache (`dfa_size_limit`) | (in 2 MiB) / 1000 MiB | 64 KiB (a too-small cache disables the hybrid engine; PikeVM fallback, slower) [INFERENCE: verify in the heap test] | 64 KiB |
| Onepass DFA | — / fixed 10 MiB | = `size_limit` (D76-2) | ≤ 128 KiB, only for anchored searches [INFERENCE] |
| Searcher buffer | 16 KiB scratch / 64 KiB default | capacity 16 KiB, `heap_limit` 16 KiB + 65,536 | ≤ 80 KiB |
| Preflight scratch | 16 KiB | 16 KiB | 16 KiB |
| Listing page buffer | — | `VFS_DIR_PAGE_MAX` | 4 KiB |
| Entries per directory | 4,096 (DV7-6) | 4,096 | packed (D76-4) |
| Pending children across the stack | 1 MiB names (DV7-6) | 96 KiB **packed bytes** (2-byte header + name) and ≤ 8,192 entries | 96 KiB |
| Visited entries, depth, path | 65,536 / 64 / 1,024 | unchanged | counters |
| Ignore file | 65,536 B / 1,024 rules | 16 KiB / 256 rules | — |
| Retained ignore bytes | 256 KiB | 32 KiB (+ compiled globsets, measured) | 32 KiB + globs |
| Row queue | 8 rows / 256 KiB per edge (spec) | 8 rows / 32 KiB payload, ≥ 1 row | ≤ 32 KiB + one row |
| One row (`Value`) | record ≤ 65,536 B serialized | D76-5 | ≤ ~66 KiB |
| One encoded row in `Delivery` | 65,536 B | unchanged | 64 KiB |
| Resume window (streamed stdin; VFS re-reads otherwise) | — | ≤ 96 KiB | 96 KiB (stdin mode has no walker state) |
| Collected output (`> f`, `$(…)`, byte consumers) | 4 MiB (`typed.rs:43`) | rg-headed: 256 KiB (D76-6) | 256 KiB |

- **Sum.** Tree mode ≈ 128 + 64 + 80 + 16 + 4 + 96 + 32 + 98 + 64 KiB ≈ 582 KiB worst case, before globsets and engine caches. That **exceeds** the target if every maximum coincides. Typical use is far below it.
- **What S7-3 measures.** The worst case at each bound (32 Unicode `-i` patterns, a 4,096-entry directory, maximal ignore files, 64 KiB lines, dense matches). If the measured peak is over 512 KiB, the profile is lowered further (recorded for CP2), or the user picks M2 (D76-3).
- **Units.** Values are per walk. Nothing is retained after a file except the walker stack and the matcher.
- **Pre-existing, not changed.** `sort-by` buffers up to 4,096 rows / 4 MiB (`pipeline.rs:22-24`). `rg … | sort-by …` on large output can still exhaust the arena, as `ps`/`ls` can. This is recorded as a risk, not fixed here.

## 7. Verification (behavioral only)

### 7.1 Host
- **`cargo test -p text-engine --target x86_64-unknown-linux-gnu`.**
  - Latch: sticky; `check` polls exactly once per 16 calls; `poll_now` polls every time; `inert` never polls.
  - Completion forwarding: for every stage kind built by `apply_row_stage` (select, where, take, take 0, sort-by, sort-by --frame over Frames) plus OrderGuard, `chain.completion()` equals the fake producer's value after EOF, after an error and after an early close. The producer's `close` runs before its completion is read.
  - `typed_json_len == value_to_json(v, true)?.len()` over a matrix of Bytes/Path/String with escapes, nested lists and Null (D76-5).
  - The existing count minus search.rs's 3.
- **`cargo test -p vigrep --target x86_64-unknown-linux-gnu`** (only with D76-2).
  - All S7-1 tests unchanged.
  - Differential corpus re-run through `FileSearch` with the queue cap forced to 1 and to 3: all 172 records identical across 20 cases, including `-A/-B/-C` overlaps, zero-width matches, the 300 KB multi-buffer file, final no-LF, CRLF and the `\b` byte regex. The `\A` edge is asserted against the uninterrupted run.
  - Pause right before EOF; pause on an after-context row; pause with B = 0 and with B = 20.
  - `modified` set between two steps gives `FileModified`. `abandon` releases the handle (fake count 0).
  - The resume window: a hit serves from memory (no fake read); a miss on a seekable source re-reads; a miss on a stream is an error.
  - `line_submatches` at the 4,096 cap gives an error.
  - Packed pending storage keeps byte order and every walker limit test.
- **`cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`** (`rg/tests.rs`, over an in-memory `ViSearchTree` that counts handles and reads):
  - **Argv:** every supported spelling; each invalid combination and each `:41` flag gives status 2 with **zero** tree calls and no input consumed; `-h`/`--help` alone gives 0; `-e --text` and `-- --text` keep `--text` as pattern/path; the limits at N and N + 1; empty path and NUL rejected.
  - **Records:** exact field order and types. Path is Path. `line_bytes` keeps CR and `text` is Null for invalid UTF-8. The final line has an empty terminator. Offsets cover a BOM and multi-chunk files > 64 KiB (several 4,000-byte reads). Submatches are ordered and non-overlapping; context rows have none.
  - **Status:** 0, 1 (empty, all-binary), 2. `-q` with a later error gives 2. A denied subdirectory gives a diagnostic, the walk continues and the status is 2. A mount skip is counted with status unaffected. `Other` kind gives 2. A record over the bound gives a per-file 2 and the walk continues.
  - **Close and cancel:** `take 1` closes with handle count back to 0 and no further reads; an interrupt latched mid-file gives `Ok(None)`, no further reads, handles 0, completion None.
  - **Queue:** never more than 8 rows or 32 KiB plus one row.
  - **Heap:** a test-only counting `#[global_allocator]` measures peak bytes per §6 case (target ≤ 512 KiB). [INFERENCE: ostd defines no host global allocator; verify at implementation.]
  - **Text renderer:** escaping matrix (backslash, LF/CR/tab/NUL, other C0, DEL, C1, invalid bytes, `:`/`-` in paths, `<stdin>`, context separator, D76-11 bidi set); no ESC byte in any output.
  - **Admission:** `rg x` gives `Head::Producer` plus SearchText at the terminal. `rg x | select …` gives Table. `cat f | rg -a x -` gives head Cat with the rg stage. The §3.7 rejections. `text_display` comes from the parser.
  - **Status mapping:** sole stage, `| to jsonl --typed`, `| take 1`, `| where …`; Collected bound exceeded gives 2.
  - **Ledger:** builtins = executor `BUILTINS`; count 57; catalog block regenerated.
- **Bare metal.** `app-cellbox` and `app-cellbox --features shell_test` for `x86_64-unknown-none` (release, PIC, `-Z build-std=core,alloc`); `vigrep` for both x86 profiles. `scripts/cellbox-ceiling-diff.py` **G-S1a PASS**.
- **`cargo deny check licenses advisories`.** No new crate. The pre-existing `ocel-pdf` AGPL finding is out of scope.

### 7.2 Runtime: split between S7-3 and S7-4 (D76-16)
- **S7-3 (fixture-free).** New terminal-runner checks (`scripts/qemu-phase04-terminal-test.sh` plus `cells/tests/phase04-terminal-driver`). Each check asserts structure and status, not wording.

  | Check | Asserts |
  |---|---|
  | `rg-help` | `rg -h` → `$?` 0, non-empty output |
  | `rg-unsupported-before-reads` | `rg -P x /tmp`, `rg -iw x`, `rg -q -C1 x` → 2 each |
  | `rg-stdin-requires-a` | `echo hi \| rg hi -` → 2 |
  | `rg-stdin-match` | `echo 'a:b' \| rg -a b -` → line `<stdin>:1:a:b`, `$?` 0; `rg -a zz -` → 1 |
  | `rg-tmp-tree` | tree built with `mkdir`/`echo >` under `/tmp/rgt` (RamFS), including `.h` and `.ignore`: `rg foo /tmp/rgt` rows have absolute paths; the hidden and ignored files are absent; `--hidden --no-ignore` includes them; `$?` 0 |
  | `rg-typed-roundtrip` | `rg foo /tmp/rgt > /tmp/r.jsonl; from jsonl --typed < /tmp/r.jsonl \| select line_number` → the same line numbers |
  | `rg-escape` | a file with ESC/CR bytes (`printf` if available, else a `vwrite` fixture) → output shows `\x1B`/`\r` and the device stream carries no raw ESC from the content |

  This exercises `VfsTree` on target over S7-2's `list_page`/`open_file_at`/`read_chunk`/`file_state`, the target-built regex and the renderer.
- **S7-3 regression.** Terminal 430 + 7 = **437** [planned], GUI 6 × 279, P03 62, P02 48, kernel AP 39/39 with the 3 dir-page markers. All are needed because typed.rs/executor.rs drivers, Ctrl-C polling and default sinks change for every typed pipeline.
- **S7-4 (fixture-dependent, unchanged from dossier §9).**
  - Private FAT `/data/rg/` tree by `scripts/phase07_rg_fixture.py`: a file larger than the quota with exact offsets, a directory larger than one page, competing ignores, late-NUL binary vs `-a`, CRLF/BOM/no-LF/invalid UTF-8, a denied subtree.
  - P03: `rg | take 1` stops traversal and frees handles; `| where`; JSONL round trip; `cat … | rg -a x -`.
  - P02: Ctrl-C during a long no-match walk → 130 and a prompt.
  - Elapsed time recorded.

### 7.3 Size
- **Baseline** (§1.6): text 1,330,508 / data 27,720 / file 1,535,720.
- **Estimate** [INFERENCE]: +300–550 KB text and +50–115 KB data. The S7-1 LTO probe (451,000 + 114,352 B) is the upper bound; cellbox already links the regex-automata meta/hybrid/NFA core through text-engine, and the Unicode tables (≈ 360 KB probe-level) are new.
- bss is unchanged under M1. M2 would add 1 MiB of bss.
- The `search.rs` deletion has no size effect.
- `size` is recorded before and after with no budget (D76-17). No loader ELF limit exists (Phase 06 §1.5).

## 8. Risks and rollback
- **Restart correctness (P1).** The proof in §2 assumes line-oriented search without multi-line mode, which rg never enables (`-U` is rejected). The forced-pause differential run is the guard. Any record difference blocks the slice.
- **Read amplification.** Dense matches mean one re-scan per 8 rows, ≤ 16 KiB of CPU re-scan and usually no re-read thanks to the window. The TCG time is measured in S7-4. If it is material, the queue cap or the window is tuned and the change recorded.
- **Heap.**
  - The §6 worst-case sum is above target until it is measured. M2 is the fallback.
  - `sort-by` on large rg output can still exhaust the arena (pre-existing).
  - An `OOM:` exit kills the shell, so the heap test is mandatory, not advisory.
- **Unicode cost.** A small DFA cache can push Unicode `-i` patterns onto the PikeVM (slow on TCG). This is correct, and recorded.
- **Type-ahead.** Producer polling scrubs keystrokes typed during a long `rg … > f` (Collected). Today that happens only in streamed pipelines.
- **ERR_IO ambiguity.** "Not found" cannot be distinguished from an I/O error. The message says both; a VFS change is out of scope.
- **vigrep delta after S7-1 closure.** The `FileSearch`, `line_submatches`, packed pending and onepass changes touch Cellos-owned `walk.rs`/`config.rs` and one test. Upstream-derived searcher files are untouched under P1.
- **Rollback.**
  - Revert the slice files together and rebuild a matched private image.
  - `search.rs` was untracked and is deleted: recover it from the S7-1-era working tree, or not at all (D70-6).
  - There is no wire change and no persisted state. Rollout containment, CP2 and quarantine are unchanged.

## 9. Decisions requested (recommendations first)
1. **D76-1 Pull/push bridge.** *P1 segmented restart in `vigrep::walk`, upstream searcher untouched, exactness by the §2 rule and the forced-pause differential* (recommended). Alternatives: P2 re-entrant upstream core patch; P3 per-file queue (rejected: heap).
2. **D76-2 vigrep delta in S7-3.** Add `libs/vigrep/src/{walk.rs, walk/tests.rs, regex/config.rs}`, `tests/differential/main.rs` and `PROVENANCE` to the S7-3 file set: `FileSearch`, the public `line_submatches`, `max_pending_entries`, packed pending children, onepass limit = `size_limit` (recommended). Alternative: none, which forces P3.
3. **D76-3 Heap.** *M1: keep the 1 MiB arena with the §6 `RG_PROFILE`, peak ≤ 512 KiB proven by a host counting-allocator test; ratify the tighter limits as **DV7-10*** (recommended, as D64-7 did for `top`). Alternative M2: a 2 MiB cellbox arena (`declare_custom_heap!`, +1 MiB bss per shell), allowing contract-sized limits.
4. **D76-4 Pending-children accounting.** *Packed storage, limit counted in real bytes (96 KiB) plus ≤ 8,192 entries* (recommended). Alternative: keep `Vec<DirEntry>` (32 B header per entry) with a lower name limit.
5. **D76-5 Record bound (clarifies DV7-8).** *S1: a row whose typed-JSONL encoding would exceed 65,536 B (`typed_json_len`, allocation-free) is a per-file error 2 and the walk continues* (recommended). Line content then effectively ≤ ~21 KiB for ASCII lines, because `line_bytes` hex doubles and `text` repeats them. Alternative S2: payload-only check, where an encode failure aborts the whole pipeline with status 1.
6. **D76-6 Collected output.** *K2: rg-headed Collected runs (redirect, `$(…)`, byte consumers) are bounded at 256 KiB; over that is error 2 with no redirect written; streaming redirect is recorded as a separate engine defect* (recommended: the 4 MiB `MAX_SINK_BYTES` cannot fit the 1 MiB arena). Alternatives: K1, a streaming redirect sink for all typed producers (engine-wide; changes "failure → no redirect"); K0, leave 4 MiB (OOM risk).
7. **D76-7 Latch cadence.** *A producer `check()` reads the latch every time and polls the session every 16th check* (recommended; one poll is a `PollEvents` IPC; matches the `ExtRun` 16-step precedent). Alternative: poll on every entry and chunk, as the C1 text literally says.
8. **D76-8 Status when stages follow rg.** *rg's completion is the pipeline status when only an optional explicit `to` follows it; otherwise the final stage's* (recommended: `to` is final rendering). Alternative: strictly the sole stage.
9. **D76-9 Default renderer.** *SearchText when the segment after rg is ⊆ {`where`, `take`, `sort-by`}, at the terminal and with `--text` everywhere including `RowFeed`; otherwise the existing defaults* (recommended). Alternative: SearchText only when rg is the sole stage.
10. **D76-10 Stdin heads.** *All four heads; the external stream behind a ≤ 96 KiB resume window, with a needed-but-evicted resume giving error 2* (recommended: completes "Bytes-to-Rows for explicit stdin"). Alternative: in-memory heads (Cat/Capture/FromFile) only; external heads rejected at admission.
11. **D76-11 Escaping.** *Contract set plus the bidi/format controls listed in §3.6 as `\xHH`* (recommended: they are not printable and can reorder displayed text). Alternative: contract set only (Cc).
12. **D76-12 Displayed path.** *The absolute canonical path, identical to the record* (recommended). Alternative: operand-relative like upstream, which needs a per-root display prefix.
13. **D76-13 512-byte argv bound.** *Enforce `Σ(len + 1) ≤ 512` in the rg parser* (recommended: contract §1 literal). Alternative: per-field limits only, because cellbox has no `/bin/cellbox rg` entry (`main.rs:116-119`).
14. **D76-14 Parent-handle cache.** *One cached parent directory handle in `VfsTree`* (recommended: halves per-file IPC; peak 2 dir + 1 file handles). Alternative: reopen and close per file.
15. **D76-15 Upstream rg status reporting.** *Reuse `earlier_failures` (any nonzero, including 1)* (recommended: same as an external `grep \| cat` today). Alternative: report only 2.
16. **D76-16 Runner split.** *The seven fixture-free terminal checks of §7.2 land with S7-3; fixture-dependent proof stays in S7-4* (recommended, as D64-15 did). Alternative: all runtime proof in S7-4.
17. **D76-17 Binary size.** *Record before/after, no budget* (recommended). Alternative: set a text budget after the first measurement.
18. **D76-18** Approve this S7-3 CP1 (§2–§7) as written, including the vigrep delta (D76-2), the `RG_PROFILE` deviation **DV7-10** and the DV7-8 clarification (D76-5).

Housekeeping: the stale `plan.md:39` Phase 07 row ("S7-1 CP1 (D72-5) pending") was corrected when this proposal was approved.

**D76-3 revised (user, 2026-10-24): M1 → M2.** The §6 host heap test measured a 4,445,961-byte peak for a 4,095-entry directory plus two 16 KiB / 256-rule ignore files (`prefix-*.bak` rules): `regex-automata` 0.4.16 construction of a globset peaks at ≈20 KiB per non-literal rule regardless of the hybrid cache size (a 256-rule set: ≈5 MB peak, ≈0.5–0.9 MB retained). The walker alone peaks at ≈302 KB. Lowering the profile would have left ≈8 regex rules for the whole tree, so the user chose M2 over that. Change from the §9 alternative as written: the arena is **6 MiB** (`HEAP_ARENA_BYTES`, `declare_custom_heap!`), not 2 MiB, because 2 MiB cannot hold the measured peak. The `RG_PROFILE` limits stay as DV7-10. The heap test target is now arena − 1 MiB = 5 MiB (the shell keeps the 1 MiB it ran in before). bss grows by 6 MiB per shell, because ostd's 1 MiB default arena is still linked (x86_64 PIC release: bss 7,340,280 B). `MAX_TABLE_HEAP_BYTES` and the 256 KiB rg Collected bound (D76-6) are unchanged.

## 10. Implementation results (2026-10-24)

S7-3 is implemented as approved (§3, D76-1..18, with D76-3 revised to M2). One change outside the §3.1 file set was needed; it is recorded below as D76-19 for CP2 ratification. CP2 is pending and rollout stays blocked.

### 10.1 Host
| Command (`--target x86_64-unknown-linux-gnu`, offline) | Result |
|---|---:|
| `cargo test -p text-engine` | 115 passed |
| `cargo test -p vigrep` | 590 passed, 1 ignored (lib 561 plus 6/5/1/10/7 across the integration targets, including the forced-pause differential) |
| `cargo test -p app-cellbox` | 170 passed |
| `cargo test -p service-vfs` (after D76-19) | 94 passed |

`cargo deny check licenses advisories` fails on two findings, and neither is in the `app-cellbox`/`vigrep` graph: the known `ocel-pdf` AGPL rejection, and RUSTSEC-2026-0387 in `elliptic-curve` 0.13.8 (reached through `ecdsa`/`p256` from `service-net`/`attestation`). Both are out of scope here; no new crate was added.

### 10.2 Runtime (private signed x86 images, TCG unless stated)
All runs below use the final tree (D76-19 included). Logs are in [reports/s7-3-evidence](reports/s7-3-evidence/).

| Lane | Result |
|---|---|
| `qemu-phase04-terminal-test.sh` | **437** checks PASS (155 driver markers), including the seven §7.2 `rg-*` checks |
| `qemu-phase04-gui-test.sh` | 279 GUI checks PASS; canonical cellbox/terminal/`kernel_fs.img` hashes unchanged |
| `qemu-phase03-pipeline-test.sh` | 62 PASS, observations complete |
| `qemu-phase02-session-test.sh` | 48 passes, observations complete |
| `PHASE02_KERNEL_TESTS=1 qemu-phase02-session-test.sh` | atomic publication 39/39; VFS dir-page markers 3/3 |
| `scripts/cellbox-ceiling-diff.py` | G-S1a PASS (33 syscalls, 6 manifest keys) |

### 10.3 Size (D76-17: recorded, no budget)
x86_64 PIC release `cellbox`, sha256 `ea19e7b6…55a9fd`: text **2,286,228** (+955,720 over §1.6), data **156,448** (+128,728), bss **7,340,280** (M2), file **2,723,400** (+1,187,680). The text growth is above the §7.3 estimate of +300–550 KB.

### 10.4 D76-19: P02 regression from the larger ELF; fixed in VFS (user-selected root-cause fix)
- **Symptom.** Before the fix, the P02 lane under TCG failed in 3 of 3 runs, and passed under KVM. The first external command after boot printed `shell: foreground preparation failed: Indeterminate`. Saved in `phase02-tcg-fail{,-serial}.log`.
- **Trace.** Temporary logs, since removed, gave this order of events:
  - The shell's `OpenRootDir("/")` (`libs/terminal/src/session.rs:249`, a single attempt bounded at 100 ticks) ended with `PreDispatchTimeout`.
  - VFS was still serving init's `ReadFileGrant` (op 13) for `/bin/cellbox`. That request started before the shell spawned and ended after the shell's timeout.
  - init's call had already expired. Its `GrantFree` was refused because the region was still pinned (`overlaps a pinned region … 1 in-flight operation`), and it fell back to the kernel ramdisk load.
- **Cause.** `backend_bootfs::read_to_vec` read with a 512-byte `ReadCap` loop. The ELF grew from 1.53 MB to 2.72 MB with vigrep, which made ≈5,320 syscalls inside the single-threaded VFS. The S7-2 baseline already showed the same `GrantFree` refusal, so the stall predates S7-3; the larger ELF pushed it past the shell's first request.
- **Fix** (`cells/services/vfs/src/backend_bootfs.rs`). Read the stat'ed size directly into the result in 64 KiB `ReadCap`s, so each kernel bounce buffer is at most one chunk. The previous 512-byte loop still drains anything past the stat'ed size.
- **After the fix.** P02 under TCG passes, with zero `GrantFree … refused` lines; the lane took 158 s, against 520 s before. No wire, right, syscall or SDK change.
- **For CP2.** `backend_bootfs.rs` is outside the §3.1 file set. The SDK's single 100-tick `OpenRootDir` attempt is unchanged, so any other long VFS request can still make the first spawn fail in the same way. That is recorded here as a known residual, not fixed.

### 10.5 Open
- S7-4 (fixture-dependent proof, §7.2) is pending.
- CP2 for S7-3 was **approved by the user on 2026-10-24, per the recommendations**: D76-19 is ratified; the SDK single-attempt `OpenRootDir` residual stays recorded and unfixed; the binary size is recorded without a budget (D76-17); the two out-of-graph `cargo deny` findings stay out of scope.
- Rollout containment is unchanged.
