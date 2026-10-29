# OOM local-sink CP2 dossier: implementation review request

**Status: CP2 APPROVED by the user on 2026-10-22 (“duyệt bốn CP2”) for dossier SHA-256 `588fff6545c6fc34b0212dc60a0a3efaf1da5de8569976a2a0fa9ad4a1f2c7fe` (digest before this status amendment).** CP2 would make only the OOM local-sink delta's source the baseline. Quarantine, canonical packaging, release signing and rollout stay blocked.

Prepared 2026-10-22 under S4b of the [S4 retirement proposal](phase-04-s4-retirement-proposal.md). Sources: saved evidence and current disk bytes. The only new commands were `sha256sum`, read-only `git`/`grep`, and one hash of `shell_state.rs` with lines 92–97 removed (see the inventory). Nothing was built, tested or booted.

## Authority

- [OOM local-sink CP1](phase-04-oom-local-sink-proposal.md): Option A approved by the user on 2026-10-22 (“A”). Option C (fallible allocation for the materializing sinks) was **not** selected.
- Root cause comes from the [debug report](../debug/debug-261022-oom-endless-typed-pipeline.md) (CONFIRMED, hermetic reproduction). The terminal sink collected every row into one `Vec` before writing anything. cellbox runs on the 1 MiB ostd arena, and a `Vec` doubling at 892928 bytes aborted the cell.
- The previous CP2 baselines that touch this closure are [S3-gui](phase-04-s3-gui-cp2-dossier.md) (`executor.rs`) and [S3-output](phase-04-s3-output-cp2-dossier.md) (`shell_state.rs`, the Phase04 driver and runner). Their approvals cover their recorded hashes only and do not extend to the current bytes.

## Implemented delta

A typed pipeline whose sink is the live terminal now **streams**. Rows are pulled one at a time, each encoded row is written to the session stdout as soon as it is produced, and Ctrl-C is polled before every pull and while the terminal applies backpressure. Ctrl-C ends the pipeline with status 130 and closes the producer exactly once. Redirects, byte-consuming built-ins and `$(...)` captures keep the bounded collected path (`run_rows`). The default table still buffers its rows to align columns, bounded by `MAX_TABLE_ROWS` (4096) and `MAX_TABLE_HEAP_BYTES` (512 KiB), then streams the rendered lines.

Streaming is selected only when **all three** hold (`executor.rs:1420-1436`):
- the segment ends at the terminal with no redirect;
- a terminal session exists (`serial_active()`);
- the call is not inside an enclosing capture (`capture_depth() <= 1`).

`serial_active()` is true for the shell-created serial session and for context-mode sessions alike (`executor.rs:110-111,125-126,307-309`).

Behaviour change: before, a failing terminal pipeline showed no bytes. Now the rows already delivered stay on screen and the diagnostic follows, the same as the external-tail path (`typed.rs:890-891`).

No kernel, ABI, ceiling, launch-profile, manifest or signing change. Runner line 6: `G-S1a PASS: declare_manifest and declare_syscalls sets are identical` for cellbox.

## File inventory: current disk SHA-256

| Path | Bytes | SHA-256 | OOM regions (current lines) | Shared with |
|---|---:|---|---|---|
| `cells/tools/cellbox/src/typed.rs` | 81450 | `6c8eb69b72d202f367d1c28a83f5e1b57afaefab87f01bf122d7bed77cfd7796` | module doc 14–16; `MAX_TABLE_HEAP_BYTES`/`SINK_WRITE_MAX`/`INTERRUPTED_STATUS` 46–53; `SinkStop`, `ChunkSink`, `StreamEnd`, `Delivery`, `stream_encoded`, `stream_table`, `stream_rows` 743–918; table heap bound 1265–1336; `render_table` doc 1378–1379; tests `RecordingSink` + 3 tests 1830–1902 | Phase03 typed engine (CP2 approved 2026-10-21 **without per-file hashes**); no S2-K/S2-N/S2-T/W change claimed |
| `cells/tools/cellbox/src/executor.rs` | 142801 | `2ef99fa14d0ae9dc5f581925dc6a60cbfcd71c5bd0c445cda24bd841a5430531` | `LocalOutcome` 1125–1131; `TerminalSink` + `ChunkSink` impl 1163–1209; `flush_repl_capture` 1211–1223; local-sink dispatch and outcome handling 1420–1475 | **S2-K** (gate 2381–2383, `echo >`/`>>` status 1544–1589); S3-gui approved base; whether S4a's dead-code cleanup touched it is not recorded |
| `cells/tools/cellbox/src/shell_state.rs` | 8981 | `1e5e054b9a7b476f8b94ebac101a2a5ffc15f09c8343594088dd8f3b6ffe4163` | `take_capture` 92–97 (its only consumer is `flush_repl_capture`) | S3-output approved base |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 53581 | `0139d5cc3404d092d848ba756d73c680bbeeaa0eeea59c912625cf7166d8fe61` | `oom_text` + `endless_local_sink` 984–1060; call at 327 | **S2-K** (`secret_namespace`), S2-N (`net_rights`), S2-T/W, S4a (`sandbox-no-file`); S3-output approved base |
| `scripts/qemu-phase04-terminal-test.sh` | 42941 | `d4cdaadaffe48949d3122ca9ab81863b3732b8bce7f7de4d2dab382a0ee884a6` | expected markers 547–548; prompt-bounded check 658–659; UART no-OOM check with case-sensitive `OOM` 660–664 | **S2-K** (fixture 316–321, markers 549–550, 665), S2-N, S2-T/W (digest pins); S3-output approved base. S4a Results claim no runner change (mtime 17:26, before S4a's 18:14 edits) |

Not in the closure: `native_rows.rs` (`605ff51d…518d`). The proposal allowed it "only if the pull API needs it", and it references none of the new streaming items. `context_session.rs` equals its S3-gui hash `c2bc83b8…bff` (unchanged).

### Exact diff where it can be shown

- **`shell_state.rs`: exact.** It was 8749 bytes at the approved hash `20b4b89151562593003ebc10d81da82c2e51a6c4d219b64dcf737e11c3104ec4` and is now 8981 bytes (+232). Lines 92–97 (`take_capture`) are exactly 232 bytes. With those lines removed, the current file hashes to the approved `20b4b891…3ec4`. The only change since S3-output CP2 is therefore that inserted block.
- **`executor.rs`: not separable.** It went from the S3-gui hash `e123054d…88c1` (139053 bytes) to the current one (142801 bytes), a net +3748 bytes. That growth covers the OOM regions above, the S2-K regions, and possibly S4a cleanup. No pre-delta copy exists (the file is staged only at its HEAD rename content; no snapshot or patch was found), so a per-delta diff cannot be reconstructed from disk.
- **`typed.rs`: no git base and no prior hash.** It is untracked and absent from `HEAD`, and `HEAD` contains no `stream_rows`/`ChunkSink`/`TerminalSink`. The whole-file hash therefore also anchors Phase03 code that has never had a hash.
- **Driver and runner: not separable.** Their S3-output hashes were `ec167edc…c06b` (41380 bytes) and `bcce7aad…89aa` (34917 bytes). The current bytes mix OOM, S2-K, S2-N, S2-T/W and S4a changes.

## Caller census

| API | Consumers |
|---|---|
| `typed::stream_rows` | `executor.rs:1433` (only production caller); typed tests 1863, 1891 |
| `typed::ChunkSink` | implemented by `executor::TerminalSink` (1184) and test `RecordingSink` (1845) |
| `typed::StreamEnd` / `INTERRUPTED_STATUS` | `executor::LocalOutcome::Streamed` (1128, 1463–1467) → status 0 or 130; `^C` echoed via `shell_console` |
| `typed::run_rows` (collected path, kept) | `executor.rs:1435` (redirect, byte consumer, capture, no-session host); `shell_test.rs:1084,1095,1106,1117`; typed tests |
| `SINK_WRITE_MAX`, `MAX_TABLE_HEAP_BYTES` | inside `typed.rs` only (818; 1331–1334) |
| `state::take_capture` | `executor::flush_repl_capture` (1215) only |
| reused, pre-existing | `poll_session_interrupt` (1136; also used by the external path at 3315), `terminal_write_while` (233; also 264, 3265) |

Nothing outside `cells/tools/cellbox` consumes these items; a repository `grep` over `cells/` found no other users.

## Evidence

Final logs are `reports/s4-final-evidence/` (SHA-256 below). That run is the post-S4a tree, so it carries every later delta. The logs record the signed binary hashes and the `Cargo.lock` hash, but **no source hashes**. That this evidence matches the source hashes above is `[INFERENCE]` from two facts: the current `Cargo.lock` equals the run's `cargo_lock_sha256` `e58e780a…18f6` (terminal log 486), and every source file's mtime precedes the archived logs.

| Claim | Log line(s) |
|---|---|
| Rows reach the terminal while the pipeline runs (fresh rows after a 1.5 s pause, no prompt) | terminal log 564 `PASS marker:endless-local-sink-streams-live`; serial log 1509 |
| Ctrl-C returns the prompt with `$?=130` (`ccsink=130`) | terminal log 565 `PASS marker:endless-local-sink-ctrl-c-130`; serial log 1511 |
| Ctrl-C reaches the prompt within the 15 s bound | serial log 1510 `P04 INFO endless-local-sink elapsed_ms=17`; terminal log 856 `PASS endless-local-sink-ctrl-c-prompt-bounded` |
| Shell survives (`echo alive`) | terminal log 566 `PASS marker:endless-local-sink-shell-alive`; serial log 1512 |
| No OOM/heap-exhausted text on the terminal streams | terminal log 567 `PASS marker:endless-local-sink-no-oom`; serial log 1513 |
| No case-sensitive `OOM` or `heap exhausted` on the UART (framed lines and raw bytes) | terminal log 857 `PASS no-oom-heap-exhausted-uart` |
| Whole Phase04 terminal run | terminal log 878 `PASS: 353 Phase04 S3-output runtime checks (87 driver markers)`; serial log 1733 `P04-TERMINAL: PASS checks=87`; 353 `PASS ` lines, 0 `FAIL` lines |
| cellbox ceiling unchanged | terminal log 6 `G-S1a PASS`; lines 7–11 are the informational Terminal `SetTimer` delta approved earlier |
| Phase03 regression (serial shell, typed pipelines incl. `take-closes-endless-producer`, Ctrl-C) | phase03.log 397–456: 60 `PASS ` lines, 0 `FAIL`; 430–433 |
| Phase02 regression | phase02.log 407 `passes: 48`, 408 `orphan ticks: 4` |
| Earlier observation of the same scenario before S2-K | [s2k-baseline-before-fix.log](reports/s2k-baseline-before-fix.log) 460–463, 733–734 (OOM markers PASS; that run failed only on S2-K checks) |

The logs do not contain process exit codes. "Exit 0" for the Phase04, Phase03 and Phase02 runners comes from the [S4a Results](phase-04-s4-retirement-proposal.md) and the [OOM Results](phase-04-oom-local-sink-proposal.md).

**Host suite:** the OOM Results report `cargo test --offline -p app-cellbox --target x86_64-unknown-linux-gnu` 89/89, including:
- `endless_terminal_stream_is_live_bounded_and_interruptible`
- `terminal_stream_matches_collected_output`
- `collected_sink_still_refuses_past_its_bound`

S4a also reports 89/89. **No raw host-test log is archived**, so these counts rest on the Results text only.

| Evidence file | SHA-256 |
|---|---|
| `reports/s4-final-evidence/phase04-terminal.log` | `ad492a2f74e63e4a8c059e2bfba7d16bc3d7ce44444d89bca7ca3c051a5dbd43` |
| `reports/s4-final-evidence/phase04-serial.log` | `7153285123116ffebe2f919fa074d7c5f1b560830ffe21a99f13e8c962406f93` |
| `reports/s4-final-evidence/phase03.log` | `c46d7e3f79d1c203df26fe4279725818ad8879442609f71b853eb2659e500ec7` |
| `reports/s4-final-evidence/phase02.log` | `592a99e3d6236f7cbfb7f3546424a2c6f5381326f4445a9041b6f5486cfc69f2` |
| `reports/s2k-baseline-before-fix.log` | `ec61771422174f68e18cbd723c521e970439a7a592e9a6766d7e0fc27ba88ec3` |
| `../debug/debug-261022-oom-endless-typed-pipeline.md` | `50077659d6f4c12e3bd53292ac3522bf18cd64a9fcf09aef2e0f26a21fd0bae3` |

## Known limitations and what was not exercised

1. **GUI 6×103 runner not rerun.** The `executor.rs` bytes approved for S3-gui have since changed. Streaming under `CELLBOX_STREAMS=merged` in the GUI Terminal is unobserved. CP1 acceptance box 3 stays partially open.
2. **Serial-created session not exercised for the endless local sink.** The private-image check drives a context-mode TerminalHost session. Phase03 only covers forms with an external tail or `take`. With no terminal session at all, the collected path is used.
3. **Materializing sinks are unchanged (Option C not selected).** Redirects, `to <fmt> | <byte built-in>`, `$(...)`, `from`/`cat` inputs and byte pipelines still collect up to `MAX_SINK_BYTES` (4 MiB), which exceeds the 1 MiB arena. `[INFERENCE]`: they can still abort the cell, as the debug report analysed; `encode_all` (`typed.rs:729`) is still the collected encoder. `collected_sink_still_refuses_past_its_bound` runs on the host allocator and does not show the bound being reached inside the 1 MiB arena.
4. **"Peak heap bounded" is shown structurally, not measured.** The host test asserts one sink write per pulled row, each under 128 bytes, and 5000 newlines at Ctrl-C. That means `Delivery` hands every row on before the next pull. No allocator peak was measured on the host or on the image; the image shows only the absence of OOM text.
5. **No A/B image run.** No archived image run shows the endless-local-sink scenario failing on the pre-fix code. Its ability to distinguish the old code from the new rests on the debug report's hermetic reproduction and the original GUI `serial.log` it cites.
6. **Out of scope and still open.** The CP1 lists the first two; the debug report lists the third.
   - x86 `GetTime` ns vs `TIMER_HZ` 10 MHz (`top`'s "1 s" frame is about 10 ms);
   - the GUI runner losing the ` | /bin/p03-filter cat` tail in run2;
   - stale ostd heap comments.
7. TCG private image only; no KVM, physical or canonical-image evidence.
8. Docs not reconciled: `docs/cellbox-cli.md:104`, `CHANGELOG.md:10-11` and weakness 3 of the S3-gui dossier still describe the endless-pipeline OOM as open. This dossier does not edit them.

## Deviations from the CP1

- **Runner check corrected after the first run.** That run failed only `no-oom-heap-exhausted-uart`: the case-insensitive `\bOOM\b` matched the marker name `no-oom`. `OOM` is now matched case-sensitively (`qemu-phase04-terminal-test.sh:662-664`). The failed first run's log is not archived under `reports/`.
- **`shell_state.rs` is in the closure.** The CP1 named only `typed.rs`/`executor.rs` (+`native_rows.rs`). The delta added `take_capture` (exact 232-byte insertion, proven above) so that earlier REPL output is flushed before the streamed rows.
- **Default table stays aligned and bounded.** The debug report left "stream unaligned vs bounded aligned" open; the implementation keeps a bounded aligned table.
- **Counts moved with later deltas.** The OOM Results report 314 checks / 56 markers. The final archived run is 353 / 87 because it includes the S2-K, S2-N, S2-T/W and S4a additions.

## What CP2 would authorize

Accept the OOM regions listed above as the local-sink baseline:
- `typed.rs`;
- `executor.rs`, its OOM regions only;
- `shell_state.rs` `take_capture`;
- the driver's `endless_local_sink`;
- the runner's endless/UART checks.

The current whole-file hashes are recorded as the reviewed bytes. Because `executor.rs`, the driver and the runner are shared, those three whole-file hashes become a CP2 baseline only once **this and the S2-K CP2** are both approved, together with S2-N/S2-T/W for the driver and runner. Approving this dossier alone authorizes its regions, not the other deltas' bytes in the same files. The driver also carries S4a's `sandbox-no-file` change. S4b has no dossier for S4a; S4a source rests on the S4 CP1 approval, and the user must decide whether that is enough for a whole-file baseline. This CP2 does **not**:
- lift quarantine;
- authorize canonical packaging, release signing or rollout;
- accept Option C's remaining materializing-sink risk;
- close Phase04 criteria.

## Options

A. **Approve this CP2** for the OOM regions at the hashes above.

C. **Not yet**: keep the implementation and evidence and withhold CP2. A GUI rerun or a serial-session endless check can be requested first.

No approval has been recorded.

## Rollout and rollback

Rollout stays **BLOCKED** under [containment](reports/rollout-containment.md). Nothing is unblocked.

The CP1 rollback ("revert the two files") no longer applies as written, because every file is shared. A rollback must remove the OOM regions **as a matched set**:
- `typed.rs`'s streaming section and tests;
- `executor.rs`'s `LocalOutcome`/`TerminalSink`/`flush_repl_capture` and the streaming branch, returning to collect-only;
- `shell_state.rs` lines 92–97;
- the driver's `endless_local_sink` call and function, plus the runner's markers 547–548 and checks 658–659.

Removing source without the driver/runner checks makes the runner fail, and vice versa. The case-sensitive UART `OOM` match (660–664) is harmless to keep. Rolling back brings back the demonstrated 1 MiB-arena abort on endless terminal pipelines. There is no persisted state, and bytes already delivered to a terminal cannot be recalled.
