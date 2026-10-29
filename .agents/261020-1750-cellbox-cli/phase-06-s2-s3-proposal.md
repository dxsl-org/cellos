# Phase 06 slices S6-2 and S6-3 — telemetry model and engine Frames (CP1 proposal)

Status: **APPROVED (CP1, user, 2026-10-23) — every §9 recommendation accepted:** D60-7 (S6-3 before S6-2; S6-4 narrowed to the Ratatui interactive view, drill-down and keys), D62-1 (one-capture `ps --records`, Null `cpu_tenths`, quality `new`), D62-2 (text `ps` on the shared renderer in S6-2), D62-3 (DV-9..DV-11 ratified), D63-4 (DV-12: `to json`/`to text` refuse every Frames stream), D62-4/D63-5 (both CP1s as written). Implementation follows in the order S6-3 → S6-2; results are appended in §10. The text below is the approved draft. S6-2 replaces the `text_engine::top` model, including its fabricated `build_cell_records`, its 64-row cap and its 100% clamp, and migrates every caller onto the S6-1 provider. S6-3 adds spec Frames (Begin/Row/End) to the typed engine plus the JSONL envelope form accepted in D63-1..3. Read-only census of the dirty working tree, 2026-10-23, after S6-1 landed with its runtime proof (`reports/s6-1-evidence/`; terminal 399/399, P03 60/60, P02 48, AP 38/38, GUI 259/259).

Inputs: [entry dossier](phase-06-entry-dossier.md) §2, §4, §4.2, §4.3, §7 and §10 (S6-1 results); [phase-06-cell-top.md](phase-06-cell-top.md) deviation log (user decisions D60–D65); [spec.md](spec.md) §1 and §2; [abi-vfs-telemetry.md](abi-vfs-telemetry.md) §5 (D04).

## 1. Facts

### 1.1 The model being replaced (`libs/text-engine/src/top.rs`, 297 lines; tests in `top/tests.rs`, 130 lines)
- `TIMER_HZ = 10_000_000` and `MAX_ROWS = 64` (`top.rs:13-17`). `parse_options` turns `-d SECS` into `secs × TIMER_HZ` raw `GetTime` units (`:55-86`), so the default delay is 10 ms on x86 (dossier §2.1).
- `build_rows` (`:93-122`) matches rows by bare `id` across two `ProcessInfoV2` slices. Its `cpu_tenths` (`:162-170`) divides scheduler-pass ticks by hart-0 ticks and **clamps to 1000**. `mem_bytes = owned_bytes`, which counts heap a second time.
- `CellRecord`/`build_cell_records` (`:215-297`) group tasks by the name prefix before `.`, set `cell_id = task.pid` and `generation: 1`, and keep the largest "shared heap". This is the fabricated identity withdrawn on 2026-10-20 (phase-06 deviation log). Nothing in the live path calls it. It is exported and tested by `top/tests.rs:104-130`.
- Old tests: 7 (`top/tests.rs`). Two pin the defects themselves: the clamp (`:48-62`, `cpu_tenths == 1000`) and the name-prefix grouping (`:104-130`).

### 1.2 Every caller of the old model (grep `text_engine::top`, `MAX_ROWS`, `TIMER_HZ`, `build_rows`, `build_cell_records`, `sample_processes`, `wait_ticks`)

| Caller | Uses | Lines |
|---|---|---|
| `cells/tools/cellbox/src/top.rs` | `build_rows, parse_options, TopOptions, MAX_ROWS, TIMER_HZ`. `sample_processes` reads `GetProcs2` into `[ProcessInfoV2; 64]` and truncates silently. `wait_ticks` and `wait_for_next_sample` **yield-spin** on raw `GetTime`. `KEY_POLL_TICKS = TIMER_HZ/50` | `:15, :52-57, :61-66, :70, :100-120` |
| `cells/tools/cellbox/src/top/render.rs` | `format_bytes, sort_label, state_label, SortKey, TopRow, MAX_ROWS, TIMER_HZ`. `uptime = GetTime / TIMER_HZ`. Hand-made `PID STATE CPU% HEAP MEM NAME` table | `:1-3, :8, :23-38` |
| `cells/tools/cellbox/src/native_rows.rs` | `ps --records` (`PsSource`, two samples `TIMER_HZ/10` apart = 1 ms on x86) and `top --records` (`TopSource`). Both emit flat `{pid,name,state,cpu_pct,cpu_tenths,heap_bytes,mem_bytes}` rows through `process_value` | `:14-16, :24, :63-83, :87-145, :213-300` |
| `cells/tools/cellbox/src/typed.rs` | `classify_command` calls `text_engine::top::parse_options` to tell interactive (`Session`) from batch `top` | `:128-134` |
| `cells/tools/cellbox/src/commands.rs` | `cmd_top` forwards to `crate::top`. Text `cmd_ps` uses legacy `GetProcs` into a local `[ProcessInfo; 64]`, a second silent cap that is not a `text_engine::top` caller. Failure prints a diagnostic and returns status 0 | `:614-616`; `:482-532` (`:495`) |
| `cells/tools/cellbox/src/executor.rs` | `top` maps `Err` to status 1 and bad options to 0, because `cmd_top` prints usage and returns `Ok` | `:1873-1878` |
| `build_cell_records` | No caller outside `text-engine/src/top/tests.rs` | — |

Not callers, and unchanged: `cmd_sys.rs` `sleep`/`uptime`/`date` (10 MHz defect, D60-3, its own CP1); `init`, `sys-tools ps/kill`, `hypha tools/sys` (legacy `GetProcs`); the parser tests that only parse `where cpu_tenths …` (`parser.rs:910-935`); `shell_test.rs:831` (refused at stage parse, whatever the field).

### 1.3 Pipeline engine (Phase 03)
- `Stage::{SortBy{field,reverse}, Take(usize)}` (`pipeline.rs:601-611`). `sort-by --frame` and `take --frame` are refused at parse (`:721, :737-740`; tests `:1996, :2006`). `SinkKind{Rows,Bytes}` (`:614-617`). `validate_stage_sequence(starts_with_bytes: bool, …)` (`:800-856`).
- `RowSource::next_row() -> Option<Value>` (`:1460-1465`). Implementations:
  - text-engine: `VecSource`, `SelectSource`, `WhereSource`, `TakeSource`, `SortSource`, and the test `Counting` (`:1468-1693, :1868`);
  - cellbox: `Tap` (`typed.rs:645`), `StreamRows` (`:1035`), `PsSource`/`LsSource`/`TopSource` (`native_rows.rs`), and the test sources `Counting` (`typed.rs:1444`) and `EndlessRows` (`shell_test.rs:1037`, `shell_test` feature build).
- Sinks:
  - `encode_row`/`finish_encoding` (`pipeline.rs:1394-1456`);
  - cellbox `run_rows`/`encode_all` (`typed.rs:716-750`), `stream_encoded`/`stream_table`/`stream_rows` (`:850-924`), `RowFeed` (`:929-1004`);
  - default sink `Table` on the terminal and `TypedJsonl` on a redirect or byte consumer (`:670-676`; chosen at `executor.rs:1518-1533`);
  - the table buffers the **whole stream** (`collect_table`, `typed.rs:1297-1359`; bounds `MAX_TABLE_ROWS = 4096`, `MAX_TABLE_HEAP_BYTES = 512 KiB`, `MAX_SINK_BYTES = 4 MiB`, `:43-49`).
- Untyped JSON refuses `UInt > 2^53−1` with `PrecisionLoss` (`pipeline.rs:384-391`).

### 1.4 Provider (S6-1, landed)
- `api::telemetry::TelemetrySnapshot::parse(&[u8])` gives a validated borrowed view. Its guarantees:
  - Cells sorted by generation, tasks by `(generation, tid)` (`libs/api/src/abi/telemetry.rs:379-433`);
  - header flags `COMPLETE`, `CPU_AVAILABLE`, `SYSMEM_PRESENT/DENIED`, `MEMORY_INTERVAL`;
  - per-row availability flags.
- `ostd::syscall::sys_telemetry_snapshot(&mut [u64], flags)` and `telemetry_bytes` (`libs/ostd/src/syscall.rs:1586-1615`). cellbox already declares `GetProcs2` (bit 55, shared) and `SetTimer` (`cells/tools/cellbox/src/main.rs:39-40`), so its syscall ceiling does not change.
- `provider_epoch` is a nonzero value drawn from boot entropy, or else RTC − monotonic. In practice it is always > 2^53 (dossier §10, E1/E2).
- `ostd::session::monotonic_ns()` uses the same `GetTime(0)` clock (`libs/ostd/src/session.rs:284-289`).
- `SetTimer(n)` parks the task for `n` `system_ticks` (`kernel/src/task/syscall.rs:6988-7001`).
- cellbox runs on the default 1 MiB ostd heap arena (`libs/ostd/src/heap.rs:28`; there is no `declare_custom_heap!` in cellbox).

## 2. Slice order and boundary (D60-7, recommended)

D60-4 fixed S6-2 → S6-3. Implementing that order literally leaves S6-2 without a Frames representation. `top --records` would then need an interim flat-row form that cannot carry `complete`/`dropped`, and the same runner checks would migrate twice (field names in S6-2, envelopes in S6-3).

**Recommended (D60-7):** approve both CP1s together and implement **S6-3 first**. It is pure engine work: no producer emits Frames yet, so no runner changes. **S6-2** then lands the model and the whole cellbox data-path cutover, with `top --records` emitting Frames from its first commit. Runner migration happens once, in S6-2.

S6-2 also absorbs the parts of the dossier's S6-4 row that the old-model deletion forces: the `ps --records` move, deleting the old sampler, the batch renderer, and text `ps` (D64-3, see D62-2). **S6-4 keeps** the vitui/Ratatui interactive view, drill-down and keys, the `TuiSession::next_event` wait, and the vitui dependency.

## 3. S6-3 — Frames in the typed engine

### 3.1 Types and API (`libs/text-engine/src/pipeline.rs`)
- New types:
  - `pub struct FrameBegin { sample_id: u64, monotonic_ns: u64, meta: Option<Value> }`. `meta` is a `Record` when present (D63-3 M1).
  - `pub struct FrameEnd { complete: bool, unavailable_fields: Vec<String>, dropped: u64 }`.
  - `pub enum Item { Begin(FrameBegin), Row(Value), End(FrameEnd) }`.
  - `pub enum Category { Rows, Frames }`.
- **Trait change (clean cutover):** `RowSource::next_row` becomes `next_item(&mut self) -> Result<Option<Item>, PipelineError>`. Every implementation in §1.3 is migrated. Rows producers wrap in `Item::Row`; nothing keeps a second method or a shim.
- `Stage::SortBy { field, reverse, frame: bool }` and `Stage::Take { n: usize, frame: bool }`. `parse_stage` accepts `sort-by [--reverse] [--frame] FIELD` and `take [--frame] N` (spec §2 rows `:28-29`). A duplicate flag is a usage error.
- `validate_stage_sequence(input: Input, stages) -> Result<SinkKind, _>`, with `Input { Bytes, Rows, Frames }` and `SinkKind { Bytes, Rows, Frames }`. It replaces the `starts_with_bytes` bool. Rules, all checked **at admission, before any producer is constructed**:
  - `select`/`where`: Rows→Rows, Frames→Frames.
  - `sort-by`/`take`: without `--frame` they accept Rows only. On Frames they are refused: "input is a frame stream; use `sort-by --frame` / `take --frame`". With `--frame` they accept Frames only; on Rows they are refused.
  - `to jsonl [--typed]`: Rows or Frames → Bytes. `to json [--typed]`, `to text [--field]`: Frames are refused, because they are continuous (spec §2 `:30`; see D63-4).
  - `from`: always Rows. `from jsonl` never turns envelopes back into Frames (D63-2).
- `pub fn check_items(source, Category) -> Box<dyn RowSource>` is an order guard applied once at the producer. Errors (`PipelineError::Sequence`):
  - any marker in a Rows stream;
  - a Row outside a frame, a nested Begin, or an End without a Begin;
  - a stream that ends inside a frame;
  - a `sample_id` that does not strictly increase.

  Downstream stages and sinks rely on this order without re-checking it.
- Frame-mode stages:
  - `where`/`select` filter or project rows and pass Begin/End through **unchanged** (spec §1 "preserves metadata"). Filtering is not truncation, so `complete`/`dropped` are not rewritten.
  - `take --frame N` emits N whole frames, then closes upstream right after the Nth End. N=0 closes without pulling.
  - `sort-by --frame`: Begin is emitted as received. Rows are buffered up to that frame's End, bounded by `MAX_SORT_RECORDS` (4,096) and `MAX_SORT_PAYLOAD_BYTES` (4 MiB), then sorted (stable, same `compare_values`) and emitted, then End. Nothing is pulled past End before the frame is emitted. Going over a bound is `SortLimitExceeded` (an error, never truncation).
- Encoding: `pub fn encode_item(format, category, &Item, first, out)` replaces `encode_row` at the sinks. In a Rows stream it behaves exactly like today's `encode_row`. In Frames (`Jsonl`/`JsonlTyped` only), the envelopes of D63-2 are:
  - `{"frame":"begin","sample_id":N,"monotonic_ns":N,"meta":{…}}` (`meta` is omitted when `None`)
  - `{"frame":"row","value":{…}}`
  - `{"frame":"end","complete":B,"unavailable_fields":[…],"dropped":N}`

  Each envelope is built as a `Value::Record`, taking ownership of the row (no copy), and serialized by the existing `value_to_json`. `--typed` therefore yields `{"cellbox":1,"value":["record",…]}` and round-trips through `from jsonl --typed` as an ordinary record. The line bound (64 KiB) and depth bound (16, envelope included) apply to the whole envelope.

### 3.2 cellbox plumbing (`cells/tools/cellbox/src/typed.rs`, `executor.rs`, `native_rows.rs`)
- `native_rows::producer_category(argv) -> Category`: `top --records` → Frames, `ps`/`ls --records` → Rows. It decides by name only and constructs nothing.
- `admit_inner` passes `Input::Frames|Rows|Bytes` to `validate_stage_sequence` (replacing `typed.rs:493`). `TypedPlan` gains `category: Category`.
- `run_rows`, `stream_rows` and `RowFeed::new` take `category`. Behavior per sink:
  - Explicit `to jsonl` and the redirect/non-terminal default `TypedJsonl` encode envelopes item by item.
  - The **terminal default table streams per frame**: a `-- sample N --` line, an aligned table of that frame's rows (the same `collect_table` bounds, applied per frame), then `-- complete --` or `-- incomplete: D dropped; unavailable: F,… --`. Each frame is written at its End, so an endless producer is never buffered whole.
  - `--text` with a redirect renders the same per-frame text, bounded by `MAX_SINK_BYTES`.
  - The display wording is not pinned by any test.
- `executor.rs:1439-1533`: the producer is wrapped with `check_items`; the category reaches the sinks.

### 3.3 Spec and docs (D63-3 M1)
- `spec.md` §1 `:16` becomes: "Frames are `Begin(sample_id:u64, monotonic_ns:u64, meta: optional Record)`, …; `meta` is producer metadata (schema, totals, units), never authority, and passes through row stages unchanged."
- `spec.md` §2 `to jsonl` row (`:31`) gains the three envelope shapes. The `to json` (`:30`) and `to text` (`:32`) rows state that Frames are refused.
- `spec.md` change-log entry, 2026-10-xx: "§1/§2 amended (D63-2/3): Begin `meta`, JSONL frame envelopes".
- CHANGELOG `Unreleased`.

## 4. S6-2 — telemetry model and caller cutover

### 4.1 Model (`libs/text-engine/src/top.rs` + new `top/sample.rs`, `top/records.rs`, `top/text.rs`; `top/tests.rs` rewritten)

**Deleted:** `TIMER_HZ`, `MAX_ROWS`, `SortKey`, `TopRow`, `build_rows`, `cpu_tenths` (the clamp), `compare_rows`, `state_label`, `sort_label`, `parse_sort`, `trim_name`, `CellRecord`, `build_cell_records`, the `ProcessInfoV2` import, and all 7 old tests.

**Options (`top.rs`, D64-2 grammar):**
- `top [-a] [-b] [-n COUNT] [-d SECS] [-o FIELD] [--fields LIST] [--tasks | --cell ID:GEN]`; cellbox handles `--records [--frames N]` as it does today.
- `pub enum View { Cells, Tasks, Cell { cell_id: u64, generation: u64 } }`
- `pub struct TopOptions { batch, count: Option<usize>, delay_ns: u64, show_all, view: View, sort: SortSpec, fields: Option<Vec<Field>> }`
- `pub fn parse_options(&[&str]) -> Result<TopOptions, &'static str>`
- `-d` is decimal seconds with up to 9 fraction digits, converted to ns with checked arithmetic; overflow is refused.
- `-o` takes a field name of the selected row kind. Aliases: `cpu`→`cpu_tenths`; `pid`→`cell_id`/`tid`; `name`; `state`; `mem`/`heap`→`quota_bytes` (Cell view only; the task view refuses them and names `stack_bytes`/`segment_bytes`).
- Numeric resource fields sort descending with Null last. Identity and text fields sort ascending. Ties break on identity.
- Unknown or conflicting flags → usage on stderr and **status 2**.
- `pub fn request_flags(View, sysmem: bool) -> usize`: the Cell view requests no task rows.

**Sampler (`top/sample.rs`):**
- `Sampler::observe(&mut self, &TelemetrySnapshot<'_>, View) -> Frame`.
- The baseline keeps only `(generation, cell_id, cpu_ns)` and `((generation, tid), cpu_ns)`, sorted as the provider emits them. Matching is a linear merge-join: no hashing, and no copy of the previous snapshot.
- Identity is `(provider_epoch, cell_id, generation)` or `(provider_epoch, tid)` (DV-1).
- `Quality { New, Ok, Reset, Unavailable }` on each row's `Cpu { ns, tenths: Option<u32>, quality }`:
  - **New:** no matching baseline row, or a provider epoch different from the baseline's (the whole frame is New and `interval_ns` is Null).
  - **Reset:** `cpu_ns` decreased.
  - **Unavailable:** the header or row `CPU_AVAILABLE` flag is clear, the interval is ≤ 0, or the result does not fit `u32`.
  - **Ok:** otherwise, `tenths = floor(Δcpu_ns × 1000 / Δcapture_start_ns)` in `u128`, **no clamp**. One busy core is 1000, and a two-thread Cell can show 2000 (D04 §5 core-equivalent).
  - `New`, `Reset` and `Unavailable` rows carry Null tenths, never 0.
- **Gone:** baseline identities missing from a *complete* capture are counted in `gone_cells`/`gone_tasks`. After an incomplete capture the counts are Null, because a missing row may only be truncated.
- **Memory:** a Cell's `quota_bytes`/`limit_bytes` come once per Cell from the provider and are Null when the availability flag is clear. A task's `stack_bytes`/`segment_bytes` are informational and are **never summed into the Cell** (DV-4). System memory is `Present{total,used,free bytes}` (frames × page size, checked), `Denied` or `Absent`.
- **Ownerless kernel tasks** (generation 0, dossier §10 deviation 4) get `cell_id`/`generation` = Null.
- `Frame { meta: FrameMeta, rows: Rows::Cells(Vec<CellSample>) | Rows::Tasks(Vec<TaskSample>), complete, dropped, unavailable_fields }`:
  - `complete` = header `COMPLETE` (and every row fits the frame bound);
  - `dropped` = total − emitted for this frame's row kind;
  - `unavailable_fields` = the fields the provider flagged unavailable in at least one row (or frame-wide for CPU).
- `Frame::retain(show_all, view)` hides Retiring rows unless `-a`. `Frame::sort(&SortSpec)` sorts one frame only; no history is kept.

**Records (`top/records.rs`, D64-1 R1: one schema per frame):**
- `top.cell.v1`: `cell_id, generation, root_tid, name, name_truncated, state("Live"|"Retiring"), tasks, cpu_ns, cpu_tenths, cpu_quality("new"|"ok"|"reset"|"unavailable"), quota_bytes, limit_bytes`.
- `top.task.v1`: `tid, cell_id, generation, name, name_truncated, state("Ready"|"Running"|"Waiting"|"Retiring"), cpu_ns, cpu_tenths, cpu_quality, stack_bytes, segment_bytes, stack_in_quota`.
- Begin `meta`: `schema, provider_epoch` (16 lowercase hex digits, a String; DV-9), `interval_ns, online_cpus, cpu_unit("core_tenths"), cpu_available, total_cells, total_tasks, gone_cells, gone_tasks, mem_state("present"|"denied"|"absent"), mem_total_bytes, mem_used_bytes, mem_free_bytes, memory_interval`.
- `pub fn frame_items(Frame) -> impl Iterator<Item = pipeline::Item>`: Begin{sample_id, monotonic_ns = capture_start_ns, meta}, then rows converted lazily, one `Value` per pull, then End.
- `pub fn task_rows(Frame) -> impl Iterator<Item = Value>` serves `ps --records`.

**Text (`top/text.rs`):**
- `render_frame(&Frame, columns, out: &mut String)` is shared by batch `top`, the interim interactive loop and text `ps`, so every text mode prints the same numbers as the records.
- Columns follow D64-4. Cell view: `CELL(id:gen) NAME TASKS CPU% QUOTA LIMIT STATE`. Task view: `TID NAME STATE CPU% STACK SEGMENTS`. `--fields` may name any field of the row kind.
- Header line: `sample`, `complete` or `TRUNCATED n`, the CPU unit, and system memory (`used/total`, `denied` or `n/a`).
- A non-Ok CPU prints `new`/`reset`/`n/a`, never `0.0`. Output fits 79 columns.
- `pub enum TopError { Denied, OutOfMemory, Malformed, Incomplete{emitted,total} }` provides `message()` and `status()`: 1 for each of these, 2 for usage.

**Bounds:**
- Rows per frame ≤ `MAX_FRAME_ROWS = MAX_SORT_RECORDS` (4,096).
- Snapshot buffer ≤ `MAX_SNAPSHOT_BYTES = 192 KiB`. That covers the default worst case of 64 Cells + 2,048 tasks = 186,464 B, so a default image is always complete. `cell-scale-experiment` populations beyond it are reported as explicit truncation.
- Model memory per frame: the snapshot buffer, about 24 B of baseline per row, and one sample per row. There is no history.

### 4.2 cellbox cutover
- **`top.rs`**:
  - `TelemetryReader { buf: Vec<u64>, flags }` keeps its buffer across captures. It starts at header + 64 Cells (+ 64 tasks when they are requested). After a capture without `COMPLETE` it grows once to the header totals, capped at `MAX_SNAPSHOT_BYTES`, and captures again (so `sample_id` may skip).
  - Errors: `PermissionDenied` → `TopError::Denied` ("top: telemetry denied", status 1, no rows); `OutOfMemory`; parse failure → `Malformed`.
  - The run loop: seed capture → wait → capture → `observe` → `retain` → `sort` → `text::render_frame`.
  - **Waits block** instead of spinning. The deadline is `monotonic_ns() + delay_ns`, and the loop runs `SetTimer(1)` steps (about one 10 ms tick on x86), polling quit keys between steps in interactive mode. With K1 billing, a spinning `top` would otherwise show itself as busy.
  - The interim interactive view is today's ANSI redraw over the new text renderer, with `q`/`Q`/Ctrl-C/EOF as the quit keys. S6-4 replaces it with vitui.
  - Deleted: `sample_processes`, `wait_ticks`, `KEY_POLL_TICKS`, and the `MAX_ROWS` buffer.
- **`top/render.rs`**: becomes a thin writer (`shell_print`, or `shell_console(CLEAR_SCREEN)` + `shell_live`) around `text::render_frame`. The `TIMER_HZ` uptime goes away; the header shows the sample's monotonic seconds.
- **`native_rows.rs`**:
  - `TopSource` (Frames) keeps one `Sampler` and one `TelemetryReader`. Its first pull seeds, waits and captures; each later frame waits and captures. It emits `frame_items`. `close()` drops reader and sampler, so a closed `top` never captures again (as today).
  - `PsSource` (Rows) makes **one** capture with tasks only and no delay (D62-1). It emits `top.task.v1` rows with `cpu_quality:"new"` and Null `cpu_tenths`. `-p TID` filters. An incomplete capture → `TopError::Incomplete` (status 1, no rows), because Rows cannot carry truncation.
  - Deleted: `PS_WINDOW_TICKS`, `process_value`, `sample`.
- **`commands.rs`**:
  - `cmd_ps` (text) runs the same single capture through `text::render_frame` in task view (D64-3, D62-2). It keeps `-p TID` and `-s` (state order). Read failure → status 1. Unknown flag → usage, status 2. The second 64-row cap and the `state_order` helper are deleted.
  - `cmd_top` returns a status.
- **`executor.rs:1873-1878`**: the `top`/`ps` exit statuses become 0/1/2 as above.
- **`typed.rs:128-134`**: unchanged in shape. `parse_options(..).batch` still separates `Session` from `Builtin`.
- Unchanged: `main.rs` `declare_syscalls!` (the ceiling rule; `GetProcs` stays declared although cellbox no longer calls it), `ledger.rs`, `Cargo.toml`.

## 5. Behavior changes visible to users
1. `top` and `top -b` show **Cells by default** (`--tasks` lists all tasks; `--cell ID:GEN` drills into one incarnation). CPU% is core-equivalent and can exceed 100. A new, reset or unavailable sample prints `new`/`reset`/`n/a`, not `0.0`. Heap/MEM columns are replaced by QUOTA/LIMIT for Cells and STACK/SEGMENTS for tasks.
2. `-d` is real seconds (it was 10 ms per "second" on x86) and accepts decimals. While waiting, `top` blocks instead of spinning.
3. No silent 64-row ceiling remains in `top`, `ps` or `--records`. Truncation shows `TRUNCATED n` (text), `complete:false`+`dropped` (frames), or an error (`ps`).
4. `top --records` emits **Frames**. Rows-only stages refuse it before running: `take N` and `sort-by FIELD` without `--frame`, `to json`, `to text`. `to jsonl` emits begin/row/end envelopes. The terminal default shows one table per frame. A redirect gets typed envelopes.
5. Record fields change. Removed: `pid`, `cpu_pct`, `heap_bytes`, `mem_bytes`. Added per schema: `tid`, `cell_id`, `generation`, `cpu_ns`, `cpu_quality`, `quota_bytes`, `limit_bytes`, `stack_bytes`, `segment_bytes`. Task state 3 is labelled `Retiring` (it was `Dead`).
6. `ps --records` takes one snapshot with no delay. Its `cpu_tenths` is Null with quality `new`; use `top --records --tasks` for rates. Text `ps` shows every task with the same columns as `top --tasks`.
7. Exit status: bad options → **2** (was 0 for `top` and ignored for `ps`). Denied, failed or incomplete telemetry → 1 with one diagnostic, never an empty "success".
8. In untyped `to jsonl`, a counter above 2^53 (about 104 days of ns) fails with `PrecisionLoss` instead of rounding. `--typed` is exact. `provider_epoch` is a hex string.

## 6. Verification plan (behavioral only; no heading or wording pins)

### 6.1 Host
- **`cargo test -p text-engine --target x86_64-unknown-linux-gnu`**. Snapshots are built with `TelemetryHeaderV1/CellV1/TaskV1::to_bytes` and decoded through `TelemetrySnapshot::parse`.
  - **S6-2:**
    - seeded frame: exact tenths (e.g. 250 ms of 1 s → 250);
    - New row → Null, not 0; `cpu_ns` decrease → Reset;
    - same `cell_id` with a new generation → New, never merged, and the old one counted Gone;
    - thread exit: Cell CPU keeps rising with exact tenths and `gone_tasks = 1`;
    - two busy workers → Cell 2000 and each task 1000 (no clamp);
    - CPU unavailable (header or row) → Null plus `cpu_tenths` in `unavailable_fields`;
    - provider-epoch change → all New, Null interval; zero interval → Unavailable;
    - quota read once per Cell and Null when flagged; task bytes never added to the Cell;
    - sysmem present, denied and absent;
    - truncated capture → `complete:false`, exact `dropped`, Null Gone counts;
    - `-a`, `--tasks`, `--cell ID:GEN` filtering; ownerless tasks get Null Cell fields;
    - `-o` order with Null last and identity ties; alias rules; unknown and conflicting flags → usage;
    - `-d 0.5` = 500,000,000 ns, and overflow is refused;
    - each schema's field set and types;
    - a default frame encodes in untyped JSON (`provider_epoch` as a String);
    - text: Denied, OOM and Incomplete produce one message and status 1 with no table (D65-1 host part); a truncated frame shows its count; a non-Ok quality never prints 0.0;
    - **one recorded snapshot pair** rendered as text and as records gives identical numbers and quality. This is the batch=records half of the "same snapshot" proof; the TUI half is S6-4.
  - **S6-3:**
    - parse accepts `--frame` and refuses duplicates;
    - every admission rule in §3.1, including the error text naming `--frame`;
    - `take --frame N` gives N whole frames, with a pull count showing the upstream closed after the Nth End; N=0 pulls nothing;
    - `sort-by --frame` sorts per frame with Begin/End preserved and nothing pulled past End; 4,096 rows pass and 4,097 → `SortLimitExceeded`; the payload bound behaves the same way;
    - `where`/`select` pass markers unchanged;
    - exact untyped envelope bytes; typed envelopes round-trip as Records; meta is omitted when `None`;
    - order-guard errors (each case in §3.1);
    - all existing Rows tests stay green after the trait migration.
- **`cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`** (existing harness, 97 tests):
  - Frames admission is refused before effects: a counting producer is never constructed or pulled;
  - `run_rows` and `RowFeed` produce identical bytes for Frames;
  - the terminal table streams per frame (a pull count proves no whole-stream buffer);
  - the redirect default is typed envelopes;
  - `producer_category`;
  - `top`/`ps` option classification and exit statuses.
- **Bare-metal builds:** `app-cellbox` x86_64-unknown-none release, also with `--features shell_test` (`shell_test.rs` migrates `Stage::Take`/`SortBy` and `EndlessRows`). Every `cellbox-ceiling-diff.py` check G-S1a must PASS.

### 6.2 Runner checks that change (all in S6-2 under D60-7; S6-3 changes none)
- **P03 `scripts/qemu-phase03-pipeline-test.sh`:**
  - `pid` → `tid` in the `ps --records` commands `:440-442, :444, :461, :485, :490, :512, :519-520, :534, :547, :571`, in the victim selection `:528-530, :553-556`, and in the checks `:663-668, :695, :711, :720-729, :738-753`. Meaning is unchanged: same TIDs and `state` labels (`"Running"`, `"Waiting"`). `:488`/`:697` are **not** renamed: there `pid` is the `p03-filter typed` fixture's own field.
  - `top-frames-closed-by-take` (`:454, :677-678`) → `take --frame 2`: exactly 2 begin and 2 end envelopes, rows only between them, `sample_id` strictly increasing, `complete:true`, `dropped:0`.
  - `typed-rows-stream-into-external-stage` (`:492, :699-700`): 3 frames of envelopes reach `/bin/p03-filter cat`.
  - `take-closes-endless-producer` (`:496, :704`) → `take --frame 2`: exactly 2 end envelopes.
  - **New:** `frames-rows-stage-refused` (`top --records | take 2` is refused with no output and status 2) and `sort-by-frame-live` (`top --records --frames 2 | sort-by --frame --reverse cpu_tenths | to jsonl`: rows ordered within each frame, metadata intact).
  - The P03 count goes from 60 to 62. `ctrl-c-cancels-rows-pipeline` keeps its command.
- **Terminal driver `cells/tests/phase04-terminal-driver/src/main.rs`:**
  - `count_cellbox` `:301`, `builtin-ps-records` `:403`, `typed-pipeline` `:423-427`: `pid` → `tid`.
  - `frontend-top-quit` `:1425`: instead of waiting for the heading `"PID  STATE"`, it waits for a data condition: a Cell row whose name is `cellbox`, drawn after the clear-screen sequence.
  - `top-pipe-batch` `:1529-1531` → `top --records | take --frame 1 | to jsonl`: one begin, one end, at least one row.
  - Unchanged: `ctrl-c-recovers` `:1235`, `endless_local_sink` `:1282`, text `ps` `oneshot-no-tui-bytes` `:1517`, and `ps-reflects-new-shell`. Count stays 399.
- **P02** serial `top` then `q` (`qemu-phase02-session-test.sh:291-319`): unchanged.
- **GUI** `phase04_gui_drive.py:701`: unchanged command. Its ink-in-last-columns detector still sees long JSON lines [INFERENCE, confirmed only by the run].
- **Not touched:** `s2tw-probe`, launch-profile digests.
- **Regression set after S6-2:** terminal 399, P03 62, P02 48, GUI 6×259, kernel AP 38/38. The S6-5 fixture (more than 64 tasks, restart and reuse, busy/idle workers) stays in S6-5.

## 7. Deviations from spec/D04 (for ratification)

| # | Source | Proposal | Why |
|---|---|---|---|
| DV-9 | D04 header `provider_epoch:u64` | ABI unchanged; in records it is a 16-hex-digit String | Always > 2^53 (E1 entropy, E2 RTC ns), so untyped JSON would refuse every frame |
| DV-10 | D04 "missing next sample → Gone" | Gone is a per-frame count in Begin `meta`, not rows; Null after an incomplete capture | R1 keeps one row schema per frame; a truncated capture cannot tell gone from dropped |
| DV-11 | D04 per-frame bound of 4,096 rows (Cell + task) | 4,096 rows per frame of one kind, plus a 192 KiB snapshot cap on the client | R1 frames never mix kinds; the cap keeps cellbox's 1 MiB heap safe and still covers the default population |
| DV-12 | spec §2 `to json`: "continuous input rejected" | Every Frames stream is refused by `to json`/`to text`, including `take --frame N`-bounded ones | No array/text form of envelopes is specified (D63-4) |
| — | spec §1 Begin, §2 `to jsonl` | Amended per D63-2/3 (§3.3) | Accepted decisions, recorded as amendments |

## 8. Risks and rollback
- **Heap.** cellbox has 1 MiB (`heap.rs:28`). A theoretical default worst case of 2,048 tasks shown in the terminal table needs about 186 KiB snapshot + ~250 KiB samples + up to 512 KiB table, which approaches the arena. Runners have about 30–100 tasks. S6-5 records the heap high-water at more than 64 tasks; Phase 04's OOM handling makes exhaustion a reported failure.
- **Trait ripple.** 13 `RowSource` implementations and the `shell_test` feature build. The change is mechanical, but every one must compile, so the bare-metal `shell_test` build is mandatory.
- **Runner churn.** About 20 command strings move `pid` → `tid`. A missed one fails loudly (`FieldNotFound`), never silently.
- **Interim UI.** Between S6-2 and S6-4 the interactive view is still the hand-made ANSI redraw. Quit-key latency is now up to one `SetTimer` tick (nominal 10 ms).
- **Self-observation.** cellbox's own Cell is billed for its captures, which is honest. It is not billed for waiting.
- **Rollback.** Revert the slice's files together and cold-boot a matched private image. `GetProcs2` and `MemInfo` are untouched, so reverting S6-2 restores the old consumer. There is no persisted state. JSONL recorded with envelopes is never re-read as Frames by either version.

## 9. Decisions requested (recommendations first; open after D60–D65)
1. **D60-7 Order/boundary:** *implement S6-3 before S6-2, approve both CP1s now, and narrow S6-4 to the vitui interactive view, drill-down/keys and the `TuiSession` wait* (recommended: one runner migration, and no interim producer that hides truncation). The alternative keeps S6-2 → S6-3 with interim flat rows that error when a frame is incomplete, and migrates the top checks twice.
2. **D62-1 `ps --records` CPU:** *one capture with cumulative `cpu_ns`, Null `cpu_tenths` and quality `new`* (recommended: an inventory, no hidden delay). The alternative takes two captures over a 100 ms monotonic window to fill `cpu_tenths`.
3. **D62-2 Text `ps`:** *move it in S6-2 onto the shared renderer, keeping `-p`/`-s`* (recommended: one sampler, the last 64-row cap gone with the old model). The alternative leaves it on `GetProcs` until S6-4.
4. **D62-3** Ratify DV-9..DV-11 (§7), or name the ones to keep as in D04.
5. **D63-4** *Refuse `to json`/`to text` on every Frames stream* (DV-12, recommended), or allow `to json` to emit an array of envelopes after `take --frame N`.
6. **D62-4 / D63-5** Approve the S6-2 CP1 (§4–§6) and the S6-3 CP1 (§3, §6) as written.

**Decision (user, 2026-10-23):** all six recommendations accepted as written (see Status).

## 10. Results (implemented 2026-10-23, S6-3 then S6-2; private QEMU runs archived in `reports/s6-23-evidence/`; CP2 approved 2026-10-24)

Runtime results and the final-tree runs: [CP2 dossier](phase-06-cp2-dossier.md) §7, §8 and §5. The §10.2 deviations were ratified with CP2 on 2026-10-24.

### 10.1 Files
- **S6-3 engine:** `libs/text-engine/src/pipeline.rs` — `FrameBegin`/`FrameEnd`/`Item`/`Category`/`Input`; `SinkKind::Frames`; `RowSource::next_item` (every source migrated, `VecSource::from_items` for fixtures); `Stage::SortBy{frame}`/`Stage::Take{n, frame}` and their parse (duplicates refused); `validate_stage_sequence(Input, …)` with the §3.1 rules; `check_items` order guard; `SortFrameSource` and frame-counting `take`; `encode_item` with the D63-2 envelopes (`encode_row` is now private); 10 new frame tests.
- **S6-3 cellbox plumbing:** `cells/tools/cellbox/src/typed.rs` (`TypedPlan.category`, admission by `Input`, `build_chain` wraps the producer with `check_items`, `run_rows`/`stream_rows`/`RowFeed::new` take the category, per-frame table via `write_tables`/`frame_footer`, `--text` redirect bounded by `MAX_SINK_BYTES`), `executor.rs` (category reaches the sinks), `native_rows.rs::producer_category`.
- **S6-2 model:** `libs/text-engine/src/top.rs` rewritten; new `top/sample.rs`, `top/records.rs`, `top/text.rs`; `top/tests.rs` rewritten (28 tests); `libs/text-engine/Cargo.toml` dependency comment.
- **S6-2 cutover:** `cells/tools/cellbox/src/top.rs` (`TelemetryReader`, `capture_tasks`, `wait_ns`, `cmd_top` → status), `top/render.rs` (thin writer), `native_rows.rs` (`PsSource` one capture, `TopSource` Frames), `commands.rs` (`cmd_ps` on the shared renderer, `cmd_top` → status, `state_order` and the 64-row `GetProcs` buffer deleted), `executor.rs` (`top`/`ps` statuses 0/1/2; `ps` left the legacy table), `shell_test.rs` (`Stage`/`EndlessRows` migration; `top` heading pins `HEAP`/`MEM` replaced by exit-status checks; `pid` → `tid`).
- **Runners:** `scripts/qemu-phase03-pipeline-test.sh`, `cells/tests/phase04-terminal-driver/src/main.rs` (§10.3).
- **Docs:** `spec.md` §1/§2 + change log, `phase-06-cell-top.md` deviation log, `CHANGELOG.md` (Added/Changed/Removed).

### 10.2 Implementation deviations (for CP2)
1. `check_items` is applied in `typed::build_chain`, the single constructor of every run/stream/feed chain, instead of at the `executor.rs` call sites; violations are attributed to the producer stage.
2. `encode_item` takes the `Item` by value (the envelope owns the row; no copy), not `&Item`.
3. `Cpu.ns` is `Option<u64>`: Null when the provider flags CPU unavailable (it writes 0 then), listed in `unavailable_fields`.
4. A `--cell ID:GEN` frame's `dropped` is the system-wide `total_tasks − emitted_tasks` (the provider does not say which Cell a dropped task belonged to).
5. `-o mem|heap` in a task view returns a dedicated usage line naming `stack_bytes`/`segment_bytes` (still status 2).
6. `TelemetryReader` always requests system memory (also for `ps`); without `observe_memory` the header shows `denied`. A capture error other than `PermissionDenied`/`OutOfMemory` maps to `Malformed`.
7. `wait_ns`: a failed `SetTimer` falls back to `yield_now` for that step; an unreadable monotonic clock ends the wait.
8. The interim interactive/batch view prefixes the shared renderer with one status line (`t=` monotonic seconds, mode, `q to quit`/`remaining`).
9. `top --records` accepts `--fields` (text-only); records always carry the full schema.

### 10.3 Runner migration (§6.2, nothing weakened)
- **P03** (60 → **62**): `pid` → `tid` in every `ps --records` command, victim selection and check (the `p03-filter typed` fixture's own `pid` kept); new parsers `frames_of` (begin opens with `frame, sample_id, monotonic_ns`; rows only inside a frame; every frame ends; `sample_id` strictly increasing), `cell_meta_ok` (exactly the 15 meta keys, `top.cell.v1`, 16-hex `provider_epoch`), `closed_ok` (`complete:true`, `dropped:0`). `top-frames-closed-by-take` → `top --records --frames 3 | take --frame 2 | to jsonl` (exactly 2 well-ordered frames, rows with `cpu_tenths`, closed_ok); `typed-rows-stream-into-external-stage` → exactly 3 frames through `/bin/p03-filter cat`; `take-closes-endless-producer` → `take --frame 2`, exactly 2 end envelopes; new `frames-rows-stage-refused` (`top --records | take 2`: `shell:` refusal only, `stfr=2`) and `sort-by-frame-live` (2 frames, meta intact, `cpu_tenths` descending with Null last per frame, ≥1 non-Null when `cpu_available`). `ctrl-c-cancels-rows-pipeline` unchanged.
- **Terminal driver** (stays **399**): `count_cellbox`, `builtin-ps-records`, `typed-pipeline` `pid` → `tid`; `frontend-top-quit` waits for a data row (an `id:gen` token followed by `cellbox`, after `ESC[2J`) instead of a heading; `top-pipe-batch` → `top --records | take --frame 1 | to jsonl` (one begin first, one end last, ≥1 row).
- **Unchanged:** P02 serial `top`/`q`, GUI `phase04_gui_drive.py:701`, `ctrl-c-recovers`, `endless_local_sink`, text-`ps` `oneshot-no-tui-bytes`, `ps-reflects-new-shell`. No other script or test reads `pid`/`cpu_pct`/`heap_bytes`/`mem_bytes` or the old `top` heading.
- **P03 follow-up after the first private run (59/62, `/tmp/cellbox-phase03-hLN8fY`):** (1) `typed-rows-stream-into-external-stage`: all 3 frames arrived, but the console starts every relayed 512-byte write record with `USER: ` even mid-line and spliced a kernel `[ INFO]` record into an open line, so per-line `json.loads` broke; the frame checks behind `/bin/p03-filter cat` (`typed-rows-…`, `take-closes-endless-producer`) now parse the stream rebuilt by `relayed()` (kernel records and record prefixes removed), with `frames_of` unchanged. The same mid-line prefixes are in the pre-cutover runs (shorter rows hid them). (2) `s2-kill-*`: no victim was chosen — `input` was `Ready`, not `Waiting`. D62-1 made `ps --records` one instantaneous capture; the old two-sample `PsSource` waited between samples, so `input` (which yields after delivering the Enter key and parks only on an idle turn) had already parked. The runner now samples with `sleep 1; ps --records | select tid name state | take 500 | to jsonl` (the shell yields until that turn has run), retaken up to 5 times when no Waiting candidate exists; the evaluator's before-set is the last such sample before `kill`. Every check condition is unchanged.
- **P03 second follow-up (reruns `/tmp/cellbox-phase03-fecweV`, `/tmp/cellbox-phase03-vxgD7j`, 60/62 each):** all five `sleep 1;` samples plus the final `ps` again show `init` Waiting, `vfs` Ready, `input` Ready and `cellbox` Running, so no `kill` was sent (14/14 task-state captures over the three new-model runs have `input` Ready; the pre-cutover runs have it Waiting). The state labels are not at fault: the kernel encodes `Sleeping`/`Recv`/every other blocked state as 2 (`syscall.rs` `encode_task_state`), the same function feeds `GetProcs2`, `api::telemetry::task_state::WAITING` is 2, and `top::sample` labels it `Waiting`; D04 renames nothing. Cause: (1) `input` no longer blocks indefinitely. Its owner loop is `sys_try_recv_attested`, then `SetTimer(1)` when idle or `sys_yield` when busy, so it is Waiting only from its turn until the next tick sweep, which queues it Ready again; `vfs` yield-spins (`try_recv_attested` + `yield_now`, ~98% CPU), and only `init` really blocks. (2) The builtin `sleep` yield-spun until raw `GetTime` advanced by 10 000 000, which is 10 ms of HPET nanoseconds on x86. It neither parked the shell nor moved the capture away from the tick at which `input` was just re-queued. The old two-sample `PsSource` waited 1 ms, a mid-tick point after `input` had parked again. Fix (product, not runner): `sleep N` now waits N real seconds through the shared `top::wait_ns` (`SetTimer(1)` steps; host tests `sleep_operand_*` in `cmd_sys.rs`). The shell's last wake then comes from the same tick sweep as `input`'s, the sweep walks TIDs in ascending order (`input` 6 before `cellbox` 7), and the ready bucket is FIFO, so `input` runs and parks before the capture. Checks, sampling command and retries are unchanged; only the runner comment was corrected.
- **P03 third follow-up (reruns `/tmp/cellbox-phase03-2boe79`, `/tmp/cellbox-phase03-CbvXmW`, 60/62 each, with the `sleep` fix):** all five `sleep 1;` samples and the final `ps` still show `init` Waiting, `vfs` Ready, `input` Ready, `cellbox` Running, so the second follow-up's tick-order explanation is **wrong**. Over the 12 retained runs, `init` is Waiting in 61/61 task-state captures, `input` is Ready in 26/26 new-model captures and Waiting in 35/35 pre-cutover ones. The old capture (`cells/tools/shell/src/top.rs` at HEAD, `wait_for_next_sample`) yield-spun between two `GetProcs2` samples, so the second one directly followed a yield and `input` had just parked; the single capture has no yield before it, and why `input` is still queued at that point is not established statically. Rather than tune timing, the victim is now deterministic: **`init`, selected by name** (its supervision loop blocks in `Recv` via `Message::receive(0, true)`), sampled with a plain `ps --records | select tid name state | take 500 | to jsonl` (no `sleep`, at most 3 samples; no `kill` unless init is Waiting, and then both checks fail). `s2-kill-victim-selected` additionally requires the victim row's name to be `init`; `s2-kill-builtin-refused-no-ipc` is unchanged (Waiting in the before-sample, refusal text, status 1, no signal/ForceExit text, survivor, unchanged process set). Rejected: a blocked `/bin/p03-filter` victim (the serial-session shell refuses `&` for SDK launches, `executor.rs` `spawn_serial_foreground`: "SDK background launch requires explicit nonterminal streams", so it needs a cellbox background-launch feature); re-sampling relative to the tick (still timing luck). **For ratification:** (1) the runner's old exclusion of `init` from the victim pool ("non-system cells preferred") is dropped: `init` is now the only victim; (2) the `sleep` fix landed in the second follow-up although D60-3 declared the 10 MHz `sleep`/`uptime`/`date` users a separate defect with its own CP1 (and §1.2 lists `sleep` as unchanged). It is kept as an independent defect fix (`sleep N` was N × 10 ms on x86, it is now N real seconds on every architecture); the runner no longer depends on it, and `uptime`/`date` stay in D60-3.
- **Ratified (user, 2026-10-23):** (A) the P03 kill victim is `init`, selected by name; the old "non-system cells preferred" exclusion of `init` stays dropped. (B) The `sleep` fix stays landed as an independent defect fix despite D60-3 (`sleep N` = N real seconds on every architecture); `uptime`/`date` remain in D60-3 with their own CP1.

### 10.4 Verification run here (host and build only)
- `cargo test -p text-engine --target x86_64-unknown-linux-gnu`: **105 passed** (incl. 10 `pipeline::frame_tests`, 28 `top::tests`).
- `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`: **105 passed** (was 97; +6 Frames tests in `typed.rs`: `producer_category`, admission refusals before any producer, `RowFeed` = `run_rows` bytes for Frames, per-frame terminal table with pull counts, typed-envelope redirect default, order-guard attribution; +2 in `commands.rs`: `ps` options, `top` classification/usage).
- Bare metal `x86_64-unknown-none --release -Z build-std=core,alloc` (`relocation-model=pic`): `app-cellbox` and `app-cellbox --features shell_test` build (only warnings are pre-existing ones outside these files); `app-phase04-terminal-driver` builds.
- `scripts/cellbox-ceiling-diff.py`: **G-S1a PASS** (declared syscall/manifest sets unchanged).
- `bash -n scripts/qemu-phase03-pipeline-test.sh` OK; all 6 embedded Python heredocs compile.

### 10.5 Expected runner totals (to be run by the parent)
Terminal **399/399**, P03 **62/62**, P02 **48**, GUI **6 × 259/259**, kernel AP **38/38**. Criteria in `phase-06-cell-top.md` stay unchecked until these runs and S6-4/S6-5 land.
