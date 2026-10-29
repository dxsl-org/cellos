# Phase 06 slice S6-5 — load fixture and runtime proof (CP1 proposal)

Status: **APPROVED (CP1, user, 2026-10-23) — every §9 recommendation accepted:** D65-3 P-a `/data/bin/top-load` with an EMPTY ceiling, no kernel or launch_profile change, no digest re-pin, **DV-13 ratified**; D65-4 H1 (separate driver hosts, the `&` gap unchanged); D65-5 C1 (stdin line commands with acknowledgements, C2 as the pre-agreed fallback); D65-6 thresholds as listed; D65-7 two `top-load 31` instances; D65-8 T-a (`s2tw-probe trunc` + the existing host rendering tests); D65-9 `P04 INFO` value lines echoed by the runner; D65-10 absolute per-call `bench`, no budget; D65-11 no new P02/P03/GUI checks; D65-12 this CP1 as written. Implemented the same day (host tests and bare-metal builds; private QEMU run pending): §10. §1–§9 below are the approved text as drafted.

Drafted as: **DRAFT for user decision (CP1). No source change has been made.** Read-only investigation on the dirty working tree, 2026-10-23. Inputs: [entry dossier](phase-06-entry-dossier.md) §4 (S6-5 row), §7, §9; accepted **D65-1** (`s2tw-probe` provider denial + host test of the denied rendering), **D65-2** (feature-gated `cells/tests/top-load`, kept as a runner fixture), **D64-15** (fixture-dependent TUI checks move to S6-5); [S6-2/S6-3 proposal](phase-06-s2-s3-proposal.md) §4, §10; [S6-4 proposal](phase-06-s4-proposal.md) §7.2, §10 (with the 2026-10-23 ratifications); [phase-06-cell-top.md](phase-06-cell-top.md) steps and criteria; [acceptance.md](acceptance.md) CBX-17/18. Checks are behavioral only: no source-text, heading, label or wording pins. The fixture's own `TOPLOAD …` protocol lines are fixture data, not product wording.

## 1. Facts

### 1.1 What already has private QEMU evidence (S6-1..S6-4)
- **Provider** (S6-1): `sandbox-denied-telemetry` / `control-allowed-telemetry` PASS (`reports/s6-1-evidence/phase04-terminal.log:610,616`; again in `reports/s6-4-evidence/phase04-terminal.log:644,650`).
- **Frames/records** (S6-2/S6-3): P03 `top-frames-closed-by-take`, `frames-rows-stage-refused`, `sort-by-frame-live`, `typed-rows-stream-into-external-stage`, `take-closes-endless-producer` PASS (`reports/s6-4-evidence/phase03.log:444-467`), P03 62/62.
- **TUI** (S6-4, fixture-free): `top-pipe-batch`, `top-subst-refused`, `top-tui-frame`, `-refresh`, `-drill` (cellbox's own Cell; TUI TIDs = `top -b --cell` TIDs), `-keys`, `-no-lifecycle`, `-resize` (132×50 clamped, no `OOM:`), `-quit-restores`, `-two-sessions`, `-idle-cost` (< 500 tenths, ratified), `-ctrl-c-130` (ratified) PASS (`reports/s6-4-evidence/phase04-terminal.log:665-676`); terminal 410/410, 128 markers (`:971`). GUI 6 × 279 incl. `TOP-0..5` (`gui-*.log`, 20 `TOP-` lines each). P02 48 with the extended serial `top` step (`phase02.log:442`). VFS idle fix: `reports/vfs-idle-evidence/`.
- **Not yet shown at runtime:** a multi-thread Cell under known load, exited-thread CPU retention, Cell restart with ID reuse, Gone, more than 64 tasks, drill-down into a loaded Cell, provider truncation.
- **Evidence gap:** the driver prints `P04 PASS <name>` without values, and `P04 FAIL <name> <why>` only on failure (`cells/tests/phase04-terminal-driver/src/main.rs:105-113`). The measured idle-cost value behind `top-tui-idle-cost` is therefore in no archived log.

### 1.2 Known shell gap: `&` is refused on every session shell
- Both the physical serial shell and every frontend-held shell install `SERIAL_SESSION` (`cells/tools/cellbox/src/executor.rs:45-56,104-119`).
- `spawn_external` routes any session shell to `spawn_serial_foreground` (`executor.rs:2230-2233`). That path refuses a background launch: `shell: SDK background launch requires explicit nonterminal streams`, status 2 (`executor.rs:2966-2970`). Pipelines with `&` are refused earlier (`executor.rs:1050-1053`), and so are typed stages, stateful builtins and session commands such as interactive `top` (`typed.rs:626-639`).
- This conforms to CBX-09 (unsupported background forms fail before effects). It already cost P03 a blocked-fixture kill victim (S6-2/S6-3 proposal §10.3).
- **Consequence:** one shell cannot run `/…/top-load` and `top` at once. The design avoids it (§2) and does not change the shell.

### 1.3 Launch admission
- Admission is exact on caller, route and target path; an unlisted `/bin/X` gets **no launch** (`kernel/src/loader/launch_profile/targets.rs:119-138`; `profiles.rs:60-77`).
- Any `/apps/`, `/data/bin/` or `/home/` target is admitted with `CapSet::EMPTY`: no X/M/W, no network (`targets.rs:130-137`). `/data/bin/s2tw-probe` already uses this arm.
- The syscall allowlist is the ELF's own `__ViCell_syscalls` section (`kernel/src/loader/governed_spawn.rs:160`). It is not narrowed by the CapSet, so an EMPTY Cell may declare `Spawn` (threads) and `Wait`. Rights are checked separately (`kernel/src/task/syscall.rs:9571-9674`: only `GetProcs*`/`TelemetrySnapshot` need X, `MemInfo` needs M).
- **Pins:** `scripts/qemu-phase04-terminal-test.sh:252-265` hard-codes SHA-256 of `launch_profile/{mod,profiles,targets,tests}.rs` and `libs/api/src/abi/cell_session.rs`, and fails before build on a mismatch. `check-lookupservicebound-law1-digests.sh` and `check-ai-law1-digests.sh` do not cover these files. The P02, P03 and GUI runners record provenance only.
- **Fixture packaging:** `cells/tests/*` packages are plain workspace members with no Cargo feature. The gate is that only the private runner builds, maps and signs them with a throw-away key (`qemu-phase04-terminal-test.sh:292-324,403-405`).

### 1.4 Threads, CPU and identity
- `ostd::task::spawn(f) -> SyscallResult`. The kernel returns the new TID (`kernel/src/task/syscall.rs:5202-5211`). Workers are named `thread`. A worker that returns calls `sys_exit(0)`, which wakes a `Wait` joiner (`libs/ostd/src/task.rs:21-48`).
- `MAX_THREADS_PER_CELL = 32` counts the root, so a Cell has at most **31 workers** (`kernel/src/task/scheduler.rs:22,1015-1035`).
- K1 charges only the task that is current at a scheduler pass. A worker parked in `SetTimer` or `WaitCompletion` accrues nothing (`syscall.rs:6988-7002`; `completion_wait.rs:88-105`).
- The CPU-monopoly watchdog applies **only to RealTime-priority tasks** (`scheduler.rs:64-70,2317-2336`). A default-priority spinner is requeued, never killed.
- Cell IDs are the lowest free quota slot (`kernel/src/memory/cell_quota.rs:205-221`). The generation is global and monotonic (`kernel/src/task/tcb.rs:559-575`). A slot is reusable only after retirement leaves `Retiring` (`scheduler.rs:535-577`).
- Default `MAX_CELLS` = 64 → 63 allocable Cells × 32 = 2,016 Cell tasks. The cellbox reader grows once to ≤ 192 KiB (64 Cells + 2,048 tasks; `cells/tools/cellbox/src/top.rs:21-86`). **On the default image `top` cannot observe its own truncation** [INFERENCE from those bounds]. The provider truncates on the caller's `out_len` alone (`kernel/src/task/telemetry.rs:325-372`). `s2tw-probe` picks its own buffer (16 KiB, `cells/tests/s2tw-probe/src/main.rs:96-151`).

### 1.5 Model outputs the checks read
- Quality labels: `new`/`ok`/`reset`/`unavailable`. Text prints `N.n`/`new`/`reset`/`n/a` (`libs/text-engine/src/top/sample.rs:18-52`; `text.rs:86-93`).
- `gone_cells`/`gone_tasks` exist only in Begin `meta`, and are Null after an incomplete capture. End carries `complete`, `dropped` and `unavailable_fields` (`records.rs:10-25,160-180`).
- Gone counts are **system-wide**. The baseline marks every Cell and, in a task view, every task, before the view filter (`sample.rs:473-506`). `gone_tasks` is computed only while consecutive frames are task views.
- The batch header has `complete`/`TRUNCATED n` but no Gone count (`text.rs:298-325`).
- A gone `--cell ID:GEN` gives an empty table (batch) or an empty frame (records). Only the TUI shows a notice (`top/view.rs:195-204,497-508`).

### 1.6 Driver mechanics
- A `Term` is one `FrontendHost` with its own `/bin/cellbox sh` (`main.rs:170-228,1671-1687`). It has `send`, `run`, `wait_prompt`, `type_keys`, `press`, Ctrl-C, `resize` and `run_beside`, which pumps another host while one runs (`:227-299,1658-1701`).
- `top_tui` already keeps three hosts open at once (a, t, u). A typed line reaches a foreground child's stdin (`p03-filter hex`, `:750-758`).
- New markers go into the runner's `EXPECTED` list. Totals are computed, not hard-coded (`qemu-phase04-terminal-test.sh:600-670,819-847`).
- The P02/P03 serial runners and the GUI driver have one session each and no concurrent-launch facility (`qemu-phase03-pipeline-test.sh:429-439`; `phase04_gui_drive.py:610-640`).

## 2. Concurrency without `&` (D65-4, recommended H1)

**H1: one shell per role, each on its own driver host (recommended).**
- The load Cell runs in the **foreground** of its own host shell (L1, L2). The observer runs `top` (batch, records or TUI) in another host shell (O).
- Each shell is a separate Cell, so the foreground wait on L1 never blocks O. This is the same mechanism as `top-tui-two-sessions` and `two-hosts-independent-*`, with no shell change.
- The driver steers the fixture by typing lines into L1's foreground child (§3.2) and reads its acknowledgements from L1's output. A step that must overlap an observer stream is sent on O first (`send`), then performed on L1, then O's prompt is awaited (the `run_beside` pattern).

**Alternatives:**
- **H2:** co-schedule in one pipeline (`top --records … | /data/bin/top-load relay …`). It works on serial (P02/P03) [INFERENCE from `typed-rows-stream-into-external-stage`], but it ties the fixture's life to the producer's frames and cannot host the TUI.
- **H3:** a cellbox background launch with explicit nonterminal streams. That is a product change under CBX-09 and needs its own CP1. Not for S6-5.

## 3. Fixture `cells/tests/top-load` (D65-2)

### 3.1 Package and launch placement (D65-3, recommended P-a: no kernel change)

| Option | Image path | Rights | Kernel / pins |
|---|---|---|---|
| **P-a (recommended)** | `/data/bin/top-load` | `CapSet::EMPTY` through the existing sandbox arm | **No kernel or launch_profile change; no digest re-pin** |
| P-b | `/bin/top-load`, a `#[cfg(feature = "phase04-driver-profile")]` row in `targets.rs` with ceiling `CapSet::EMPTY` | EMPTY | `targets.rs` + `tests.rs` change (row, feature on/off tests), **re-pin those 2 entries** at `qemu-phase04-terminal-test.sh:254-255`, +kernel host tests |
| P-c | `/bin/top-load` as `LEGACY` (like `/bin/s2tw-probe`) | X/M/W | as P-b; over-privileged for a load generator |

- P-a deviates from the letter of D65-2/§4, which named a "private `/bin/top-load` launch row". The fixture needs no right: it observes nothing and writes no files. The sandbox arm adds no new authority, and the "feature gate" is the same one `/data/bin/s2tw-probe` has (only the private terminal runner builds, maps and signs it). This is **DV-13**, for ratification.
- New package: `app-top-load` (bin `top-load`), deps `api`, `ostd`, `terminal-sdk`, like `phase03-filter`. Manifest all false. Syscalls: `Send, Recv, TryRecv, RecvTimeout, Reply, Log, Exit, Yield, Heartbeat, LookupService, GetTime, SetTimer, Spawn, Wait`. Default priority (never RealTime, §1.4). Root workspace `Cargo.toml` member + `Cargo.lock`.

### 3.2 Behaviour (D65-5, recommended C1: stdin-driven phases)
`top-load W` (1 ≤ W ≤ 31) spawns W workers and prints `TOPLOAD ready workers=W tids=t1,…,tW`. It then reads **one command per line** from its delegated stdin and answers each with exactly one line.

| Command | Effect | Acknowledgement (printed after the effect holds) |
|---|---|---|
| `busy K` | workers 1..K spin (pure arithmetic loop, no syscall), the rest park | `TOPLOAD busy tids=…` after K workers report that they are spinning |
| `idle` | all workers park | `TOPLOAD idle` after every spinner has parked |
| `exit K` | the first K live workers return from their closures | `TOPLOAD exited tids=…` after the root has `Wait`-joined each, so they have left the live table |
| `ping` | none | `TOPLOAD pong live=L` |
| `quit` / end of stream | the root exits, the Cell retires | `TOPLOAD bye`, status 0 |
| `bench N` | N × `GetTime(0)`, then N × `Yield`, each timed on the monotonic clock | `TOPLOAD bench clock_ns=… yield_ns=…` (per call, D65-10) |

- A parked worker loops on `SetTimer(10)` (≈ 100 ms) and checks one shared atomic phase word, so a park costs ≈ 0 CPU and a phase change takes effect in ≤ 100 ms. A spinner re-reads the word every few thousand iterations.
- Unknown command → `TOPLOAD error`, nothing changes. Status: 0 normal, 2 usage, 3 stream failure, 130 cancelled (as in `p03-filter`).
- Bounds: ≤ 31 workers, one 64-byte line buffer, no allocation after start-up beyond the spawn boxes.
- **Fallback C2** (if typed lines prove not to reach a foreground child line by line, §8): an argv timeline (`top-load W busy:K:ms,idle:ms,exit:K,…`) with the same acknowledgement lines printed as each step starts. The checks key on the acknowledgements, not on wall-clock guesses.

### 3.3 Population for > 64 tasks (D65-7, recommended two instances)
L1 and L2 each run `top-load 31`, i.e. 32 tasks per Cell. That is 64 fixture tasks plus every shell, service and driver task, so `total_tasks > 64` by construction, without relying on the unmeasured base population. The alternative is one instance plus a `total_tasks > 64` precondition that fails closed.

## 4. Runtime scenarios (terminal runner; new driver function `top_load` after `top_tui`)

- Hosts: L1, L2 (load) and O (observer, 30×100, later 40×120). At most four hosts are live, `a` included. The `top_tui` hosts are closed first.
- Every number the checks compare is printed as `P04 INFO top-load <key>=<value>` (D65-9), and the runner echoes those lines into its log. This includes a repeat of the `top-tui-idle-cost` value.
- `ID:GEN` and TIDs are always taken from data: the fixture's `tids=` lines, `top --records` rows and the grid model.

| # | Marker | Steps | Asserts (fail-closed) |
|---|---|---|---|
| 1 | `top-load-launch` | L1, L2: `/data/bin/top-load 31`; O: `top --records --frames 1 \| to jsonl` | both `ready` lines; exactly two Cell rows named `top-load`, distinct `cell_id:generation`, `tasks` = 32 each |
| 2 | `top-load-over-64-records` | O: `top --records --tasks --frames 1 \| to jsonl`; `ps --records \| to jsonl` | `meta.total_tasks` > 64; task rows = `total_tasks`; End `complete:true`, `dropped:0`; `ps --records` rows > 64 and status 0 |
| 3 | `top-load-over-64-text` | O: `top -b -n 1 --tasks`; `ps` | header `complete`, > 64 data rows; text `ps` > 64 rows; each status 0 |
| 4 | `top-load-over-64-tui` | O: resize 40×120, `top --tasks -o tid`, PageDown until the highlight stops, `q` | last highlighted TID = max TID of #2's frame; distinct TIDs seen while paging = #2's row count; status line `complete`; restore bytes, `$?` = 0; no `OOM:` on the UART (heap proof at > 64 rows, S6-4 §6/§7.3) |
| 5 | `top-load-cpu-idle` | all parked; O: `top --records -d 2 --frames 2 \| to jsonl` | L1 and L2 `cpu_quality` `ok`, `cpu_tenths` ≤ 20 |
| 6 | `top-load-cpu-busy` | L1 `busy 1`; O: Cells frames `-d 2 --frames 2`, then `--cell L1 -d 2 --frames 2` | L1 ≥ 600 tenths; L2 ≤ 20; L1's own shell Cell (found the way `top-tui-idle-cost` finds its host's Cell) < 500; in the task frame the acknowledged busy TID has the highest tenths and ≥ 80 % of the frame's task sum |
| 7 | `top-load-cpu-two-busy` | L1 `busy 2`; O: as #6 | both busy TIDs ≥ 250 and within 2× of each other; L1 ≥ 600 (one vCPU: no > 1000 claim, D60-5) |
| 8 | `top-load-cpu-sum-bounded` | every Cells frame captured in #1, #5–#7, #13, #15 | Σ `cpu_tenths` over Cells ≤ `online_cpus` × 1000 + (number of Cells) (no CPU is billed to two Cells); during busy frames Σ ≥ 800 (the spinner leaves no unbilled CPU) |
| 9 | `top-load-drill-live` | L1 `busy 1`; O: `top`, `o` × 4 (sort `cell_id`), Down to L1's `ID:GEN`, Enter, wait 2 frames | title carries L1's `ID:GEN`; first drill frame CPU `new` (D64-11), next frame numeric; drilled TIDs = fixture `tids=` ∪ {L1 `root_tid`} with 32 rows; busy TID has the highest CPU; Esc → Cells view |
| 10 | `top-load-views-agree` | L1 `idle`; O: TUI drill as #9, `q`; `top -b -n 1 --cell ID:GEN`; `top --records --cell ID:GEN --frames 1` | same `ID:GEN` and the same TID set in TUI, batch and records (membership stable while parked). The numeric same-snapshot proof stays the D64-14 host pair |
| 11 | `top-load-no-lifecycle` | O: TUI, select L1's row, `k`, `K`, Delete, `q`; L1 `ping` | `pong live=31`; L1's TID set unchanged; L1 printed nothing between its previous acknowledgement and `pong` |
| 12 | `top-load-exit-gone-tasks` | L1 `busy 2`, wait 3 s; O: `send` `top --records --cell ID:GEN -d 1 --frames 4 \| to jsonl`; L1 `exit 1` (a spinner); O prompt | the first frame without the exited TID has `gone_tasks` ≥ 1 (exactly 1 when its `total_tasks` is the previous total − 1); the TID is in no later frame; no row `reset` |
| 13 | `top-load-exit-retains-cpu` | O: `send` `top --records -d 1 --frames 4 \| to jsonl` (Cells); L1 `exit 1` (the other spinner); O prompt | L1 `tasks` 31 → 30; L1 `cpu_ns` never falls and its quality stays `ok` across the exit (never `reset`); L1 `cpu_ns` after the exit ≥ Σ `cpu_ns` of L1's task rows in #12's last pre-exit frame. Without A1 the aggregate would fall by the spinners' CPU |
| 14 | `top-load-quota-once` | O: Cells + `--cell L2` records | `quota_bytes` appears once per Cell row and on no task row; L2 `quota_bytes` ≥ Σ `stack_bytes` of L2's `stack_in_quota` tasks and ≤ `limit_bytes` (stacks inside the quota, counted once) |
| 15 | `top-load-relaunch` | O: `send` `top --records -d 1 --frames 20 \| to jsonl`; L1 `quit` → prompt, `echo $?`; L1 polls `top -b -n 1 -a` until the old `ID:GEN` is absent; L1 `/data/bin/top-load 4`; O prompt | sequence: old row present → a frame without it with `gone_cells` ≥ 1 → the new incarnation's first frame `new` → a later frame `ok`; never `reset`; never both generations in one frame; fixture status 0 |
| 16 | `top-load-id-reused` | from #15 | new `cell_id` = old `cell_id`, new `generation` > old (fails closed with the observed ids if another launch took the slot) |
| 17 | `top-load-gone-cell-empty` | O: `top -b -n 1 --cell OLD`; `top --records --cell OLD --frames 1 \| to jsonl` | zero rows, `complete`, no row of the new generation; status recorded |
| 18 | `top-load-tui-gone-notice` | O: TUI drill into the new `ID:GEN`; L1 `quit`, poll as #15, relaunch; wait 2 frames; Esc | the drill view keeps its `ID:GEN` title, shows no task rows and no row of the third generation (structure only); Esc → Cells view lists the third generation |
| 19 | `top-load-teardown` | L1, L2 `quit` | `bye`, status 0; shells answer; no `OOM:`, watchdog or `CellFault` line on the UART during the scenario |
| 20 | `control-telemetry-truncated` (D65-8 T-a) | a: `/bin/s2tw-probe trunc` | `S2TW telemetry-trunc other:cells-E/T-tasks-0/U-complete-0` with E = 1 < T, decoded through `TelemetrySnapshot::parse` (whole rows, counts explicit) |

- Timing: busy phases ≤ 5 s, so the scenario adds an estimated 3–5 minutes of TCG time [INFERENCE]. The runner timeout is raised if needed.
- `bench` (D65-10) runs once on L1 before #1, with nothing busy. INFO only, no threshold.

## 5. Truncation and denial

**Truncation (D65-8):**
- **T-a (recommended).** `s2tw-probe` gains a first-argument mode `trunc`: one capture with tasks requested into a 192-byte buffer (header + one Cell row), printing one `telemetry-trunc` line.
  - Runtime proof: the **provider** reports truncation explicitly (`COMPLETE` clear, emitted < total).
  - Host proof (S6-2 `top/tests.rs`): its rendering — `TRUNCATED n`, End `complete:false` + `dropped`, Null Gone, `ps` refusal with status 1.
  - `top` itself cannot be made to truncate on a default image (§1.4) without a product test hook, which is rejected.
- **T-b.** Host-only, with the runtime gap recorded.
- **T-c.** A `cell-scale-experiment` lane. Rejected: it is measurement-only, needs a launcher for more than 64 Cells, and is a different kernel.

**Denied (D65-1, unchanged):**
- Runtime: `sandbox-denied-telemetry` (provider refusal from `/data/bin`).
- Host: S6-2 text tests (Denied → one message, status 1, no table) and the S6-4 `Denied seed → status 1, no terminal step` test.
- Nothing new in S6-5. A runtime denied `top` UI would need a restricted cellbox row, which D-TW2 declined.
- `mem denied`/`n/a` and CPU `unavailable`/`reset` are likewise not reachable at runtime: cellbox holds M; `cpu_ns` never decreases within an incarnation; HPET is present. They stay host-tested. #12, #13 and #15 add the runtime negative: exit and restart never produce `reset`.

## 6. Phase 06 success criteria → evidence

| Criterion / step | Existing evidence | S6-5 evidence (planned) | Residual |
|---|---|---|---|
| **C1** incarnations distinct across restart/reuse; drill-down returns actual tasks | Host: S6-1 generation-reuse ledger test; S6-2 "same `cell_id`, new generation → New, old Gone"; S6-4 gone-notice reducer. Runtime: `top-tui-drill` (cellbox's Cell, TUI = batch TIDs) | #9, #10, #15, #16, #17, #18 (TIDs checked against the fixture's own spawn results, independent of telemetry) | none if all pass |
| **C2** shared memory not summed; CPU convention and unknown samples explicit | Host: DV-4 tests (task bytes never summed), non-Ok never `0.0`, Unavailable/Reset/epoch-change. Runtime: P03 `cell_meta_ok` (`cpu_unit` `core_tenths`), `sort-by-frame-live` Null-last | #5–#8 (attribution, core-equivalent sum bound), #9 `new` then numeric, #13 retention without `reset`, #14 quota once | `unavailable`/`reset`/>1000 tenths host-only (D60-5; §5) |
| **C3** missing/denied/truncated not shown as zero load or complete inventory | Runtime: `sandbox-denied-telemetry`; host: Denied/OOM/Incomplete text and TUI seed exit; `TRUNCATED n` rendering | #2–#4 (> 64 complete in records, text, `ps` and TUI: the former silent cap is gone), #20 provider truncation explicit | `top`'s own truncation rendering host-only (T-a) |
| **C4** TUI/batch/typed agree for one snapshot; observation grants no lifecycle rights | Host: constructed pair (D64-14) TUI = batch = records with Ok/New/Reset. Runtime: `top-tui-drill`, `top-tui-no-lifecycle`; `kill` is `InSuiteRefused` | #10 identity agreement on a loaded Cell; #11 no lifecycle effect on a live loaded Cell | numeric same-snapshot stays a constructed host pair (D64-14; no capture-dump path exists) |
| Step 1 identity/membership/units/completeness | S6-1 provider + host tests; P03 meta checks | #1, #2, #16 | — |
| Step 2 Cell vs task projection | host DV-4; schema | #14 | — |
| Step 3 CPU deltas, new/reset/stale/unavailable, no clamp | host S6-2 | #5–#9, #12, #13, #15 | multi-core host-only (D60-5) |
| Step 4 no silent 64-row ceiling; bounded frames | host bounds; P03 frame checks | #2–#4, #20 | — |
| Step 5 finite/streaming records, TUI, fields/sort/drill | S6-3/S6-4 runtime set | #4, #9, #12, #15 (streams spanning live changes) | — |
| Step 6 multi-thread Cell with work/idle, restart/reuse, denied, > cap, cross-mode, no double count | partial (above) | whole §4 | "one recorded snapshot" is the D64-14 constructed pair (recorded deviation) |
| Verification "record data provenance; no CPU-speed/hardware claim" | runner `provenance.json` | `P04 INFO` values (D65-9); evidence ceiling stated with the run: TCG, 1 vCPU, HPET ns | — |
| Entry dossier §9 K1 hot-path cost (owed since S6-1) | not measured | `bench` (D65-10): per-call `GetTime(0)` (an upper bound of K1's one added counter read per pass) and `Yield` round trip, recorded | no before/after A/B (§9 alternative) |
| CBX-17 / CBX-18 | — | all rows above | CP2 for Phase 06 after S6-5 |

**Regression (unchanged suites):**
- terminal 410 + 20 → **430 checks, 148 markers** (fixed at implementation);
- P03 62, P02 48 (+ its 6 serial `top` runner checks), GUI 6 × 279, kernel AP 38/38.
- No new P02/P03/GUI checks (D65-11): those runners have one session and no concurrent launch (§1.6).

## 7. Files (planned)
- **New:** `cells/tests/top-load/Cargo.toml`, `cells/tests/top-load/src/main.rs`.
- **Changed:** root `Cargo.toml` (workspace member), `Cargo.lock`.
- **Changed:** `cells/tests/s2tw-probe/src/main.rs` (`trunc` mode, T-a).
- **Changed:** `cells/tests/phase04-terminal-driver/src/main.rs` (`top_load`, `P04 INFO`).
- **Changed:** `scripts/qemu-phase04-terminal-test.sh` (build `app-top-load`, map it to `/data/bin/top-load`, sign it, add 20 `EXPECTED` markers, echo `P04 INFO`, comment block).
- **Docs:** `CHANGELOG.md`, `phase-06-cell-top.md` log, this file §Results.
- **Unchanged under P-a:**
  - kernel, including `launch_profile/*`, so **no digest re-pin**;
  - `libs/api`, `ostd`, cellbox, text-engine, vitui;
  - the P02, P03 and GUI runners.
- **Under P-b:** add `kernel/src/loader/launch_profile/{targets,tests}.rs` and re-pin their two `authority` entries.

## 8. Risks and rollback
- **Typed input to a foreground child.** Bytes reach it (`p03-filter hex`), but line-by-line delivery before end of input is unproven [INFERENCE]. The first implementation run decides; if delivery fails, use C2 with identical checks.
- **One vCPU under a spinner.** The driver, input and shells slow down while a worker spins. Busy phases are capped at ≤ 5 s and ≤ 2 spinners; prompt waits use existing timeouts plus margin. The thresholds in D65-6 are conservative, and every measured value is logged (D65-9).
- **ID reuse depends on retirement finishing.** #15 polls `top -a` before relaunch, and #16 fails closed with the observed IDs rather than retrying blindly.
- **Membership drift during the > 64 comparisons** (a transient task between captures) would fail #4/#10 loudly with both TID sets in the detail. It is never tolerated silently.
- **Host count** (four live hosts) is not verified against any session cap [INFERENCE]. The fallback is L2 on host `a`.
- **Rollback.** Revert the slice's files together. Under P-a no kernel or pinned file is touched, there is no persisted state, and the fixture is never in an ordinary image.

## 9. Decisions requested (recommendations first)
1. **D65-3 Placement:** *P-a `/data/bin/top-load`, EMPTY, no kernel change, no re-pin; ratify DV-13* (recommended). Alternatives: P-b, a feature-gated `/bin` EMPTY row (re-pin `targets.rs`/`tests.rs`); P-c, LEGACY (not recommended).
2. **D65-4 Concurrency:** *H1 separate driver hosts; the `&` gap stays as is* (recommended). Alternatives: H2 pipeline co-scheduling; H3 a cellbox background launch (own CP1).
3. **D65-5 Phase control:** *C1 stdin commands with acknowledgements* (recommended), with C2 as the pre-agreed fallback.
4. **D65-6 Thresholds** (TCG, 1 vCPU), recommended:
   - idle ≤ 20 tenths;
   - one spinner: Cell ≥ 600, busy TID ≥ 80 % of the task sum;
   - two spinners: each ≥ 250 and within 2×;
   - parent shell < 500 (the ratified idle-cost value);
   - Σ Cells ≤ `online_cpus` × 1000 + Cells, and ≥ 800 while busy.
   
   Alternative: ordering-only checks (busy > idle) with values recorded.
5. **D65-7 Population:** *two `top-load 31` instances* (recommended) / one instance with a > 64 precondition.
6. **D65-8 Truncation:** *T-a `s2tw-probe trunc` provider proof + existing host rendering tests* (recommended) / T-b host-only / T-c scale lane (not recommended).
7. **D65-9 Provenance:** *`P04 INFO` value lines echoed by the runner* (recommended) / PASS-only as today.
8. **D65-10 K1 cost:** *`bench` absolute per-call `GetTime(0)` and `Yield` cost on the private image, recorded, no budget* (recommended). Alternative: an A/B against a pre-S6-1 kernel. It is confounded, because the HEAD baseline also lacks every other uncommitted Phase 02–06 change.
9. **D65-11 Other runners:** *no new P02/P03/GUI checks* (recommended) / add an H2 pipeline scenario to P03.
10. **D65-12** Approve this S6-5 CP1 (§2–§7) as written. After the run, Phase 06 criteria are checked against §6 and Phase 06 CP2 is prepared.

**Decision (user, 2026-10-23):** all ten recommendations accepted as written (see Status); DV-13 ratified.

## 10. Results (implemented 2026-10-23; private QEMU runs archived in `reports/s6-5-evidence/`; CP2 approved 2026-10-24)

Runtime results: [CP2 dossier](phase-06-cp2-dossier.md) §10 (per-scenario values) and §5 (final-tree logs: terminal 430/430, P03 62, P02 48, GUI 6/6 at 279/279, kernel AP 38/38). The 13 §10.3 deviations were ratified with CP2 on 2026-10-24.

### 10.1 Line delivery (D65-5): C1 kept, C2 not built
Code reading shows typed lines reach a foreground child line by line, so the C2 timed-script fallback was not implemented:
- In canonical mode the input owner queues typed text and keys as session events (`cells/services/input/src/session.rs` `admit_events`); only raw-mode bytes go to the child's stdin stream.
- The foreground child is the session's reader after `PrepareForeground`/`CommitForeground` (`libs/terminal/src/session.rs:235-264`). It polls those events with `TerminalSession::from_own_context` + `next_event` (`PollEvents`, 100 ms bounded waits).
- That path already has runtime evidence: P02 `phase02-session-child ctx` prints `got|<text>` for each typed line (`cells/tests/phase02-session-child/src/main.rs:120-165`).
- `top-load` therefore reads `Text` events up to an Enter key event (`api::session::key_code::ENTER`). End of input (`Eof`) acts as `quit`; `Interrupt` and a cancelled call exit 130. Nothing is echoed: the line editor that echoes is the shell's, and it is not reading while the fixture runs.

### 10.2 Files
- **New:** `cells/tests/top-load/{Cargo.toml,build.rs,src/main.rs,src/command.rs}` (`app-top-load`, bin `top-load`). Syscalls exactly as §3.1, manifest all false, default priority. Workers park in `SetTimer(10)` and spin in a syscall-free loop that re-reads the command word every 4,096 iterations. `exit K` joins with `Wait`; a worker already reaped counts as gone. `bench N` times N `GetTime(0)` then N `Yield` on the timer counter. Output lines are formatted into a fixed 512-byte buffer (no heap).
- **New:** `cells/tests/phase04-terminal-driver/src/records.rs` holds the pure readers and comparisons (JSONL envelope frames, `top.cell.v1`/`top.task.v1` rows, `TOPLOAD` acknowledgements, the probe `trunc` result, and the #8/#12/#13/#14/#15 evaluations). `src/top_load.rs` holds the scenarios.
- **Changed:** `cells/tests/phase04-terminal-driver/src/main.rs` (modules; `top_load::top_load(&mut a)` after `top_tui`; `cfg_attr(not(test), no_std/no_main)` for host tests) and `src/screen.rs` (`Frame::status()`).
- **Changed:** `cells/tests/s2tw-probe/src/main.rs`: mode `trunc` (§5 T-a), `cfg_attr` for host tests.
- **Changed:** root `Cargo.toml` (member `cells/tests/top-load`), `Cargo.lock` (`app-top-load`).
- **Changed:** `scripts/qemu-phase04-terminal-test.sh`:
  - S6-5 README block;
  - builds `-p app-top-load`, maps it only to `/data/bin/top-load`, signs it with the cells and checks it in the FAT layout;
  - adds `top-load` and an `s6_5_fixture` note to `provenance.json`, and an S6-5 paragraph to `acceptance-limits.txt`;
  - adds 20 `EXPECTED` markers;
  - echoes every `P04 INFO top-load …` and `P04 INFO top-tui-idle-cost …` line (joined across `USER+ `) into its output; these are not checks;
  - raises the default boot window from 1200 to 1800 s.
- **Docs:** `CHANGELOG.md`, [phase-06-cell-top.md](phase-06-cell-top.md) log, this §10.
- **Unchanged:** kernel (incl. `launch_profile/*`; no digest re-pin), `libs/api`, `ostd`, `terminal-sdk`, cellbox, text-engine, vitui; P02, P03 and GUI runners.

### 10.3 Implementation deviations (for ratification with the run)
1. **`-a` added** where Retiring rows matter: #2's task capture (`top --records --tasks -a`, so rows = `total_tasks`; without `-a` Retiring tasks are hidden but counted), #4's TUI (`top --tasks -a -o tid`), and the #12, #13 and #15 streams. A retiring row stays listed until it leaves the snapshot, so "the first frame without it" is the frame whose Gone count includes it.
2. **#4 baseline.** The TUI's TID set is compared with `top --records --tasks -a` captures taken on host `a` just before and just after the TUI. Both must be equal, and the TUI set must equal them. This replaces the comparison with #2's earlier frame and shortens the drift window; drift still fails loudly with the missing TIDs.
3. **#19 UART part.** The driver cannot read the UART. "No `OOM:`, watchdog or `CellFault`" is covered by:
   - the runner's existing log-wide `no-oom-heap-exhausted-uart` and `no-panic` checks;
   - both fixtures surviving to `bye` with status 0 (a watchdog kill or fault ends a fixture first).
   
   `CellFault` is an audit event, not a UART line. Recorded in `acceptance-limits.txt`; no runner check added, so totals stay 430/148.
4. **One TUI session for #9–#11** on O at 40×120, so the 32-row drill fits.
   - #10's drill starts after `idle` is acknowledged.
   - #11's `k`/`K`/Delete go to the Cells view with L1's row highlighted (checked) after #10's Esc.
   - #10's batch and records captures come after those keys, so they also give #11's "TID set unchanged".
   - Before `ping`, L1's transcript must be exactly the `idle` acknowledgement.
5. **Stream timing.** #12, #13 and #15 wait for the stream's first End before acting on L1, so frame 0 is pre-event. #12's floor for #13 is the Σ `cpu_ns` of L1's task rows in the last frame that still lists the exited TID.
6. **#8 busy frames** are the #6 and #7 Cells frames. #1, #5, #13 and #15 frames are bound-checked only.
7. **#14:** `quota_bytes` must occur exactly once in every Cell row (the key; L2's value must be non-null and within `limit_bytes`) and in no task row.
8. **#16** takes the new incarnation from the last #15 frame: the one `top-load` row that is neither the old L1 nor L2. If another launch took the slot, it fails with the observed IDs.
9. **#17** statuses are recorded (INFO), not asserted, as proposed.
10. **#20 runs first** on host `a`, before the load hosts open, and is independent of the fixture. The probe prints only the `telemetry-trunc` line (no `DONE`), and the driver requires status 0.
11. **INFO format.** One `P04 INFO top-load <marker> key=value …` line per marker, plus `launch` (one per host), `bench` and `identities`. A failed marker's line starts with `failed`. Each emitted marker is printed exactly once; a scenario that cannot run fails its markers as `not reached: <why>`.
12. **Fixture failure lines.** `TOPLOAD error timeout` (workers not settled within 30 s), `TOPLOAD error spawn` (status 3), `TOPLOAD error clock` (no timer frequency for `bench`).
13. **Host tests.** The fixture, probe and driver crates use `cfg_attr(not(test), no_std/no_main)` (as cellbox does) so their pure parts have host tests.

### 10.4 Verification (this change)
- `cargo test --target x86_64-unknown-linux-gnu`:
  - `-p app-top-load`: **6 passed** (worker count 1–31; commands bounded by live workers; unknown/extra words refused; bounded input line; no-heap output line incl. 31 TIDs; per-call ns arithmetic).
  - `-p app-s2tw-probe`: **2 passed** (a header + one Cell snapshot reads `other:cells-1/5-tasks-0/40-complete-0`; a length mismatch and a COMPLETE-with-missing-Cells header are `malformed`).
  - `-p app-phase04-terminal-driver`: **10 passed** (whole-key lookup, row parsing, envelope order, acknowledgements, probe counts, Σ-CPU bound, exit/Gone, CPU retention, relaunch sequence, quota once).
- Bare-metal release builds:
  - `x86_64-unknown-none` (runner flags) of `app-top-load`, `app-phase04-terminal-driver` and `app-s2tw-probe`: warning-free.
  - `aarch64-unknown-none-softfloat`: builds.
  - `cargo clippy --target riscv64gc-unknown-none-elf`: no warning in these three crates (dependency warnings pre-exist).
- `bash -n scripts/qemu-phase04-terminal-test.sh` passes, and all 10 embedded Python blocks compile. `EXPECTED` has 148 unique markers, and the driver's 20 S6-5 names equal the runner's.

### 10.5 Expected totals of the owed private runs (parent)
- **Terminal:** **430/430 checks, 148 markers** (410 + 20; no other runner check added). The `P04 INFO top-load` and `top-tui-idle-cost` values appear in the runner output.
- **Unchanged:** P03 62/62, P02 48 (+ its 6 serial `top` runner checks), GUI 6 × 279, kernel AP 38/38. None of those runners builds the fixture, and the probe's default mode is unchanged.
- After the run, Phase 06 criteria are checked against §6 and Phase 06 CP2 is prepared (D65-12).
