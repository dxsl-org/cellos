# Phase 04 — endless typed pipeline OOM in the local sink (CP1 proposal)

Status: **Option A CP1 APPROVED by the user on 2026-10-22 (“A”); C not selected.** Evidence: [debug report](../debug/debug-261022-oom-endless-typed-pipeline.md) (**Confidence: CONFIRMED**, hermetic reproduction on the real `typed.rs`; the throwaway crate and its target dir were not kept). Results below.

## Root cause (from the report)
`top --records --frames 100000 | to jsonl` with no external tail takes the local-sink path: `encode_all` (`cells/tools/cellbox/src/typed.rs:704,718-730`) appends every row to one `Vec<u8>` and checks `MAX_SINK_BYTES` (4 MiB, `typed.rs:40`) only after each append. cellbox has no custom heap, so it runs on ostd's default **1 MiB** arena (`libs/ostd/src/heap.rs:28`); the allocator has no `realloc`, so a `Vec` doubling needs old+new capacity live. The logged failing size 892928 = 109 x 2^13 matches exactly; the 1.34 MiB doubling cannot fit and `alloc_error` exits the cell. Output reaches the terminal only after the pipeline returns, so nothing is shown first. With a 16 MiB heap the same code returns the designed `output exceeds the 4194304 byte bound`. The table sink OOMs the same way at 2580 rows, below its 4096-row bound. The local sink also never polls Ctrl-C, so raising the heap would only make the run uninterruptible.

Unaffected: `top | take 3`, `ps`/`ls --records | take N`, and any pipeline with an external tail (rows are pulled through `RowFeed` and forwarded live with Ctrl-C polling). The serial shell shares the executor, so it is affected too.

## Options
- **A (recommended): stream the local terminal sink.** Pull rows incrementally and write each encoded chunk to the terminal as it is produced, with the same Ctrl-C polling and exact-130 status as the external path; keep full materialization only where a redirect or a downstream byte consumer needs the whole buffer (bounded).
- **B (not recommended): raise the heap.** Delays the failure, leaves the run uninterruptible, enlarges every cellbox.
- **C (alongside A): fallible allocation with heap-aware bounds** so the remaining materializing sinks (redirect target, sort, `from` documents, `cat` input) return the designed bound error instead of aborting the cell. Larger blast radius; can be a separate follow-up.

## Proposed closure (A, and C only if you select it)
`cells/tools/cellbox/src/{typed.rs,executor.rs}` (+ `native_rows.rs` only if the pull API needs it) with host tests built on the real `typed.rs`: an endless scripted source never exceeds a small peak heap and stops on a simulated Ctrl-C with status 130; `take N` and redirects keep their exact output; the 4 MiB bound still returns its error for materializing sinks. A private-image check reuses the S3 TerminalHost driver (new scenario: endless `top | to jsonl` with no tail, Ctrl-C returns the prompt with `$?=130`, shell alive, heap does not abort). No kernel, ABI, ceiling, launch-profile or signing change.

## Not in this proposal (reported separately)
1. x86 `GetTime` returns nanoseconds while `TIMER_HZ` assumes 10 MHz, so `top`'s "1 s" frame delay is about 10 ms; that is why the heap fills within seconds. Separate defect, separate CP1 (touches a shared timing assumption).
2. Why the GUI runner's typed line lost its `| /bin/p03-filter cat` tail in run2 is unexplained (the same line worked in later runs); possibly a dropped keystroke. Needs its own investigation before anyone changes the input path.

## Acceptance
- [ ] Endless `top --records | to jsonl` shows rows live and Ctrl-C returns the prompt with `$?=130`; no OOM, shell survives (private image).
- [ ] Hermetic host tests above pass; peak heap bounded for an endless source.
- [ ] Existing S3 (52), Phase03 (60), Phase02 (48), GUI (6x103) runners and host suites show no new failures; ceilings unchanged.

## Rollback
Revert the two files; no persisted state.

## Results (2026-10-22)
- Source: the terminal sink streams via `typed::stream_rows` + `ChunkSink` (`typed.rs`) and `TerminalSink` (`executor.rs`); redirects, byte consumers and `$(...)` captures keep the bounded `run_rows` path. Table default still buffers rows under `MAX_TABLE_ROWS`/`MAX_TABLE_HEAP_BYTES`, then streams lines.
- Host: `cargo test --offline -p app-cellbox --target x86_64-unknown-linux-gnu` 89/89, incl. new `endless_terminal_stream_is_live_bounded_and_interruptible` (endless source, one row per write, <128 B per write, Ctrl-C → 130, producer closed once), `terminal_stream_matches_collected_output`, `collected_sink_still_refuses_past_its_bound`.
- Private image (`scripts/qemu-phase04-terminal-test.sh`, TCG): exit 0, **314 checks / 56 driver markers**, incl. `endless-local-sink-{streams-live,ctrl-c-130,shell-alive,no-oom}`, prompt-bounded and UART no-OOM. First run failed only `no-oom-heap-exhausted-uart` as a false positive (case-insensitive `\bOOM\b` matched the marker name `no-oom`); the runner now matches `OOM` case-sensitively.
- Regression: Phase03 runner exit 0 (60 PASS); Phase02 runner exit 0. Terminal ceiling delta (`SetTimer`) is the existing informational line, cellbox ceiling unchanged.
- Not rerun: GUI 6x103 runner. Acceptance box 3 therefore stays partially open.
