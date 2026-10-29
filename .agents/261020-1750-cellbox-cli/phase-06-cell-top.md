---
phase: 6
title: "Cell-aware telemetry and top"
status: completed-private-evidence
priority: P1
tier: thinking
dependencies: [3, 4, 5]
---
# Phase 06: Cell-aware top

## Overview and requirements

Upgrade existing top, not a replacement fake monitor. CBX-17/18 require incarnation-aware Cell default view, task drill-down, honest sampling/memory/completeness and equivalent TUI/batch/record data.

## Architecture and related files

Reuse `cells/tools/shell/src/top.rs`, its renderer and `libs/text-engine/src/top*` through the P04 moved modules. Resolve actual task/Cell identity and resource ownership with the approved telemetry provider before editing `libs/api/src/abi/syscall.rs`, `libs/ostd` wrappers or kernel task telemetry. Kernel/API changes are owned and gated by P01, not app-private inferred fields.

## Implementation steps

1. Supply authoritative incarnation identity, task membership, timestamp/counter units and completeness/paging through an admitted provider. A name group or bare reusable Cell ID is insufficient. Add an approved versioned provider delta only if existing contracts cannot supply these facts.
2. Project Cell and task records separately. Count shared Cell heap once, distinguish per-task stack/segments from allocator committed capacity and never label unsupported values RSS.
3. Compute CPU deltas for the same incarnation/membership over valid monotonic intervals; represent new/reset/stale/unavailable samples explicitly. Apply the P01-defined core-equivalent or machine-normalized convention; no accidental per-task clamp on a multi-core Cell.
4. Replace the silent 64-row adapter ceiling with complete bounded paging/frame metadata or explicit truncation. Bound per-frame memory; filter/sort each sample, not an infinite history.
5. Expose finite observation and streaming record modes; implement top Ratatui view with field selection/sort and task drill-down. Batch mode is a renderer over the same numeric records, no key reads or ANSI in machine output.
6. Exercise a real multi-thread Cell with known work/idle intervals, a restart/ID reuse, denied telemetry and enough tasks to exceed the former cap. Compare all render modes for one recorded snapshot and verify no double-counted memory.

## Success criteria

- [x] Cell incarnations remain distinct across restart/reuse; drill-down returns their actual tasks. **Met (private QEMU/TCG, 1 vCPU):** `top-load-relaunch`/`top-load-id-reused` (Cell 11, generation 159 → 223; the drill shows only the new TIDs), `top-load-gone-cell-empty`, `top-load-tui-gone-notice`, `top-load-drill-live` (TIDs equal the fixture's own spawn list), `top-tui-drill`; host tests for generation reuse and Gone. [CP2 dossier](phase-06-cp2-dossier.md) §16.
- [x] Shared memory is not summed repeatedly; CPU convention and unknown samples are explicit. **Met (private QEMU/TCG), parts host-only:** `top-load-quota-once` (quota once per Cell row, in no task row), `top-load-cpu-idle/-busy/-two-busy/-sum-bounded`, `top-load-exit-retains-cpu`, `top-load-drill-live` (`new` first, then numeric), P03 `cpu_unit` `core_tenths` meta. `unavailable`/`reset` samples and a > 1000-tenth multi-core Cell are host-tested only (not reachable on the 1-vCPU HPET runners; D60-5).
- [x] Missing/denied/truncated telemetry is not presented as zero load or complete inventory. **Met (private QEMU/TCG), parts host-only:** provider denial `sandbox-denied-telemetry`; provider truncation `control-telemetry-truncated` (1 of 8 Cells, `complete=0`); 74 tasks complete in records, text, `ps` and the TUI (the 64-row cap is gone). `top`'s own denied and `TRUNCATED n` rendering is host-tested only (D65-1, D65-8 T-a).
- [x] Interactive, batch and typed consumers agree for the same snapshot; observation grants no lifecycle rights. **Met (private QEMU/TCG) for identity; numbers by the constructed host pair (D64-14):** `top-load-views-agree` (the same 32 TIDs in TUI, batch and records, from separate captures), `top-tui-drill`; `top-load-no-lifecycle` and `top-tui-no-lifecycle` (`k`/`K`/Delete change nothing). The numeric same-snapshot comparison is the D64-14 host test; no recorded-snapshot replay path exists.

## Verification

Keep deterministic delta/lifetime/aggregation/boundary tests and exercise the actual target provider plus real top interaction. Existing tests pinning headings are not proof. Record actual data provenance; no CPU-speed or physical hardware claim follows from QEMU work counters.

## Assumptions

Admitted providers can expose incarnation/membership/resource scope. Current ProcessInfoV2 lacks identity, so absence is an explicit prerequisite, not a parser workaround. Confirm exact approval/source before changes.

## Security and risk assessment

Cross-Cell observations are permissioned; do not broaden telemetry visibility by adding TUI. Rollback requires a matched consumer/provider/image version; never reinterpret new rows as old ABI bytes. Observation output is irreversible disclosure, so enforce scope before sampling.

## Deviation log

2026-10-20 — **Completion withdrawn; incomplete/blocked.** Telemetry/model/serialization source was added in `libs/text-engine::top`; the historical 43-host-test snapshot is retained as unit evidence only. Review identified fabricated Cell/incarnation identities rather than an authoritative admitted telemetry provider. Model aggregation tests do not establish real identity, memory attribution, completeness, counter units, live sampling or task drill-down. No target-provider or actual top interaction proof closes CBX-17/18. Design consent is preserved; owner/allocation/provider admission is incomplete and CP2 pending/not eligible. All criteria remain unchecked and rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md).

2026-10-23 — Entry dossier drafted (no source change): [phase-06-entry-dossier.md](phase-06-entry-dossier.md). Entry (D60-1), slice order and S6-1 CP1 (D61-*) await user decisions.

2026-10-23 — **Phase 06 entered (user decision).** The user read the [entry dossier](phase-06-entry-dossier.md) and accepted **every §8 recommendation**:
- D60-1 enter with Phase 05 = implemented-private-evidence and Phase 04 = in_progress (criterion 4). Phase 05 CP2 was approved the same day ([dossier](phase-05-cp2-dossier.md) §16), so D60-2 is satisfied: the Phase 05 hashes were frozen before any Phase 06 source edit.
- D60-3 10 MHz `sleep`/`uptime`/`date` recorded as a separate defect with its own CP1. D60-4 order S6-1 → S6-2 → S6-3 → S6-4 → S6-5. D60-5 no multi-hart lane (>100% Cell CPU stays host-test only). D60-6 assistant as integrating owner of the S6-1 kernel/API/ostd file set.
- D61-1 K1 exact on-CPU ns; D61-2 A1 per-incarnation `exited_cpu_ns`; D61-3 P1 one-shot caller buffer; D61-4 S1 System + X only; D61-5 E1 boot entropy, with the entry check and E2 fallback; D61-6 syscall 430 on allowlist bit 55, X gate, M for the memory block; D61-7 DV-1..DV-8 ratified; D61-8 S6-1 CP1 approved as written in §5.
- D63-1 F1 spec Frames; D63-2 envelope shape as in §4.2; D63-3 M1 spec §1 amendment (optional Begin `meta`).
- D64-1 R1 one schema per frame; D64-2 CLI and key map as in §4.3; D64-3 text `ps` moves to the provider; D64-4 columns and labels as in §4.3.
- D65-1 `s2tw-probe` denial plus a host test of the denied rendering; D65-2 feature-gated `cells/tests/top-load` kept as a runner fixture.

Status → `in_progress` (S6-1 first). Rollout containment stays in force. Evidence is private-fixture only. No source-text or wording-pinning tests.

2026-10-23 — **S6-1 implemented (kernel/API telemetry provider), private QEMU proof pending.** Built as approved in [entry dossier](phase-06-entry-dossier.md) §5: `TelemetrySnapshot = 430` on bit 55, gated by X; V1 header/Cell/task layouts in `api::telemetry`; exact on-CPU ns per task (K1); per-incarnation retained exited-thread CPU (A1); one-shot capture with explicit totals and truncation (P1); System scope (S1); boot-entropy provider epoch (E1). Five implementation deviations are recorded in dossier §10. Host tests: kernel 272, api 144, ostd pass. Bare-metal builds: x86 kernel with runner features, rv64/aarch64 kernels, cellbox/probe/driver. No runner pin changed. Criteria stay unchecked until S6-2..S6-5 and the private runs land; the terminal, P03, P02 and GUI runs, the AP lane and the K1 switch-overhead measurement are owed by the parent.

2026-10-23 — S6-1 runtime: terminal 399/399 (117, incl. sandbox-denied-telemetry/control-allowed-telemetry), P03 60/60, P02 48, kernel AP 38/38, GUI 259/259 (logs `reports/s6-1-evidence/`). Next: S6-2 telemetry model.

2026-10-23 — **S6-2/S6-3 CP1 approved (user).** The user read [phase-06-s2-s3-proposal.md](phase-06-s2-s3-proposal.md) and accepted every §9 recommendation: D60-7 implement S6-3 before S6-2 (amends D60-4's S6-2 → S6-3 order) and narrow S6-4 to the Ratatui interactive view, drill-down/keys and the `TuiSession` wait; D62-1 `ps --records` is one capture with cumulative `cpu_ns`, Null `cpu_tenths`, quality `new`; D62-2 text `ps` moves onto the shared renderer in S6-2 (keeps `-p`/`-s`); D62-3 DV-9..DV-11 ratified; D63-4 `to json`/`to text` refuse every Frames stream (DV-12); D62-4/D63-5 both CP1s approved as written. Implementation order: S6-3 → S6-2.

2026-10-23 — **S6-3 then S6-2 implemented; private QEMU runs pending.** Engine Frames (Begin `meta`, `take --frame`, `sort-by --frame`, admission by category, order guard, JSONL envelopes; spec §1/§2 amended) and the `TelemetrySnapshot` model/sampler/records/text renderer replacing the old `text_engine::top` (fabricated `build_cell_records`, 64-row caps and the 100% clamp deleted); `top`, `ps`, `top --records`, `ps --records` cut over, blocking `SetTimer` waits, exit 2 on bad options. P03 and terminal-driver checks migrated (P03 60 → 62, terminal stays 399). Host: text-engine 105, app-cellbox 105; bare-metal cellbox (also `shell_test`) and driver build; ceiling gate G-S1a PASS. Files, nine implementation deviations and expected totals: [proposal §10](phase-06-s2-s3-proposal.md). Criteria stay unchecked.

2026-10-23 — S6-3+S6-2 runtime: terminal 399/399, P03 62/62 x2 (after runner fixes: USER: mid-line prefix reassembly; victim = init), P02 48, GUI 6x259 + 1 post-sleep-fix 259 (logs `reports/s6-23-evidence/`). Pending user ratification: init as kill victim; sleep fix landed despite D60-3. Open defect: console USER: prefix inserted mid-line at 512-byte record boundaries. Next: S6-4 CP1.

2026-10-23 — **User ratified** (proposal §10.3): (A) P03 kill victim = `init` by name; (B) the `sleep` fix stays landed despite D60-3 (`uptime`/`date` stay in D60-3). User approved fixing the `vfs` idle busy-loop (~98% CPU at idle in `top`).

2026-10-23 — VFS idle fix (user-approved, outside S6 scope): the receive loop blocks in attested `Recv` unless `prepared_cwd::idle_work` reports an unsettled prepared-cwd reservation, then keeps the nonblocking receive + 128-turn reconcile. No kernel/ABI change. Host vfs 78 (+2), bare-metal vfs (also `test-hooks`) builds. Private QEMU runs pending; expected: `vfs` Waiting and near 0% CPU at idle in `top`/`ps --records`.

2026-10-23 — VFS idle fix, external-child follow-up: Phase 03 QEMU (`/tmp/cellbox-phase03-1qUNkO/evidence/serial.log`) showed `vfs` at 0% idle but ~98.6% while an external filter ran. Cause: `idle_work` required the committed child to be watched, and the kernel watches only the current receive principal, so a child that never sends to VFS kept the cadence polling for its lifetime. Now a proven-running child is settled while it or its owner is watched; the terminal SDK's cwd lease contacts VFS when a reaped child is dropped (`CloseDir`, or `ResolveCwd` action 0 for a grant-derived cwd), so the receipt is settled before the Operation closes; owner death still triggers polling until the cancelled child ends. No kernel/ABI change. Host vfs 79, terminal-sdk 52; bare-metal x86 vfs (+`test-hooks`) and cellbox build. QEMU re-run pending (expected: `vfs` ~0% during `top --records … | /bin/p03-filter cat`).

2026-10-23 — VFS idle fix verified: vfs cpu_tenths 0 in all 9 P03 top frames incl. during external /bin/p03-filter children (was ~986), no close-failed lines; P03 62/62, terminal 399/399, P02 48, GUI 6x259 (logs `reports/vfs-idle-evidence/`). Next: S6-4 CP1 decision (`phase-06-s4-proposal.md`).

2026-10-23 — **S6-4 CP1 approved (user).** The user read [phase-06-s4-proposal.md](phase-06-s4-proposal.md) and accepted every §9 recommendation: D64-5 S1 (`TuiSink` to the relay device under the session guard, `ModeTracker` + `SOFT_RESET` net); D64-6 W1 (keep the tick-polled `TuiSession` wait, idle cost measured); D64-7 Z1 (`TuiSession::enter_bounded`, clamp 120×40); D64-8 GUI TrueColor, other frontends Ansi16; D64-9 no interactive terminal → status 2 with a `-b` hint; D64-10 Ctrl-C → 130 + `^C`; D64-11 T1 drill-down CPU `new` for one interval; D64-12 interactive `-d` floor 0.1 s (status 2); D64-13 frontend detach → leave the TUI, status 1; D64-14 constructed same-snapshot pair; D64-15 fixture-free TUI runner checks land with S6-4, fixture-dependent ones in S6-5; D64-16 binary size recorded, no budget; D64-17 drain type-ahead, ≤ 8 lease tries; D64-18 CP1 approved as written. The same day the user approved fixing the kernel console defect (`USER: ` record prefix inserted mid-line at 512-byte write-record boundaries) as a separate change.

2026-10-23 — **S6-4 implemented; private QEMU runs pending.** Interactive `top` is a vitui/Ratatui view (keys, drill-down into `ID:GEN`, field picker, `o`/`r` sort, `q` → 0, Ctrl-C → 130 + `^C`, no lifecycle key) drawn through `executor::with_tui`/`TuiSink` (S1) on a viewport capped at 120×40 by the new `TuiSession::enter_bounded` (Z1); `x=$(top)`/no session and `-d` < 0.1 s refuse with 2; the interim ANSI redraw and fd-0 poll are deleted. The `text.rs` table model is shared with batch, whose output stays byte-identical. Host: text-engine 107, vitui 23, app-cellbox 121; bare-metal cellbox (+`shell_test`) and driver build; G-S1a PASS; release `cellbox` text 1,094,251 → 1,334,827 B. Runners: terminal 399 → 410 (11 `top-*` markers on a new driver grid model, `frontend-top-quit` migrated), GUI 259 → 279 per run (`TOP-0..5`), P02 serial `top` step extended (48 markers + 6 runner checks). Files, 12 implementation deviations (incl. the idle-cost threshold < 500 tenths for ratification) and expected totals: [proposal §10](phase-06-s4-proposal.md#10-results-implemented-2026-10-23-host-tests-and-bare-metal-builds-only-private-qemu-runs-pending).

2026-10-23 — Kernel console prefix defect fixed (user-approved, separate from S6-4). Cause: `emit_user_record` reset the console owner after an inner `\n`, and the record's unterminated tail never claimed the line back. The open line then looked fresh, so the next 512-byte relay record got `USER: ` mid-line and a kernel line was spliced in with no break. Now `uart.rs` `emit_record` sets the owner from each fragment. A kernel line that interrupts a Cell line breaks first, and the Cell continues on a line tagged `USER+ ` (parent's decision: a distinct marker so parsers can rejoin exactly). P03 `relayed()` is removed: `cell_lines`/`body_of` drop kernel lines, strip `USER: ` at line start and join `USER+ `, and the frame checks use `rows()`. The P04 terminal UART, `bench_results.py`, x86 c2c and VMware parsers also rejoin `USER+ `. Verified: host uart tests 7/7; kernel builds pass for x86_64 (`phase02-terminal-profile,phase03-filter-profile,phase04-driver-profile`), riscv64, aarch64 and aarch64 `board-rpi3`. QEMU reruns (P03/P02/terminal/GUI) are pending with the parent.

2026-10-23 — Terminal run `cellbox-phase04-JSM1uq`: `top-subst-refused` and `top-tui-frame` failed. (1) Product: `$(…)` runs only `echo`/`cat`/`vcat`/`pwd` (`run_capture`) and substituted every other command as empty with status 0, so `x=$(top)` never reached the §2 gate. Now an interactive-class `top` runs under its own `CaptureGuard` (depth 2), where `tui::run` refuses (2, `-b` hint, nothing drawn); a failed substitution carries its status (`SubstFailed`, was always 1) and the assignment does not run. Batch `top` forms in `$(…)` stay unsupported/empty like other unlisted commands. `shell_test` gains the status and no-assignment checks. (2) Driver: `TUI_ENTRY` expected `ESC[2J`, but the bounded (fixed) Ratatui viewport (D64-7) clears row by row with `CUP y;1`+`ED 0`; the first step `ESC[1;1H ESC[0m ESC[J` clears the whole screen. The driver now requires that exact sequence after the modes (vitui test `bounded_entry_clears_from_home_row_by_row` pins the bytes) and prints `layout=` ahead of the 200-char clip. Host: app-cellbox 121, vitui 24; bare-metal cellbox (+`shell_test`) and driver build. QEMU rerun pending with the parent.

2026-10-23 — S6-4 + UART console fix runtime: terminal 410/410 (128 markers), P03 62/62 (0 mid-line USER: in raw serial), P02 48, kernel AP 38/38, GUI 6x279 incl. TOP-0..5 last column (logs `reports/s6-4-evidence/`). Pending ratification: idle-cost threshold 500 tenths. Next: S6-5 CP1.

2026-10-23 — **User ratified (S6-4):** the `top-tui-idle-cost` threshold stays 500 tenths (50% of one core); Ctrl-C in interactive `top` exits 130 (with `^C` after restore); the field picker shows and applies `cell_id` where the table shows `ID:GEN`. Recorded in [phase-06-s4-proposal.md](phase-06-s4-proposal.md) §10.2.

2026-10-23 — S6-5 CP1 drafted (no source change): [phase-06-s5-proposal.md](phase-06-s5-proposal.md) (SHA-256 `88602bb00841660fe96e73a89553f038edbb2195ecad51f64e0571eb64c60fce`). It covers the `top-load` fixture, 20 terminal-runner scenarios and the criteria→evidence map. The known gap that `&` is refused on every session shell is avoided by running separate driver hosts. Recommended placement is `/data/bin/top-load` (EMPTY), which needs no kernel/launch_profile change and no digest re-pin (DV-13). D65-3..D65-12 await the user.

2026-10-23 — **S6-5 CP1 approved (user).** The user accepted every §9 recommendation of [phase-06-s5-proposal.md](phase-06-s5-proposal.md):
- D65-3 P-a: `/data/bin/top-load` with an EMPTY ceiling, no kernel or launch_profile change, no digest re-pin; **DV-13 ratified**.
- D65-4 H1: separate driver hosts; the `&` gap stays.
- D65-5 C1: line commands with acknowledgements; C2 is the fallback.
- D65-6: thresholds as listed.
- D65-7: two `top-load 31` instances.
- D65-8 T-a: `s2tw-probe trunc`.
- D65-9: `P04 INFO` value lines echoed by the runner.
- D65-10: absolute per-call `bench`, no budget.
- D65-11: no new P02/P03/GUI checks.
- D65-12: CP1 as written.

2026-10-23 — **S6-5 implemented; private QEMU run pending.**
- **Line delivery.** Typed lines reach a foreground child as canonical session events (`from_own_context` + `next_event`; P02 `ctx` evidence), so C1 is kept and the C2 fallback was not built.
- **Fixture.** New `cells/tests/top-load` (`app-top-load`), packaged by the private terminal runner only, at `/data/bin/top-load`. Commands: `busy K`, `idle`, `exit K`, `ping`, `bench N`, `quit`, each answered by one `TOPLOAD` line.
- **Probe.** `s2tw-probe trunc` makes one capture into a header + one Cell row buffer.
- **Driver.** `top_load` adds 20 markers (#1–#20). Every compared value is printed as `P04 INFO top-load …`.
- **Runner.** Builds, signs and maps the fixture, records its provenance, echoes the INFO values with `top-tui-idle-cost`, and raises the boot window to 1800 s.
- **Unchanged.** Kernel, API, ostd, terminal SDK, cellbox and the pinned launch_profile files.
- **Host tests.** top-load 6, s2tw-probe 2, terminal driver 10.
- **Builds.** Bare-metal x86_64 (runner flags) and aarch64 builds of the fixture, driver and probe; riscv64 clippy is clean for them; `bash -n` passes.
- **Expected terminal run:** 430/430 checks, 148 markers. P03 62, P02 48, GUI 6×279 and AP 38/38 unchanged.
- **Records.** Files and 13 implementation deviations are in [proposal §10](phase-06-s5-proposal.md#10-results-implemented-2026-10-23-host-tests-and-bare-metal-builds-only-private-qemu-run-pending). Notable ones: `-a` where Retiring rows matter; #4 compared with captures taken just before and after; #19's UART part covered by the existing log-wide checks plus both fixtures reaching `bye`.

Criteria stay unchecked until the run.

2026-10-23 — S6-5 runtime (first run): terminal 430/430 (148 markers); top idle 3.1% (31 tenths, threshold 500); GetTime 1.7 us, Yield 403 us per call (TCG); provider trunc proof 1/8 cells, complete=0; P03 62, P02 48, GUI 279 (logs `reports/s6-5-evidence/`). Next: Phase 06 criteria review + CP2 dossier.

2026-10-24 — **Criteria review and CP2 dossier** (docs only, no source change): [phase-06-cp2-dossier.md](phase-06-cp2-dossier.md), one approval line per slice (S6-1, S6-3, S6-2, S6-4, S6-5) and per extra fix (VFS idle, UART console, `sleep`, `$(…)`). All four criteria are checked against the archived private QEMU/TCG runs, with the host-only parts named; steps 1–6 are reviewed in dossier §16. Status → `implemented-private-evidence`. CP2 is **not** approved; deviations without a ratification line are marked pending-with-CP2 in the dossier. Rollout stays blocked under [containment](reports/rollout-containment.md). Stale docs, not edited here (dossier §18): CHANGELOG S6-3/S6-2, S6-4 and S6-5 entries still say "runner proof pending"; [plan.md](plan.md) row 06 still says "incomplete/blocked"; the [command catalog](command-catalog.md) `ps` row still cites 64 rows; CHANGELOG has no entry for the `$(…)` status change.

2026-10-24 — Final-tree evidence gap closed: GUI 6/6 at 279/279 and kernel AP 38/38 on the S6-5 tree (`reports/s6-5-evidence/`). CP2 dossier finding 4 (first item) resolved.

2026-10-24 — **CP2 APPROVED by the user for all nine Phase 06 items** (S6-1, S6-3, S6-2, S6-4, S6-5, VFS idle, UART console, `sleep`, `$(…)`) at the [dossier](phase-06-cp2-dossier.md) §3 hashes (§20 record, option A). The approval ratifies every deviation still pending in dossier §6–§14, accepts the out-of-CP1 changes (general `$(…)` status propagation, the terminal-SDK `CwdLease` edits to Phase 05-hashed files, the `USER+ ` format and the four non-runner script edits) and the 15 Phase 05 baseline drifts. Gap-closure logs (GUI 6/6 at 279/279, kernel AP 38/38) are hashed in dossier §5. Stale docs reconciled (dossier §18: CHANGELOG, plan.md row 06, command-catalog `ps`/`top`, proposal §10 headings). Status → `completed-private-evidence`. Rollout stays **BLOCKED** under [containment](reports/rollout-containment.md); CBX-17/CBX-18 stay pending (Phase 10).
