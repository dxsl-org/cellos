# Phase 06 slice S6-4 — interactive Ratatui `top`, task drill-down and keys (CP1 proposal)

Status: **APPROVED (user, 2026-10-23) — every §9 recommendation accepted: D64-5 S1, D64-6 W1 (keep the `TuiSession` tick wait), D64-7 Z1 (clamp 120×40), D64-8..D64-18 as recommended. Implemented; see §10.** Scope follows D60-7: S6-2 already landed the batch renderer, `--records` Frames, the `ps` cutover and the blocking `SetTimer` waits. **S6-4 keeps** the vitui/Ratatui interactive view, task drill-down and keys, the `TuiSession::next_event` wait, and the cellbox `vitui` dependency. Accepted inputs: D64-1 (R1, one schema per frame), D64-2 (CLI and key map), D64-3 (text `ps` on the provider, done in S6-2), D64-4 (columns and labels). Tests are behavioral only; no heading, label or wording pins.

## 1. Facts

### 1.1 Current `top` (after S6-2/S6-3)
- `cells/tools/cellbox/src/top.rs` (185 lines):
  - `TelemetryReader { buf, flags }`: the request flags are fixed per `View` at construction (`:56-61`). A capture without `COMPLETE` grows the buffer once (`:65-78`).
  - `wait_ns(delay_ns, stop)` parks in `SetTimer(1)` steps (`:98-115`). `sleep` (`cmd_sys.rs:126`) and `TopSource` (`native_rows.rs:255`) also use it.
  - `run_top`: seed capture → wait → capture → `retain` → `sort` → `render::render_frame` (`:136-159`). On exit the interactive path clears the screen (`:155-157`).
  - `quit_requested`: on a session, `TerminalKeys::read(0)` with `q`/`Q`/Ctrl-C/EOF meaning quit and every other key ignored. Without a session it polls legacy fd 0 for `q` (`:164-185`). Ctrl-C therefore exits with status **0** today.
- `top/render.rs` (27 lines): one status line (`t=`, mode) plus `text::render_frame`. Batch goes to `shell_print`. Interactive does `shell_console(CLEAR_SCREEN)` + `shell_live` (the interim ANSI redraw, S6-2 deviation 8).
- Classification (`typed.rs:128-134`): `top` without `-b` is `CmdClass::Session`. It is refused before any effect in a pipeline (`:356`, `:378`), with a redirect (`:338`) and in the background (`background_refusal`, `:638`). `top --records` is a Frames producer (`:175`).
- `TerminalKeys` (`executor.rs:566-619`) is used only by `top`.

### 1.2 Text-engine model (S6-2)
- `top.rs`:
  - `View { Cells, Tasks, Cell{cell_id, generation} }`, `RowKind`, `Field` (16 fields), `CELL_FIELDS`/`TASK_FIELDS`, `SortSpec{field, descending}`, `TopOptions`, `parse_options`;
  - `request_flags`: the Cells view requests **no task rows** (`:282-288`);
  - `MAX_SNAPSHOT_BYTES` = 192 KiB.
- `top/sample.rs`:
  - `Sampler::observe(&snap, view) -> Frame` (`:421-544`). Cell marks are recorded for every view. Task marks are kept only when the view shows tasks (`tasks_seen = kind == Tasks`, `:473`), so the first task frame after a switch from the Cells view has quality `New` (Null tenths).
  - `Frame { meta, rows, complete, dropped, unavailable_fields }`, `retain`, `sort` (identity tie-break).
- `top/text.rs`:
  - `render_frame(frame, columns, out)` with `MAX_WIDTH = 79`;
  - private `Column`, `cell_text`/`task_text`, `fit_widths`, `header_line`;
  - public `format_cpu`, `format_bytes` and `TopError{Denied, OutOfMemory, Malformed, Incomplete}` with `message`/`status`.
- `top/records.rs`: `frame_items`, `cell_record`/`task_record`, `META_FIELDS`.
- `top/tests.rs:782` `one_snapshot_pair_gives_identical_numbers_as_text_and_records` uses a **constructed** pair (`default_pair`), not a recorded capture.

### 1.3 vitui (Phase 05, CP2-approved bytes)
- `backend::CellosBackend<W: ByteSink>`:
  - CUP/ED/EL/SGR (16/256/RGB) output. Screen mode changes only through SDK `TerminalMode` transitions.
  - Output is buffered and handed to the sink in `ByteSink::MAX_WRITE` chunks.
  - `set_size` takes effect at the next `Terminal::draw`, which clears and repaints.
  - `Drop` restores `CANONICAL`.
  - The `Endpoint` sink writes with deadline `u64::MAX` (`backend.rs:35-41`).
- `session::TuiSession<'s, W>`:
  - It **borrows `&'s mut TerminalSession` for its whole life** (`session.rs:41-47`).
  - `enter`: capabilities → `RawInput::enter` (raw + bracketed-paste lease) → `TUI_SCREEN` (alternate screen, hidden cursor, paste) → clear (`:53-83`).
  - `next_event(deadline_ns)`: an absolute monotonic deadline. A Resize is applied to the backend before it is returned (`:92-108`).
  - `exit`/`Drop`: restore the screen, then release the lease (`:112-138`).
  - It builds `Terminal::new` (Fullscreen viewport) internally.
- `event::Event { Key, Text, Paste, Resize, Interrupt, Eof, Lost }` (Debug is redacted).
- `theme`:
  - `Theme::new(ColorMode)` (Palette::DEFAULT, rounded borders);
  - `block(title)`, `title_bar`, `status_line`, `highlight_style` + `HIGHLIGHT_SYMBOL "▌ "`, `key_hints`, `muted_style`;
  - `palette.warning`/`error`;
  - colour mode is explicit because capabilities advertise none.
- `RawInput::next_event` (`libs/terminal/src/raw_input.rs:53-84`) polls `next_control(0)` and `read_input(0)` (kernel `ReadStream`, deadline 0), then `SetTimer(1)` per tick until the deadline. It is **tick-polled** (~100 wakeups/s at the nominal 10 ms tick), not an owner-side blocking wait. The canonical `TerminalSession::next_event` instead blocks in the owner's `PollEvents` for up to 100 ms (`session.rs:97-140`).
- Input owner `SetMode` (`cells/services/input/src/session.rs:476-485`, `MODE_MASK = 1|2|16` at `:27`):
  - the caller must be the current reader;
  - pending typed input → `WouldBlock`;
  - echo-off on the physical UART → `Unsupported`; raw (bit 0) is allowed there.
- `QueryCapabilities` returns `frontend` kind (0 serial, 1 GUI, 2 SSH; `api::cell_session::frontend`), rows and cols (`:535-539`).

### 1.4 How cellbox owns its terminal
- The shell is the **controller** of its own session on every surface (option C). It is installed in the static `SERIAL_SESSION: Mutex<Option<TerminalSession>>` (`executor.rs:55`).
- Every shell write goes to the session's stdout ring and is then **drained by the shell itself** (`render_serial` → `relay_serial` → `relay_device`) to the UART (fd 1/2) or the frontend holder's writers (`executor.rs:145-206, 246-268`). No other task drains the ring.
- Consequence: `TuiSession::enter(session, session.stdout())` would deadlock.
  - Its sink writes the ring with an infinite deadline, and the only drainer is the blocked shell.
  - A sink that relays through `terminal_stdout` would need `SERIAL_SESSION` while `TuiSession` already holds `&mut` to it.
  - vitui's default `Endpoint` sink therefore cannot be used in-process (§3.2, D64-5).
- The REPL wraps each line in one capture (`main.rs:199`). `flush_repl_capture()` (`executor.rs:1312`) writes earlier output of the same line before a live stream. `shell_live`/streamed rows use `serial_active() && capture_depth() <= 1` as the "terminal is ours" test (`executor.rs:364-370, 1528, 3648`).
- Safety net: `screen_reset::ModeTracker` and `RelayScreen` plus `SOFT_RESET` (`screen_reset.rs:22,48-80,164-179`). They are used today for foreground children.
- Ctrl-C convention: interrupted builtins and pipelines print `^C` and return **130** (`executor.rs:1555-1556, 3697-3698`).

### 1.5 Linking cost (measured where possible)
- **Dependencies.** cellbox already links `terminal-sdk`, `ostd` and `text-engine`. Adding `vitui` brings in `ratatui-core =0.1.0` and `ratatui-widgets =0.3.0` (defaults off; `Table`, `List`, `Paragraph`, `Block`, `Clear` are available without features). Their closure (Cargo.lock) is: `bitflags`, `compact_str`, `hashbrown`, `indoc`, `itertools`, `kasuari`, `lru`, `strum`, `thiserror`, `unicode-segmentation`, `unicode-truncate`, `unicode-width`, `instability`, `line-clipping`.
  - All of these are already admitted for the GUI Terminal. There is no new crate admission.
  - vitui adds no syscalls (session ops ride existing service calls). The G-S1a ceiling diff must still PASS (not run here).
- **Binary size** (`size` on the S6-2/3 GUI-run build, `/tmp/cellbox-phase04-gui-Kqz4/cells-target/x86_64-unknown-none/release`, 2026-10-23 17:12):

  | ELF | File bytes | text | data | bss |
  |---|---|---|---|---|
  | `cellbox` | 1,271,368 | 1,095,459 | 24,592 | 1,048,738 (incl. the 1 MiB heap) |
  | `terminal` (links vitui) | 941,512 | 886,677 | 8,088 | 5,242,944 |

  The Ratatui share of `terminal` could not be attributed: generics are inlined, and only ~6.5 KB of symbols carry Ratatui crate names. **The cellbox increment is unmeasured** and is measured by S6-4 (§7.3). No loader ELF-size limit was found by grep (`kernel/src/loader`) [INFERENCE: none exists].
- **Heap.** cellbox uses the default 1 MiB `linked_list_allocator` arena (`libs/ostd/src/heap.rs:28`). Allocation failure logs `OOM: cell heap exhausted` and **exits the cell** (`heap.rs:111-139`), which kills the shell and its session.
  - A Ratatui `buffer::Cell` is **40 B** (measured: host build of `ratatui-core 0.1.0`, defaults off), and `Terminal` keeps two buffers, so **80 B per screen cell**:

    | Size | Cells | Two buffers |
    |---|---|---|
    | 80×24 serial | 1,920 | 153,600 B |
    | 79×26 GUI | 2,054 | 164,320 B |
    | 100×30 (driver) | 3,000 | 240,000 B |
    | 120×40 | 4,800 | 384,000 B |
    | 132×50 | 6,600 | 528,000 B |
    | 512×256 (session max) | 131,072 | 10.5 MB |

  - A full repaint also builds a transient diff `Vec<(u16, u16, &Cell)>` (16 B per changed cell).
  - On top of that come the snapshot buffer (≤ 192 KiB) and the per-frame samples.
  - **An unbounded viewport would OOM the shell on a large session**, so a cap is mandatory (D64-7).

## 2. Interactive vs batch selection

| Invocation | Path | Keys read | Screen-mode bytes |
|---|---|---|---|
| `top -b …` | batch text (S6-2, unchanged) | none | none |
| `top --records …` | Frames producer (S6-3, unchanged) | none | none |
| `top …` in a pipeline, with a redirect or `&` | refused before effects, status 2 (existing admission) | — | none |
| `top …` sole stage, admitted as `Session` | **runtime gate**, below | — | — |

**Runtime gate** (`executor::interactive_terminal()`, new):
1. A controller session is installed (`serial_active()`: serial or frontend).
2. `capture_depth() <= 1`: only the REPL capture is active, which excludes `$(top)` and any nested capture.
3. `capabilities()` succeeds.

If any of these fails, `top` refuses with one diagnostic naming `-b` and **status 2**. It draws nothing and makes no capture (D64-9). This replaces the legacy fd-0 `q` poll and the interim ANSI path. Both are deleted (clean cutover), and so is the legacy compositor `ViShell` path's interactive `top` [INFERENCE: no current runner reaches that path].

**Order of effects:**
1. Parse options. A usage error gives status 2.
2. Seed capture. A telemetry error gives status 1 with the S6-2 diagnostic and **no TUI bytes**.
3. Flush the REPL capture.
4. Drain type-ahead (D64-17).
5. Enter the TUI.

Telemetry denial therefore renders exactly as in batch mode.

## 3. Design

### 3.1 Files and APIs

| File | Change |
|---|---|
| `cells/tools/cellbox/Cargo.toml`, `Cargo.lock` | `vitui = { path = "../../../libs/vitui" }` |
| `cells/tools/cellbox/src/top.rs` | `run_top` keeps the batch loop; the interactive branch calls `tui::run`. `TelemetryReader::set_view(View)` changes the request flags and keeps the buffer. Delete `quit_requested` and the fd-0 poll |
| `cells/tools/cellbox/src/top/render.rs` | Batch writer only; delete the interactive branch and `CLEAR_SCREEN` (no other user) |
| **new** `cells/tools/cellbox/src/top/view.rs` | Pure and host-testable. `ViewState { view, sort, fields, selected: Option<RowId>, offset, picker: Option<FieldPicker>, notice: Option<Notice> }`. `fn on_event(&mut self, &Event, &Frame) -> Step { Idle, Redraw, Recapture, Quit(i32) }`. `fn draw(&self, frame: &Frame, theme: &Theme, area: Rect, buf: &mut Buffer)` using `vitui::ratatui::widgets::{Block, Table, Paragraph, List, Clear}`. There is no lifecycle action variant |
| **new** `cells/tools/cellbox/src/top/tui.rs` | The session loop (§3.4), colour mode, size cap, exit/status mapping |
| **new** `cells/tools/cellbox/src/top/tests.rs` | Host tests (§7.1) |
| `cells/tools/cellbox/src/executor.rs` | `pub fn interactive_terminal() -> bool`. `pub fn with_tui<R>(f: impl FnOnce(&mut TerminalSession, TuiSink) -> R) -> Result<R, Status>`. `pub struct TuiSink` (`ByteSink`, §3.2). `TerminalKeys` stays (type-ahead drain) |
| `libs/text-engine/src/top/text.rs` | Shared table model (§3.3); `render_frame` is rebuilt on it with **byte-identical batch output** |
| `libs/vitui/src/session.rs` (+ tests) | **Only if D64-7 = Z1:** additive `TuiSession::enter_bounded(session, sink, max: Size)`, a `Viewport::Fixed` clamped to `max`, applied again on Resize |
| Docs | `CHANGELOG.md` (Unreleased), `phase-06-cell-top.md` deviation log, `command-catalog.md` `top` entry (keys, gate, statuses) |

`Cargo.toml` features and `main.rs` `declare_syscalls!` are unchanged.

### 3.2 Output path and session borrow (D64-5, recommended S1)
`executor::with_tui` performs these steps:
1. Take the `SERIAL_SESSION` guard once.
2. `render_serial(session)`, so nothing is left queued in the rings.
3. Hand `&mut TerminalSession` plus a `TuiSink` to the closure, which runs `TuiSession::enter(session, sink)` and the loop.

`TuiSink` (`MAX_WRITE = 512`) writes each chunk with `relay_device(false, chunk)`. That is exactly the device the ring relay would reach: the UART fd 1 on serial, or the holder's stdout writer (with backoff) in frontend mode. It also feeds a `ModeTracker`.

After the closure returns (normal exit, error or early return), `with_tui` writes `SOFT_RESET` once if the tracker is still dirty. That covers a restore lost to a write error.

Why this is safe:
- The ring is empty before entry.
- While the guard is held, nothing else in the single-task shell writes to the session: no child exists, and `shell_console`/`shell_stderr` are never called inside the closure.
- Byte order to the device is unchanged.

Rule: diagnostics are written only after `with_tui` returns. Otherwise the non-reentrant guard would deadlock.

### 3.3 Shared table model (`text.rs`, no new numbers)
Promote the private column logic to `pub`:
- `pub enum Column { CellIdentity, Field(Field) }`;
- `pub fn columns(kind, fields: Option<&[Field]>) -> Vec<Column>`, using the D64-4 defaults;
- `pub fn label(Column) -> String`;
- `pub fn cell_text(&CellSample, Column)` and `pub fn task_text(&TaskSample, Column)`;
- `pub fn right_aligned(Column)`;
- `pub fn widths(frame, &[Column], max_width) -> Vec<usize>`: `fit_widths` with a width parameter, computed over **all** rows by formatting into one reused `String`, with nothing retained;
- `pub fn header_text(frame) -> String`: today's header line.

`render_frame(frame, columns, out)` becomes `header_text` + `widths(.., MAX_WIDTH)` + cells. Batch output must stay byte-for-byte equal; a host test checks this through the existing exact-number tests plus one before/after equality test on the default pair.

The TUI uses the same functions with `max_width = inner table width`. Every number, unit and quality marker on screen is therefore produced by the code that prints batch text.

### 3.4 Session loop (`tui.rs`)

```
seed = reader.capture(view)?                      // error → status 1, no TUI bytes
flush_repl_capture(); drain_type_ahead()?         // D64-17
with_tui(|session, sink| {
  tui = TuiSession::enter[_bounded](session, sink)?    // WouldBlock → re-drain, ≤ 8 tries
  theme = Theme::new(color_mode(capabilities.frontend)) // D64-8
  frame = capture now; draw
  next = monotonic_ns() + delay_ns
  loop match tui.next_event(next) {
    Key/Text  → view.on_event: Redraw (re-sort/select current frame, no capture) |
                Recapture (view change: capture immediately, next = now + delay) | Quit(0)
    Resize    → redraw (backend already resized; Ratatui clears + repaints)
    Interrupt → Quit(130)          Eof → Quit(0)
    Paste     → scrub, ignore      Lost(FRONTEND notices) → D64-13; other Lost → ignore
    Err(TimedOut) → capture; retain; sort; draw; next = max(next + delay, now); count -= 1 (-n)
    Err(other)    → Quit(1, "top: terminal input failed")
  }
  tui.exit()                                          // restore + release lease
})
then: diagnostic (if any) on stderr; for 130 print "^C\n" (shell convention); return status
```

- **Waiting:** only `TuiSession::next_event(deadline)`, with no yield spin. The idle cost is the RawInput tick poll (§1.3). It is measured (§7.2) and stays per D64-6.
- **`-n COUNT`:** frames counted as in batch mode, then status 0. **`-d`:** decimal seconds on `monotonic_ns` (D64-12 floor).
- **Capture error after entry:** exit the TUI first (screen restored, lease released), then one diagnostic, status 1. A stale frame is never left on screen as current.

### 3.5 Keys (D64-2; no lifecycle key exists)

| Key (raw-mode event) | Cells view | Task view (`--tasks`, `--cell`, drill-down) | Field picker |
|---|---|---|---|
| Up/Down | select ±1 | same | move |
| PageUp/PageDown | ± visible rows | same | move by page |
| Enter | drill into the selected incarnation | no-op | apply |
| Esc/Backspace | no-op | return to the Cells view | cancel |
| `o` | cycle the sort through the shown columns (CELL → `cell_id`), natural direction | same | — |
| `r` | reverse the sort direction | same | — |
| `f` | open the field picker | same | — |
| Space | — | — | toggle the field |
| `q`/`Q` | quit, 0 | quit, 0 | quit, 0 |
| Ctrl-C | quit, 130 | quit, 130 | quit, 130 |

- Printable keys arrive as `Event::Text`; each character is handled in order. A paste never acts as keys.
- Selection follows **identity** (`(cell_id, generation)` or `tid`) across frames, sorts and resizes. If the selected row disappears, the selection keeps its index (clamped).
- The field picker lists every field of the row kind with its toggle state. Applying it yields `fields = Some(checked, in record order)`, rendered exactly like `top -b --fields LIST`. An empty set falls back to the renderer default.

### 3.6 Drill-down (D64-11 recommended T1)
- Enter on a Cell row switches to `View::Cell { cell_id, generation }` taken from that row's identity, never its name.
  - `reader.set_view` adds task rows and a capture runs at once.
  - Task CPU shows `new` for one interval, because the sampler keeps no task baseline in the Cells view. The next frame has rates.
  - The block title names `ID:GEN` and the root name.
- If that incarnation is gone (exited, or relaunched under a new generation), the body says so as a notice. **The view never switches to the new generation.**
- Esc/Backspace returns to the Cells view with an immediate capture; the Cell baseline was kept throughout. The previous Cell stays selected if present.
- Truncation is never hidden: a drill-down frame shows `TRUNCATED n` like batch (S6-2 deviation 4: dropped is system-wide).

### 3.7 Layout (vitui::theme; GUI 79×26, serial 80×24)

| Row | Content | Theme |
|---|---|---|
| 0 | title bar: view (`Cells` / `Tasks` / `Cell ID:GEN NAME`), sort field and direction, delay | `title_bar` (accent background, full row) |
| 1 | `text::header_text(frame)`: sample id, `complete` / `TRUNCATED n`, CPU unit, memory (`used/total`, `denied`, `n/a`) | `status_line`; `TRUNCATED` in `palette.warning`; `denied`/`n/a` in `muted_style` |
| 2 … H−2 | `theme.block(view title)` over the **full width**, so its right border occupies the last column. Inside it: a `Table` with header row = `label(Column)` in `title_style`, rows = `cell_text`/`task_text` for **visible rows only**, widths = `text::widths(.., inner − 2)`, `highlight_style` + `HIGHLIGHT_SYMBOL` with the highlight column always reserved. Notices (exited incarnation, empty view, size clamp) render as a `Paragraph` in `palette.warning` | `block`, `highlight_style` |
| H−1 | key hints: `q quit  Up/Dn select  Enter tasks  Esc back  o sort  r reverse  f fields` (ASCII keys, ≤ 79 columns) | `key_hints` |

- At 79×26 this gives 21 table lines (header + 20 data rows). At the 40×12 session minimum it gives 6 data rows.
- Non-Ok CPU prints `new`/`reset`/`n/a`, never `0.0` (shared `format_cpu`).
- No column is labelled RSS, and task bytes are never summed (D64-4, DV-4).
- Field picker: a centred `theme.block` over `Clear` with a `List`.
- Glyphs: only box drawing, block elements and ASCII, which the census shows the GUI font covers.
- Untrusted Cell/task names pass through Ratatui cells. The backend replaces control characters, so a name cannot inject escape sequences into the TUI.

### 3.8 Resize and size cap (D64-7 recommended Z1)
- `Resize` is already applied by `TuiSession`. `top` redraws at once, and Ratatui clears and repaints every cell. The layout and visible rows are recomputed and the selection is kept visible.
- **Cap:** the view draws into at most **120×40 = 4,800 cells** (Ratatui buffers ≤ 384,000 B, full-repaint diff ≤ 76,800 B).
  - Under Z1 the cap is a `Viewport::Fixed` clamp at the top-left.
  - The status line shows that the view is clamped, and the area outside it stays blank after the clear.
  - Serial, GUI and the driver's 100×30 and 120×40 sizes are below or at the cap.

### 3.9 Exit and prompt restoration
- `q`, EOF and `-n` exhaustion return status 0. Ctrl-C returns 130 and prints `^C` after restore (D64-10). A failure returns 1 with its diagnostic.
- Every path goes through `TuiSession::exit` (or its `Drop` on `?`), which sends the `CANONICAL` transition (alternate screen off, cursor visible, bracketed paste off) and releases the raw lease **before** the REPL reads the next line. The `ModeTracker` net then sends `SOFT_RESET` if needed.
- Lease release failure: the SDK `RawInput::leave` drops the reference even when release fails (`raw_input.rs:43-46`). The prompt would then get `WrongDirection`, and the shell would exit 125. This is recorded as a risk (§8). S6-4 does not change the SDK.

## 4. Error and denied rendering

| Condition | Rendering | Status |
|---|---|---|
| usage error | usage on stderr, no TUI | 2 |
| no interactive terminal (§2 gate) | one diagnostic naming `-b`, no TUI, no capture | 2 |
| seed capture: Denied / OOM / Malformed / grow-then-incomplete | S6-2 diagnostic (`top: telemetry denied`, …), no TUI bytes | 1 |
| later capture error | TUI exited and restored, **then** the diagnostic | 1 |
| truncated frame | rows shown, `TRUNCATED n` in the status line; Gone counts Null | — |
| CPU unavailable / new / reset | `n/a` / `new` / `reset`, never 0.0 | — |
| system memory denied | `mem denied` | — |
| drilled incarnation gone | notice in the body; no rows from another generation | — |
| frontend detached mid-run (D64-13) | sink error or `Lost(LOST_FRONTEND)` → leave the TUI, diagnostic | 1 |
| raw lease refused (type-ahead keeps arriving) | after 8 drain/enter tries: `top: terminal input busy` | 1 |

## 5. Security
- Visibility is unchanged: the same System-scope, X-gated capture.
- Observation grants no lifecycle right: the reducer has no lifecycle variant, and `k`/`K`/Delete are inert.
- Input is a raw lease taken through the owner. The owner restores it if the shell dies.
- Typed and pasted text is scrubbed (vitui `Event::scrub`).

## 6. Bounds
- **Heap at 79×26:**
  - Ratatui ≈ 164 KB, plus ≤ 33 KB transient diff;
  - snapshot ≤ 192 KiB (Cells view ≈ 6 KB);
  - samples for one frame;
  - Strings for visible rows only.
- **Heap at the cap:** ≈ 461 KB for Ratatui plus diff. A worst case of 2,048 tasks at the cap approaches the arena; the >64-task fixture run in S6-5 is the proof (no `OOM:` line).
- There is no history and no per-frame unbounded allocation.

## 7. Verification (behavioral only)

### 7.1 Host
- `cargo test -p text-engine --target x86_64-unknown-linux-gnu`:
  - batch `render_frame` output is byte-equal before and after the §3.3 refactor on the default pair;
  - `widths(.., w)` fits `w` for w = 75…79 and 38;
  - all existing tests stay green.
- `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu` (`top/tests.rs`):
  - **Same snapshot (step 6, TUI half):** one constructed pair (D64-14) is observed. `view::draw` renders into a Ratatui `Buffer` at 79×26. For each visible row, identity, CPU% string, quality marker and byte cells equal batch `render_frame` and `frame_items` records. Ok, New and Reset are all exercised.
  - **Reducer:**
    - Down/Up/PageDown clamp;
    - selection follows identity after a re-sort and a new frame;
    - Enter → `View::Cell` with the row's `(cell_id, generation)` plus `Recapture`;
    - Esc → Cells plus `Recapture`;
    - `o` cycles the shown columns and `r` flips;
    - `f`/Space/Enter yield `fields` equal to `parse_options(["--fields", …])`;
    - `q` → `Quit(0)`, Interrupt → `Quit(130)`;
    - `k`/`K`/Delete/paste → `Idle`.
  - **Drill-down to a gone incarnation:** a notice, and no row of generation + 1.
  - **Geometry:**
    - at 79×26 and 80×24 the last column is non-blank on every row 2…H−2 (border);
    - title and status rows are fully styled;
    - the field picker stays inside the area;
    - the 40×12 minimum renders 6 rows;
    - a clamped size keeps the cap (Z1).
  - **Rendering of states:** truncated (`TRUNCATED n`, warning style), unavailable CPU, mem denied.
  - **Selection gate:** `interactive_terminal()` is false without a session or at capture depth 2.
  - **Exit-path mapping:** Denied at seed → status 1 with no screen bytes; this is the D65-1 host part.
- `cargo test -p vitui --target x86_64-unknown-linux-gnu` (Z1 only): the bounded viewport never allocates beyond `max` on a 512×256 Resize, and the backend resize is exact.
- Bare metal:
  - `app-cellbox` (and `--features shell_test`) for x86_64-unknown-none, release, PIC;
  - `scripts/cellbox-ceiling-diff.py` **G-S1a PASS**;
  - `size` before/after recorded (§7.3).

### 7.2 Runtime (private QEMU/TCG; recovers the archived tui-proof coverage)
The terminal driver gains `screen.rs`, a minimal grid model for the vitui output profile: CUP, ED/EL, SGR incl. `38;2`/`48;2`, DECSET/DECRST 1049/25/2004, UTF-8, and the last-column pending wrap. The removed `tui.rs` was untracked and cannot be restored. Checks read the reconstructed grid by **structure**: border glyphs, `HIGHLIGHT_SYMBOL`, `id:gen` tokens, statuses and mode bytes. They do not read wording.

| Archived marker | New check | Runner | Asserts |
|---|---|---|---|
| `tui-frame` | `top-tui-frame` | terminal | `1049h` + cursor hidden. The border spans exactly 100×30. A row `id:gen cellbox` exists |
| `tui-reread` | `top-tui-refresh` | terminal | without input, ≥ 2 frames with strictly increasing sample id (`r` is now reverse, D64-2) |
| `tui-keys` | `top-tui-keys` | terminal | Down moves the highlight one row, PageDown one page, Up back. `o` → rows ordered by the new column. `r` → reversed. `f`/Space/Enter → column count ±1 |
| — (new) | `top-tui-drill` | terminal | navigate to `cellbox`, Enter. The title carries that `id:gen`. Row TIDs equal the TIDs from `top -b -n 1 --cell ID:GEN` run after quitting. Esc → the Cells view, `cellbox` row present |
| `tui-resize` | `top-tui-resize` | terminal | `resize(40,120)` → full repaint, border 120×40. `resize(24,80)` → 80×24. `resize(50,132)` → clamped at 120×40 (Z1), shell alive, no `OOM:` |
| `tui-exit-restores` | `top-tui-quit-restores` | terminal | `q` → `1049l`, `?25h`, `?2004l`, prompt, `echo $?` = 0. The next typed command runs (lease released) |
| `tui-ctrl-c-130` | `top-tui-ctrl-c-130` | terminal | Ctrl-C → restored, `^C`, `$?` = 130 |
| `tui-two-sessions` | `top-tui-two-sessions` | terminal | `top` on hosts a and b. Quitting a leaves b drawing. Each host sees only its own bytes |
| — (new) | `top-tui-idle-cost` | terminal | while a idles in `top` for ≥ 5 s, b's `top -b -n 2 -d 2` shows a's Cell CPU with quality ok; the value is recorded and checked against the threshold fixed with D64-6 |
| — (new) | `top-tui-no-lifecycle` | terminal | `k`, `K`, Delete leave the `ps --records` TID set unchanged |
| `tui-pipe-refused` | `top-pipe-batch` (kept), new `top-subst-refused` | terminal | `top \| /bin/cat` status 2 (kept). `x=$(top)` → status 2, no TUI bytes |
| `tui-abandon-recovered` | **not recoverable** | — | `top` is in-process: losing the holder kills the shell. The shell's soft reset stays covered by `soft-reset-after-tui-abandon` (`p03-filter` fixture) |
| `frontend-top-quit` (kept) | migrated | terminal | waits on the grid model, not on `ESC[2J` tokenising |
| P02 serial `top` + `q` | extended | P02 | UART shows `1049h` and a `cellbox` Cell row. `q` → `1049l` + prompt. `top` + `0x03` → prompt and `$?` = 130 |
| `TUI-0`/`THEME-*`/`FRAME-*` | `TOP-0` | GUI ×6 | typed line exact (prompt screen) |
| `FRAME-0/1/2` | `TOP-1` | GUI ×6 | frame size is the window grid (79×26). **The last column is inked on every row** (right border). Control: the pre-`top` prompt screen has blank last-column cells. Title-bar row has the accent background on all 79 cells; the status row has the surface colour |
| `THEME-2` | `TOP-2` | GUI ×6 | no blank pixel rows between stacked `│` on the right and left borders, nor gaps along `─` (control: a scan that finds the inter-row gap on the prompt screen) |
| `TUI-2` | `TOP-3` | GUI ×6 | Down moves the highlight band (`palette.highlight` background) by exactly one cell row. Control: the band is on the first data row before. Enter → the band returns to the first row and the title region changes |
| `TUI-3`, `THEME-3`, `FRAME-3` | `TOP-4` | GUI ×6 | `q` → restored prompt screen exact. Control: a one-character change differs |
| `TUI-4`, `THEME-4`, `FRAME-4` | `TOP-5` | GUI ×6 | `echo st=$?` → `st=0` exact |

- The GUI window cannot be resized, so resize stays a terminal-runner claim, as in Phase 05.
- **Not provable at runtime:** a mid-run Denied/OOM/Malformed path. cellbox always holds X and the provider cannot be made to fail; this is host-tested only.
- Fixture-dependent TUI checks go to **S6-5** (D64-15): drill into `top-load` with known busy/idle workers, relaunch → the old `ID:GEN` shows its notice and the new generation is separate, > 64 rows paged to the last row with `complete`.
- Regression: terminal 399 + new markers, GUI 6 × (259 + new), P03 62, P02 48 + extensions, kernel AP 38/38. Totals are fixed at implementation.

### 7.3 Size and heap evidence
- Record `size` of the new `cellbox` against §1.5.
- Prove the cap at runtime: `top-tui-resize` at 132×50 must leave no `OOM:` line on the UART (S6-5 repeats this with > 64 tasks).
- ostd has no heap-statistics API. None is added in S6-4.

## 8. Risks and rollback
- **Idle cost:** the RawInput tick poll bills `top` (and the kernel stream ops) at the tick rate. It is visible in `top` itself, which is honest. D64-6 alternative W2 trades this for a vitui change.
- **Lease-release failure** (§3.9) would end the shell. It needs the session to be frozen at that instant; no mitigation without an SDK change.
- **Shell abort mid-TUI** (panic = abort, OOM exit): the frontend is left in the alternate screen until it resets [INFERENCE: GUI Terminal behaviour on controller loss not verified].
- **Heap:** see §6. Z1 removes the large-session OOM path.
- **Rollback:** revert the slice files together and rebuild a matched private image. Batch output and records are unchanged by construction, and there is no persisted state.

## 9. Decisions requested (recommendations first)
1. **D64-5 Output path.** *S1: cellbox `TuiSink` writes straight to the relay device while `with_tui` holds the session guard; rings drained first; `ModeTracker` + `SOFT_RESET` net* (recommended: no SDK/vitui change, same bytes and order). Alternative S2: an SDK accessor so a borrowed session can drain its own rings (SDK API change).
2. **D64-6 Input wait.** *W1: `TuiSession` as is (raw lease, tick-polled `RawInput`), idle cost measured and recorded* (recommended; matches D60-7 "the `TuiSession` wait"). Alternative W2: a canonical-mode `TuiSession` variant over the owner's blocking `PollEvents` (vitui change; Ctrl-C and arrows already arrive as typed events).
3. **D64-7 Size bound.** *Z1: additive `TuiSession::enter_bounded` with a `Viewport::Fixed` clamp at 120×40 (4,800 cells)* (recommended: no OOM on any session size). Alternatives: Z2, leave the TUI with status 1 above the cap (no vitui change); Z3, a 2 MiB cellbox heap (`declare_custom_heap!`, doubles every shell's arena).
4. **D64-8 Colour mode.** *GUI → TrueColor; serial, SSH or unknown → Ansi16* (recommended: the host terminal behind a UART is unknown). Alternative: TrueColor everywhere.
5. **D64-9 No interactive terminal** (`$(top)`, no session). *Refuse with status 2 and a `-b` hint* (recommended; Phase 05 step 8 "explicit failure"). Alternative: silently run one batch frame.
6. **D64-10 Ctrl-C.** *Status 130 with `^C` after restore* (recommended; shell convention). Alternative: keep 0 (interim).
7. **D64-11 Drill-down CPU.** *T1: fresh capture, task CPU `new` for one interval, no model change* (recommended). Alternative T2: always request tasks in the TUI and let the sampler keep a task baseline whenever the snapshot carries tasks (S6-2 model change; rates at once).
8. **D64-12 Interactive delay floor.** *Refuse `-d` below 0.1 s in interactive mode with status 2* (recommended: `-d 0` would redraw continuously). Alternative: allow it.
9. **D64-13 Frontend detach during `top`.** *Leave the TUI with status 1* (recommended; the shell continues Detached). Alternative: pause and repaint on re-attach (needs a forced mode re-send in vitui).
10. **D64-14 Same-snapshot fixture.** *A constructed pair built with `api::telemetry` `to_bytes` in app-cellbox tests, as S6-2 did* (recommended). Alternative: a recorded private capture (needs a dump path that does not exist).
11. **D64-15 Runner split.** *Fixture-free TUI checks (§7.2) land with S6-4; fixture-dependent ones in S6-5* (recommended: S6-4 is not done without its own runtime proof). Alternative: all in S6-5.
12. **D64-16 Binary size.** *Record before/after with no fixed budget* (recommended: no loader limit found, increment unmeasured). Alternative: set a text budget after the first measurement.
13. **D64-17 Type-ahead before entry.** *Drain typed keys first: `q`/Ctrl-C/EOF typed before the first frame are honoured (0/130/0), other keys are discarded and scrubbed; then take the raw lease, retried ≤ 8 times* (recommended). Alternative: refuse "terminal input busy" whenever input is pending.
14. **D64-18** Approve this S6-4 CP1 (§2–§7) as written, including the `text.rs` table-model refactor with byte-identical batch output.

## 10. Results (implemented 2026-10-23; private QEMU runs archived in `reports/s6-4-evidence/`; CP2 approved 2026-10-24)

Runtime results and the final-tree runs: [CP2 dossier](phase-06-cp2-dossier.md) §9 and §5. The §10.2 deviations not ratified earlier (1–8, 10, 11, the rest of 12) and the driver `TUI_ENTRY` change were ratified with CP2 on 2026-10-24.

### 10.1 Files
- **Table model:** `libs/text-engine/src/top/text.rs` — public `Column` (`label`, `right_aligned`, `field`), `columns(kind, fields)`, `cell_text`/`task_text` (formatted through one push path), `widths(frame, columns, max_width)` (over all rows, one reused buffer), `header_parts` (`HeaderMark::{Plain, Truncated, Unavailable}`) and `header_text`, `GAP`; `render_frame` rebuilt on them. `top/tests.rs` +2 (`batch_text_is_byte_identical_to_the_pre_refactor_layout` against a frozen copy of the old layout over the default pair, a truncated capture and over-wide rows, each with default/all/subset fields; `table_widths_fit_the_requested_width` for 38 and 75–79).
- **vitui (Z1):** `libs/vitui/src/session.rs` — `TuiSession::enter_bounded(session, sink, max)`, `Viewport::Fixed(bounded_area(size, max))`, `apply_resize` re-clamps on Resize (backend keeps the exact session size); `enter` unchanged. `tests.rs` +1 (`bounded_viewport_never_exceeds_its_cap_and_the_backend_size_is_exact`: 512×256 Resize keeps 120×40 = 4,800 buffer cells, exactly 4,800 painted cells, no CUP beyond row 40/col 120, backend size 512×256; shrink to 80×24 follows; an unbounded terminal is untouched).
- **cellbox:** `Cargo.toml`/`Cargo.lock` (`vitui`); `src/top.rs` (`TelemetryReader::set_view`, `run_batch`, `cmd_top` maps the interactive `Outcome`, prints `^C` for 130; `quit_requested` and the fd-0 poll deleted; `wait_ns(delay_ns)`); `src/top/render.rs` (batch writer only, `CLEAR_SCREEN` deleted); **new** `src/top/view.rs` (`ViewState`, `Step`, `RowId`, `Notice`, field picker, `draw`); **new** `src/top/tui.rs` (`run`, `run_with`, `Source`, `color_mode`, `MAX_VIEW` 120×40, `MIN_DELAY_NS`, drain/enter ≤ 8 tries, event loop); **new** `src/top/tests.rs` (14 tests); `src/executor.rs` (`interactive_terminal`, `terminal_is_ours`, `with_tui`, `TuiSink`, `TerminalKeys::read_from`; `shell_live` and `TerminalKeys::read` deleted; `flush_repl_capture` public; the capture-stack test now also checks the gate); `src/cmd_sys.rs`, `src/native_rows.rs` (`wait_ns` callers); `src/shell_test.rs` (+2 refusal checks: `top -n 1` and `top -d 0.05` → 2 without a session).
- **Runners:** **new** `cells/tests/phase04-terminal-driver/src/screen.rs` (grid model: UTF-8, CR/LF/BS/HT, CUP/cursor moves, ED/EL with background, SGR 16/256/RGB, 1049/25/2004, save/restore, last-column pending wrap, OSC/DCS skip; layout from the border corners) and `main.rs` (every host feeds its stdout into the model; `top_tui` after `selective_tui`; `frontend-top-quit` waits on the grid); `scripts/qemu-phase04-terminal-test.sh` (11 markers registered); `scripts/qemu-phase02-session-test.sh` (serial `top` step extended); `scripts/phase04_gui_drive.py` (`TOP-0`..`TOP-5`).
- **Docs:** `CHANGELOG.md` (Added/Changed/Removed), `phase-06-cell-top.md` log, `command-catalog.md` `top` row.

### 10.2 Implementation deviations (for CP2)
1. `ViewState::draw(&mut self, frame, theme, area, clamped, buf)`: `&mut` because the draw records the visible page (PageUp/PageDown step) and the scroll offset; `clamped` comes from the session size the loop knows.
2. The status line is built from `text::header_parts` (added beside `header_text`) so `TRUNCATED n` and `denied`/`n/a` are styled without re-parsing the line. `label`/`right_aligned` are `Column` methods, plus `Column::field` (CELL → `cell_id`) for the sort cycle and the picker.
3. The type-ahead drain runs inside `with_tui` on the held session (`TerminalKeys::read_from`), immediately before each lease try; the session-locking `TerminalKeys::read` had no other caller and is deleted.
4. The selection-gate host test lives in the executor capture-stack test (the only test allowed to touch the process-global capture stack), plus the pure `terminal_is_ours(session, depth)` predicate (`true,2` and `false,1` refuse).
5. Interactive `-n COUNT` counts every drawn capture (the first frame and view-change recaptures included); after the last one the view stays up for one delay, then exits 0.
6. `FRONTEND_ATTACHED` redraws (unreachable in practice: `LOST_FRONTEND` already ends the view, D64-13); other `Lost` reasons are ignored.
7. A failed restore (`TuiSession::exit` error) after an otherwise successful run gives status 1 with a diagnostic; `with_tui` sends `SOFT_RESET` when the tracker is dirty **or** any device write failed (a failed write leaves an unknown prefix on the device).
8. `TuiSink` is `Copy` (the sink is offered again on each lease retry).
9. The field picker counts the CELL column as `cell_id`; applying writes `--fields` record names, so the `ID:GEN` column becomes a `cell_id` column (there is no `--fields` token for CELL). **Ratified (user, 2026-10-23).**
10. Body notices: `Gone` only after a complete capture (an incomplete one cannot tell), `Empty` when the view has no rows; the size clamp is shown on the status line (`clamped WxH`).
11. `top::wait_ns` lost its `stop` callback (its only real user, the interim quit poll, is deleted).
12. Runner choices: `top-tui-idle-cost` threshold is **< 500 tenths** (half a core), an implementation choice recorded in the check detail for user ratification (D64-6 fixed no number) — **ratified (user, 2026-10-23): the threshold stays 500 tenths**; `top-tui-keys` and GUI `TOP-3` first press `o` four times to sort by `cell_id` so navigation order is stable; the P02 parser drops console line breaks inside the TUI segment (kernel lines and `USER+ ` continuations, the UartPrefix fix's format) and `USER: ` prefixes before tokenising.

**User ratifications (2026-10-23, after the private runs in `reports/s6-4-evidence/`):**
- Idle-cost threshold: `top-tui-idle-cost` keeps **< 500 tenths** (50% of one core) for the Cell of an idle interactive `top` (deviation 12).
- Ctrl-C in interactive `top` exits **130** and prints `^C` after the screen is restored (D64-10 as implemented; runtime `top-tui-ctrl-c-130` and P02 `tc=130`).
- The field picker shows and applies `cell_id` where the table shows `ID:GEN` (deviation 9).

### 10.3 Size (D64-16, recorded, no budget)
`size` of the release `x86_64-unknown-none` PIC `cellbox` (`-Z build-std=core,alloc`), same working tree before and after this slice:

| ELF | File bytes | text | data | bss |
|---|---|---|---|---|
| before (S6-2/3 + VFS idle fix) | 1,275,656 | 1,094,251 | 24,632 | 1,048,738 |
| after (S6-4) | 1,545,488 | 1,334,827 | 27,864 | 1,048,824 |
| delta | +269,832 | +240,576 | +3,232 | +86 |

The heap is unchanged (1 MiB arena); the 120×40 cap bounds the Ratatui buffers at 384,000 B. Runtime heap proof is `top-tui-resize` at 132×50 (no `OOM:`), repeated with > 64 tasks in S6-5.

### 10.4 Verification run here (host and build only)
- `cargo test -p text-engine --target x86_64-unknown-linux-gnu`: **107 passed** (was 105).
- `cargo test -p vitui --target x86_64-unknown-linux-gnu`: **23 passed** (+1 bounded viewport).
- `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`: **121 passed** (14 new `top::tests`: same snapshot TUI = batch = records with Ok/New/Reset; arrows/pages clamp; selection follows identity across sort, frame and a vanished row; Enter → `View::Cell{id,gen}` + Recapture, Esc/Backspace → Cells + Recapture with the drilled Cell reselected; `o` cycle over shown columns, `r`; picker = `parse_options(["--fields", …])`, ±1 column, Esc cancels, empty → default; `q`/`Q`/EOF → 0, Interrupt → 130, `k`/`K`/Delete/paste/release → Idle with state unchanged; gone incarnation → notice and no generation+1 row, incomplete capture → no notice; geometry at 79×26, 80×24, 40×12 (6 rows), 120×40: last column inked on every row 2…H−2, title/status/hint bars filled, highlight band; picker and a clamped view never touch cells outside their area; truncated/unavailable CPU/denied memory styled and never `0.0`; Denied seed → status 1 and the terminal step never runs; no terminal or `-d 0.05` → status 2 naming `-b`; colour mode by frontend kind).
- Bare metal `x86_64-unknown-none --release -Z build-std=core,alloc` (`relocation-model=pic`): `app-cellbox`, `app-cellbox --features shell_test` and `app-phase04-terminal-driver` build (cellbox's two warnings are pre-existing: `executor.rs` `paste`, `parser.rs` `BackgroundPipeline`).
- `scripts/cellbox-ceiling-diff.py`: **G-S1a PASS** (declared syscall/manifest sets unchanged).
- `bash -n` on `scripts/qemu-phase02-session-test.sh`, `scripts/qemu-phase04-terminal-test.sh`, `scripts/qemu-phase04-gui-test.sh`: OK; `python3 -m py_compile scripts/phase04_gui_drive.py` OK; every embedded `<<'PY'` heredoc compiles (5 in P02, 10 in the terminal runner).

### 10.5 Expected runner totals (to be run by the parent)
- Terminal **410/410** (399 + 11: `top-subst-refused`, `top-tui-frame`, `top-tui-refresh`, `top-tui-drill`, `top-tui-keys`, `top-tui-no-lifecycle`, `top-tui-resize`, `top-tui-quit-restores`, `top-tui-two-sessions`, `top-tui-idle-cost`, `top-tui-ctrl-c-130`; driver markers 117 → 128; `frontend-top-quit` migrated to the grid model, `top-pipe-batch` kept).
- GUI **6 × 279/279** (259 + 20: `TOP-0` 1, `TOP-1` 4, `TOP-2` 4, `TOP-3` 4, `TOP-4` 2, `TOP-5` 5).
- P02 **48** `P02 PASS` markers (unchanged count) plus 6 new fail-closed runner problem checks on the serial `top` step (`1049h` + `id:gen cellbox` row, `q` → `1049l` + prompt, `0x03` → restore + prompt, `tc=130`); the existing `top_quit` check kept.
- P03 **62/62** and kernel AP **38/38** unchanged.
- Criteria in `phase-06-cell-top.md` stay unchecked until these runs and S6-5 land.
