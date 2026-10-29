# Phase 06 entry dossier — Cell-aware telemetry and `top`

Status: **DRAFT for user decision (CP1 inputs). No source change has been made.** Phase 06 entry, slice order and S6-1 CP1 all wait on §8. Read-only investigation on the dirty working tree (HEAD `3471028bc`), 2026-10-23.

Inputs: [phase-06-cell-top.md](phase-06-cell-top.md), [spec.md](spec.md) §1/§3/AC-12, [abi-vfs-telemetry.md](abi-vfs-telemetry.md) §1, §5–§7 (D04), [command-catalog.md](command-catalog.md) `top`/`ps` rows and rights X/M/T, [contract-deltas.md](contract-deltas.md) D04 rows, [S2-T/W annex](phase-04-s2tw-annex.md), [acceptance.md](acceptance.md) CBX-17/18.

## 1. Entry gates

| Gate | State | Blocks entry? |
|---|---|---|
| Phase 03 (dependency) | completed, CP2 approved 2026-10-21 | No |
| Phase 04 (dependency) | `in_progress`, criterion 4 only (canonical packaging, held by rollout containment); every Phase 04 delta has CP2 | No. D-CL3 already let Phase 05 enter in this state, and Phase 06 does not need criterion 4 |
| Phase 05 (dependency) | `implemented-private-evidence`: all 5 criteria checked within private QEMU/TCG (terminal 397/397, GUI 6×259, P03 60, P02 48, kernel AP 38/38, `reports/phase-05-final-evidence/`); **CP2 pending** for S5-1, S5-2, option C, S5-3, S5-4 and the renames | **Not by plan rule.** `plan.md` §"Entry gate" item 4 needs CP1 before a delta's source edit, and CP2 before *final caller/image retirement or rollout for that delta*. Nothing in the plan makes a predecessor's pending CP2 block a successor's design or private-fixture implementation. But the DAG lists 05 as a dependency, and the D-CL3 precedent was an explicit user decision, so **entry needs D60-1** |
| Overlap with pending Phase 05 CP2 | Phase 06 edits files that the Phase 05 CP2 dossier must hash: cellbox (`top.rs`, `native_rows.rs`, `typed.rs`, `commands.rs`, `Cargo.toml`), and maybe `libs/vitui` | Not a block, but an ordering risk: if Phase 06 edits first, Phase 05's "exact SHA-256 of final files" is a moving target. See D60-2 |
| Rollout containment | in force (`reports/rollout-containment.md`): no canonical packaging, signing lift or quarantine change | Unchanged by Phase 06; all evidence is private-fixture only |
| D04 CP1 prerequisites (`abi-vfs-telemetry.md` §6, D04 row) | (a) named kernel/API/accounting owner and file handoff; (b) exact clock/counter mapping; (c) provider-continuity producer; (d) explicit finite profile; (e) codec/selector allocation; (f) observation-right policy | Each is answered by a concrete proposal below for the user to ratify (§4 S6-1, §5, §6). Before ratification S6-1 cannot start; S6-2/S6-3 host-only work can |

The S2-T/W annex §1 said D04 CP1 "is not satisfiable today": there is no tick→time mapping, no task-incarnation producer and no retained Cell CPU aggregate. This dossier does not claim that any of these exists. §2 shows that the kernel already has two of the needed identity properties: TIDs are never reissued and Cell generations are global. It then proposes kernel work for CPU time and the Cell aggregate (S6-1).

## 2. Starting state (facts)

### 2.1 Consumer: cellbox `top`, `ps --records`
- `cells/tools/cellbox/src/top.rs:52-57`: `sample_processes()` reads `GetProcs2` into a fixed `[ProcessInfoV2; MAX_ROWS]` and truncates again to `MAX_ROWS`. Any task beyond 64 is dropped silently, with no count or flag.
- `libs/text-engine/src/top.rs:13-17`: `TIMER_HZ = 10_000_000` ("Scheduler tick rate the kernel timestamps samples with") and `MAX_ROWS = 64`. Default delay = `TIMER_HZ` raw `GetTime` units (`:59`).
- `cells/tools/cellbox/src/top.rs:61-66`: `wait_ticks` adds those units to raw `GetTime` (x86 = HPET ns, `kernel/src/task/syscall.rs:7086-7087`), so the "1 s" delay is **10 ms** on x86 (already recorded in the OOM CP2 dossier §6). It yield-spins (`ostd::task::yield_now`), so `top` charges itself CPU while it waits (see §2.4).
- `libs/text-engine/src/top.rs:93-122`: deltas matched by bare `id`; `wall_delta` = `sample_ticks` delta (global `system_ticks`), `cpu_delta` = `cpu_run_ticks` delta. `:162-170` clamps to 100.0%. `mem_bytes = owned_bytes` (`:117`).
- `libs/text-engine/src/top.rs:215-297`: `CellRecord`/`build_cell_records` group tasks by the name prefix before `.`, set `cell_id = task.pid` and `generation: 1`, and sum `owned_bytes`, which double-counts heap. This is the fabricated identity named in the Phase 06 deviation log. It is not called by the live path but is still exported and tested (`libs/text-engine/src/top/tests.rs`, 7 tests, 130 lines).
- Rendering: `cells/tools/cellbox/src/top/render.rs` (50 lines) writes a hand-made PID/STATE/CPU%/HEAP/MEM/NAME table. Interactive mode writes `ESC[2J ESC[1;1H` and polls keys through `TerminalKeys` (`cells/tools/cellbox/src/executor.rs:566-619`, `SERIAL_SESSION`). It quits on `q`/`Q`/Ctrl-C/EOF. **No Ratatui; cellbox does not depend on `vitui`** (`cells/tools/cellbox/Cargo.toml:19-25`).
- Records: `top --records [--frames N]` and `ps --records` (`cells/tools/cellbox/src/native_rows.rs:24,31-36,60-75,82,122`) emit **flat Rows** with fields `pid,name,state,cpu_pct,cpu_tenths,heap_bytes,mem_bytes`. There are no frame boundaries or metadata. `ps --records` reuses `top::sample_processes` and a `TIMER_HZ/10` window, which is 1 ms on x86.
- Text `ps`: `cells/tools/cellbox/src/commands.rs:483-531`, legacy `GetProcs` into a 64-row local buffer, also silently capped.
- Classification: non-batch `top` = `CmdClass::Session`, sole stage with no redirects (`cells/tools/cellbox/src/typed.rs:121-136,272-283`). `top --records`/`ps --records` = typed Producer (`:175`). Ledger: `Builtin top InSuite` (`ledger.rs`).
- The cellbox syscall list is "byte-identical to the retired `/bin/shell` ceiling … nothing may be added for one command" (`cells/tools/cellbox/src/main.rs:22-23`). It declares `GetTime, GetProcs, GetProcs2, MemInfo, SetTimer, WaitCompletion` (`:24-64`).
- Other places that assume 10 MHz: `sleep`/`uptime`/`date` paths in `cells/tools/cellbox/src/cmd_sys.rs:103,122,139,273` (outside Phase 06; see D60-3).

### 2.2 Pipeline engine (Phase 03 code)
- `Value` (`libs/text-engine/src/pipeline.rs:74-89`) and `RowSource` (`:1456-1480`) exist. **No Frames category**: there is no Begin/End, `sample_id` or `unavailable_fields` anywhere. `sort-by --frame` and `take --frame` are explicitly rejected (`pipeline.rs:720-746`, tests `:1995-2007`). `to jsonl` has no frame envelope (`:751-772,1379-1420`). spec §1 and the `to jsonl` row require Begin/End frames and "explicit begin/row/end envelopes". The JSON shape of those envelopes is **not specified** anywhere.

### 2.3 TUI library
- `libs/vitui`: `CellosBackend<W: ByteSink>` (`backend.rs:114-150,294-389`), `TuiSession::enter/next_event/exit` (raw-input lease plus TUI screen mode, restored on drop; `session.rs:41-137`), `Event` (`event.rs:19-57`), and `vitui::ratatui::{core,widgets}` (`lib.rs:22-25`).
- Its only consumer today is the Terminal **dev-dependency** round-trip test (`cells/apps/terminal/Cargo.toml:20-23`). S5-4e moved the TUI runtime checks to "history-only until Phase 06 interactive `top` lands" (`phase-05-s4-proposal.md` S5-4e). So Phase 06 top is the planned real vitui consumer.
- The GUI grid is 79×26 after the grid fix (`phase-05-ratatui-terminal.md` deviation 2026-10-23).
- `vimio`/`vipolling` are async network readiness backends. They are not needed for a synchronous `TuiSession::next_event(deadline)` loop.

### 2.4 Kernel provider substrate
- **ABI.** `GetProcs = 30` (bit 14), `GetProcs2 = 239` (bit 55), `MemInfo = 243` (bit 56) (`libs/api/src/abi/syscall.rs:138-148,612-613,1001-1004,1120-1124`).
  - `ProcessInfoV2` is 80 bytes: `id,state,reserved0,name[32],sample_ticks,cpu_run_ticks,heap_bytes,owned_bytes` (`:1446-1461`, pinned in `syscall_tests.rs:330-333`).
  - `ViMemInfoV1` is 32 bytes and holds global frames (`:1463-1471`).
  - **The u64 allowlist is full** (bits 0–62 syscalls, 63 VFS-mutate). New calls share a bit, as `LookupServiceBound` shares 37 and `WaitCompletion` shares 42 (`syscall.rs:497,1036-1040,1096`).
  - The highest allocated syscall number is `LookupServiceBound = 429`.
- **Rows.** `kernel/src/task/syscall.rs:3418-3478,7015-7031`:
  - `heap_bytes = cell_quota::in_use(task.cell_id)`, repeated on every task of the Cell.
  - `owned_bytes = in_use + user_stack.usable + segment_mem.allocated`.
  - `collect_process_rows` takes `min(cap, tasks.len())` from the TID-ordered `BTreeMap` under `SCHEDULER`. It reports no total, no truncation flag and no cursor.
  - One `system_ticks()` value is stamped on all rows.
- **Authority.** The `check_allowlist` gate requires `observe_tasks` (X) for GetProcs/GetProcs2 and `observe_memory` (M) for MemInfo (`syscall.rs:9595-9602`; S2-T/W). An admitted caller sees **every** Cell's tasks: there is no scope filter.
- **Identity.**
  - `Task{id, cell_id, root_tid, cell_generation}` (`kernel/src/task/tcb.rs:202-208,499-513`). The generation comes from a global, nonzero, checked-increment sequence (`tcb.rs:561-575`).
  - TIDs come from one monotonic `next_task_id`, `checked_add` + panic, and are **never reissued within a boot** (`kernel/src/task/scheduler.rs:807-812`; threads use the same counter, `:1000-1008`).
  - Cell IDs are quota slots, reused lowest-free (`kernel/src/memory/cell_quota.rs:209`; counters zeroed at `deregister`, `:166-185`).
  - The owner registry is `cell_owners: Vec<CellOwnerSlot{Empty,Reserved,Live,Retiring}>` (`scheduler.rs:207-212,385`).
  - The API already has `PrincipalV1{cell_id,generation}` and `CellOwner{cell_id,generation,root_tid}` (`libs/api/src/abi/cell_session.rs:126-141`, `cell_owner.rs:51-99`).
  - `MAX_THREADS_PER_CELL = 32` (`scheduler.rs:22`). `MAX_CELLS = 64`, or 4096 with `cell-scale-experiment` (`cell_quota.rs:14-24`).
- **CPU.**
  - `Task.cpu_run_ticks` is incremented in `pick_next_local` whenever the current task is `Running` (`scheduler.rs:2274-2276`). `pick_next` runs from the timer ISR (`kernel/src/task.rs:1050-1058` → `yield_cpu`) **and from every `Yield` syscall** (`syscall.rs:6044-6046`). It therefore counts scheduler passes, not time: a yield-spinning task accrues ticks without bound.
  - `system_ticks` increments only on hart 0 on x86 (`task.rs:1020-1023`).
  - The x86 LAPIC period is a nominal 10 ms, calibrated from HPET with an uncalibrated fallback count (`hal/arch/x86/src/x86_64/apic.rs:177-179,236-245`).
  - An exiting task is moved out of `tasks` to `zombies` (`scheduler.rs:1527-1533`), so **its CPU disappears from every later sample**. No Cell aggregate survives.
  - `kernel/src/task/cpu_accounting.rs` (per-generation `lifetime_charge_ns`) exists only under `cfg(cellos_g1_cpu)`. That cfg needs `CELLOS_G1_CPU=1` plus the x86 test-hooks private trace (`kernel/build.rs:33-41`): it is development-only and is not admissible as the provider.
- **Clock.**
  - `GetTime` op 0 is the raw arch counter (x86 `hpet::now_ns`, which returns `0` if HPET is absent: `hal/arch/x86/src/x86_64/hpet.rs:69-78`). Op 5 is its frequency (x86 1 GHz). Op 4 is `system_ticks` (`syscall.rs:7078-7170`).
  - `ostd::session::monotonic_ns()` = op 0 × 1e9 / op 5 (`libs/ostd/src/session.rs:284-289`).
- **Memory.**
  - `cell_quota::in_use` is **all quota-charged bytes** of the Cell, not heap only. It includes kernel-heap allocations made in that Cell's context (`kernel/src/memory/heap.rs:144-145`), worker-thread kernel and user stacks (`scheduler.rs:1035-1047`), publication claims (`launch.rs:339-345`), pipes (`pipe.rs:180`) and session objects (`cell_session.rs:49`).
  - Worker-thread user stacks are therefore counted **twice** in `owned_bytes`.
  - The per-Cell limit is in a private `QUOTA_LIMITS` map with no getter (`cell_quota.rs:48`).
  - `MemInfo` holds global frame-allocator counters (`syscall.rs:7033-7057`).
- **SMP.** `MAX_HARTS = 2`, and hart 1 is dedicated to RealTime (`kernel/src/task/smp.rs:16-20`). The private runners pass no `-smp` flag, so QEMU runs with 1 vCPU.
- No telemetry service, task-manager app or other Cell-aware provider exists. `robot-dashboard` CPU is simulated, and `hypha tools/sys list_cells` uses `GetProcs`.

### 2.5 Runtime checks that pin today's `top`/`ps` output (migration census)
- `scripts/qemu-phase02-session-test.sh:291-319,385`: serial `top`, `q`, prompt returns.
- `scripts/qemu-phase03-pipeline-test.sh:440-461,454,492,496,507,677-708`: `ps --records | where/sort-by/select/take`, `top --records --frames N | take 2 | to jsonl` with `"cpu_tenths":`, endless-producer close and Ctrl-C.
- `cells/tests/phase04-terminal-driver/src/main.rs:1232,1279,1422,1526-1535` and `scripts/qemu-phase04-terminal-test.sh:630-634`:
  - endless `top --records --frames 100000 | to jsonl` (OOM sink, Ctrl-C);
  - `frontend-top-quit` waits for `"PID  STATE"`;
  - `top --records | take 1 | to jsonl` = 1 row;
  - `top | /bin/cat` refused, status 2 (`top-pipe-batch`).
- `cells/tests/s2tw-probe/src/main.rs:116-130`: GetProcs/GetProcs2/MemInfo denied from `/data/bin`, allowed from `/bin`.

## 3. Gaps against Phase 06 steps and criteria

| Phase 06 step / criterion | Gap |
|---|---|
| 1 authoritative identity, membership, units, completeness | Rows carry no Cell id, generation, root or membership. There is no total/truncation information. No unit holds a validated time base |
| 2 Cell vs task projection, no double count | Heap is repeated per task. `owned_bytes` adds heap (and worker stacks) again. The "heap" label is wrong (it is the quota charge) |
| 3 CPU deltas, explicit new/reset/stale, core-equivalent | The counter counts scheduler passes, including yields. The window uses hart-0 ticks. The 100% clamp hides this. Exited threads' CPU is lost. There is no quality marker |
| 4 no silent 64-row ceiling; bounded frames | Two silent 64-row caps (top/`ps --records`, text `ps`). The engine has no Frames |
| 5 finite/streaming records, Ratatui view, fields/sort/drill-down, batch over the same records | Hand-made ANSI view. Batch and records each build their own rows. No drill-down. The delay is wrong by 100× on x86 |
| 6 real multi-thread Cell, restart/reuse, denied, >64 tasks, cross-mode comparison | No fixture. Runners pin task-view strings and flat-row frames |
| Crit. 4 "observation grants no lifecycle rights" | Already true: `kill` is `InSuiteRefused` and there is no lifecycle path from top. It must stay true in the new TUI |

## 4. Proposed slices (each gets its own CP1 when reached; one WIP slice at a time)

| Slice | Delivers | Main files | Proof |
|---|---|---|---|
| **S6-1 Kernel/API telemetry provider** | New syscall `TelemetrySnapshot` returning header + Cell rows + task rows (§5), gated by X. Per-task `cpu_ns` and per-Cell-incarnation CPU retained across thread exit, both on the monotonic ns clock. Quota charged/limit read once per Cell. Optional system-memory block gated by M. `ostd` wrapper. Existing `GetProcs/GetProcs2/MemInfo` unchanged | `libs/api/src/abi/{syscall.rs,syscall_tests.rs}` (+ new `abi/telemetry.rs`), `libs/ostd/src/syscall.rs`, `kernel/src/task/{syscall.rs,tcb.rs,scheduler.rs}`, `kernel/src/task.rs` (switch path), `kernel/src/memory/cell_quota.rs` (limit getter), `cells/tests/s2tw-probe` (denial/control) | Kernel host tests; private-image probe |
| **S6-2 Telemetry model** | Replace `text_engine::top`'s model. Delete `CellRecord`, `build_cell_records`, `TIMER_HZ`, `MAX_ROWS` and the clamp. New pure model: parse one snapshot; delta engine keyed by `(provider_epoch, cell_id, generation)` / `(provider_epoch, tid)`; quality New/Ok/Reset/Unavailable and Gone counts; core-equivalent tenths (u32, no clamp); memory fields; per-frame filter/sort; one record projection (`Value`) that every renderer consumes | `libs/text-engine/src/top*.rs` | Host tests |
| **S6-3 Frames in the engine** | spec §1 Frames: Begin/Row/End items. `sort-by --frame`, `take --frame`, `select`/`where` pass metadata through. `to jsonl` frame envelopes; `to json`/`to text` reject continuous Frames. Typed default on redirect. Rows-only stages reject Frames before effects | `libs/text-engine/src/pipeline.rs` (+ typed classification in `cells/tools/cellbox/src/typed.rs`) | Host tests; P03 runner |
| **S6-4 cellbox consumers** | `top` default **Cell view**. Task drill-down. `-b` batch text renderer, `--records` Frames, and an interactive **vitui/Ratatui** view, all fed by the S6-2 records. Delay and key wait on `monotonic_ns` deadlines through `TuiSession::next_event`/`SetTimer`+`WaitCompletion` instead of yield-spinning. `ps --records` (and per D64-3 text `ps`) moved to the provider; the old sampler is deleted. cellbox gains a `vitui` dependency | `cells/tools/cellbox/src/{top.rs,top/render.rs,native_rows.rs,commands.rs,typed.rs,ledger.rs?}`, `cells/tools/cellbox/Cargo.toml`, `Cargo.lock` | Host tests; runners |
| **S6-5 Fixture and runtime proof** | Private fixture Cell `cells/tests/top-load`: N workers with known busy/blocked intervals, thread exit, relaunch for Cell-ID reuse, >64 total tasks. Private launch row under `phase04-driver-profile`. Migrate the §2.5 checks. New terminal-driver, P03 and GUI scenarios (§7) | `cells/tests/top-load/**`, `kernel/src/loader/launch_profile/*` (fixture row, feature-gated), runner scripts, `phase04-terminal-driver`, `scripts/phase04_gui_drive.py` | Private QEMU/TCG |

Recommended order (D60-4): **S6-1 → S6-2 → S6-3 → S6-4 → S6-5**. S6-2 needs the S6-1 record types. S6-3 is independent but is cheaper to review once the producer schema is fixed. S6-4 needs all three.

### 4.1 S6-1 design options
- **CPU time base (D61-1):**
  - **K1 — exact on-CPU ns (recommended).** Stamp the per-hart dispatch time when a task is switched in. At switch-out, add `now − stamp` to the outgoing task's `cpu_ns`. At capture, running tasks get `capture_now − stamp` added in the row only (no mutation). The clock is the same arch monotonic counter as `GetTime` op 0 (x86 HPET ns), so header times and `cpu_ns` share a base by construction. Cost: one counter read per switch; on x86 that is an HPET MMIO read, cost unmeasured [INFERENCE: noticeable under KVM, small relative to TCG emulation]. It is measured in S6-1 evidence. HPET absent (`now_ns() == 0`) → CPU fields flagged Unavailable.
  - **K2 — per-hart timer-tick sampling.** Charge the current task one tick per local timer IRQ only, not per yield. Unit = calibrated LAPIC period. It is statistical: a task that blocks before each tick is never charged. The unit is valid only when calibration succeeded (the fallback count is uncalibrated → Unavailable).
  - **K3 — reuse `cpu_accounting` (G1).** Rejected: dev-only cfg, private trace, fixed 64-Cell table.
- **Cell aggregate (D61-2):**
  - **A1 — per-Cell-incarnation `exited_cpu_ns` (recommended).** Store it in a scheduler side table keyed by `cell_id` and tagged with the generation. Add each worker's final `cpu_ns` at exit (`scheduler.rs:1527` path) and reset the entry when a new generation takes the slot. Cell CPU = `exited_cpu_ns + Σ live members`, which is authoritative.
  - **A2 — no aggregate.** Report Cell CPU only for unchanged membership (`DerivedStableMembership`, D04 §5); Unavailable otherwise. D04 itself says A2 is not completion.
- **Capture/paging (D61-3):**
  - **P1 — one-shot capture into a caller buffer (recommended).** The header carries totals, emitted and dropped counts, and `complete`. Rows are written as whole rows only, Cells before tasks. The client sizes the buffer for its frame bound. Default population is ≤ 64 Cells + 2,048 tasks, about 182 KiB with the §5 layouts. That fits the 4,096-row frame bound, so a default image is always complete. Larger `cell-scale-experiment` populations report explicit truncation.
  - **P2 — D04 retained `SnapshotRef` with 64-row pages,** one per caller, 8 system-wide, 1 MiB, 5 s expiry. That is new kernel object, holder-binding and expiry machinery with no extra benefit at the default population.
- **Scope (D61-4):**
  - **S1 — `System` scope only, requires X (recommended).** Exactly today's visibility; nothing is broadened. A denied call returns `PermissionDenied`, and `top` reports "telemetry denied" with status 1 and no rows.
  - **S2 — also `OwnCell` without X.** This gives sandbox Cells self-observation: new visibility, which needs an authority amendment.
  - `GrantedSet` is not offered (there is no grant mechanism); it returns Unsupported.
- **Provider epoch (D61-5):**
  - **E1 — a nonzero value drawn once at boot from the kernel entropy source (recommended).** Entry check: confirm the RNG is ready before the first capture. Otherwise fall back to E2.
  - **E2 — the RTC epoch ns at boot.**
  - **E3 — constant 1.** Observers cannot outlive a boot, but recorded JSONL from two boots would then look continuous.
- **Allocation (D61-6):** syscall number **430** (next after 429), sharing **allowlist bit 55** with `GetProcs2`. This is the same authority (task observation) and the same pattern as `LookupServiceBound`/bit 37. The dispatch gate is X; the memory block additionally needs M. cellbox's declared list does not change, so the "byte-identical ceiling" rule holds. The alternative is a sub-op of an existing syscall, which overloads unrelated semantics; not recommended.

### 4.2 S6-3 options
- **Frames vs flat rows (D63-1):**
  - **F1 — spec Frames (recommended).** `top --records` emits Begin/rows/End. Rows-only stages reject Frames; consumers use `take --frame`/`sort-by --frame`. The P03 and terminal-driver checks in §2.5 are migrated (`take 2` → `take --frame 1` plus row counts).
  - **F2 — keep flat Rows** with per-row `sample_id`/quality fields. This contradicts spec §1/`to jsonl` and is not recommended.
- **Envelope shape (D63-2), proposed:**
  - `{"frame":"begin","sample_id":N,"monotonic_ns":N,"meta":{…}}`
  - `{"frame":"row","value":{…}}`
  - `{"frame":"end","complete":B,"unavailable_fields":[…],"dropped":N}`
  - Typed mode wraps each in the `{"cellbox":1,"value":NODE}` form. `from jsonl` never turns envelopes back into Frames.
- **Producer metadata (D63-3):** spec Begin has only `sample_id, monotonic_ns`, but top must carry `provider_epoch`, scope, online CPUs, totals, the CPU unit and the system-memory block.
  - **M1 — amend spec §1 so Begin carries an optional producer `meta` record (recommended).** This is a change-log entry like the 2026-10-23 LF amendment.
  - **M2 — repeat these fields in every row.**

### 4.3 S6-4 options
- **Row kinds per frame (D64-1):**
  - **R1 — one schema per frame (recommended).** Default Cell rows; `--tasks` gives task rows; `--cell ID:GEN` gives task rows of one incarnation; system memory goes in Begin `meta`. Uniform rows keep `where`/`sort-by --frame` meaningful.
  - **R2 — mixed rows with a `kind` field.**
- **CLI (D64-2), proposed:**
  - `top [-b] [-n COUNT] [-d SECS] [-o FIELD] [--fields LIST] [--tasks | --cell ID:GEN] [-a] [--records]`. `-a` shows Retiring and other non-live rows, as today.
  - `-o` keys become field names of the selected row kind. `cpu,mem,pid,state,name` keep meaning (`pid` → `tid`/`cell`; `heap` → `quota`). Unknown keys are refused with status 2.
  - `-d` is decimal seconds on `monotonic_ns`.
  - TUI keys: Up/Down/PageUp/PageDown select; Enter drills into the Cell's tasks; Esc/Backspace returns; `o` cycles the sort; `r` reverses; `f` opens a field toggle list (Space toggles, Enter applies); `q`/Ctrl-C quits.
  - No lifecycle key exists.
- **`ps` (D64-3):** `ps --records` must move with the removed sampler (clean cutover). Text `ps` (legacy `GetProcs`, 64-row cap):
  - **move it to the provider too (recommended).** One sampler; no silent cap.
  - or leave it as a recorded gap.
  - The cellbox ceiling stays unchanged either way.
- **Columns (D64-4), proposed:**
  - Cell view: `CELL` (id:gen), `NAME` (root task name), `TASKS`, `CPU%` (core-equivalent, may exceed 100), `QUOTA` (charged), `LIMIT`, `STATE`.
  - Task view: `TID`, `NAME`, `STATE`, `CPU%`, `STACK`, `SEGMENTS`.
  - Header line: sample id, `complete`/`TRUNCATED n`, CPU unit, system memory (used/total or `denied`/`n/a`).
  - The view fits 79 columns. No column is labelled RSS or MEM-as-sum, and task bytes are never summed into the Cell.

## 5. S6-1 CP1 request (concrete, pending D61-1..6)
- **Call.** `TelemetrySnapshot = 430`. Args: `a0 = out_ptr`, `a1 = out_len` (bytes, ≤ `MAX_USER_BUF`), `a2 = flags` (bit0 tasks, bit1 system memory; other bits must be 0), `a3 = 0`. Returns bytes written.
- **Errors.**
  - `PermissionDenied`: no X.
  - `InvalidInput`: `out_len < 96`, misaligned buffer (align 8), or unknown flag/nonzero `a3`.
  - `OutOfMemory`: kernel transient allocation refused. That allocation is charged to the caller's quota, as `GetProcs2` is today.
  - A bit1 request without M is **not** an error: the header flags system memory `denied` and its fields stay 0 with the availability bit clear.
- **`TelemetryHeaderV1`** (`repr(C)`, 96 bytes, align 8):
  - `version:u32 = 1`, `flags:u32` (complete, cpu_available, sysmem_present, sysmem_denied, memory_interval)
  - `provider_epoch:u64`, `sample_id:u64` (kernel monotonic per capture)
  - `capture_start_ns:u64`: the single `now` used for CPU in-flight adjustment
  - `capture_end_ns:u64`
  - `online_cpus:u32`, `scope:u32 = 1` (System)
  - `total_cells:u32`, `total_tasks:u32`, `emitted_cells:u32`, `emitted_tasks:u32`
  - `mem_total_frames`, `mem_used_frames`, `mem_free_frames`, `mem_page_size`: `u64` each
- **`TelemetryCellV1`** (96 bytes): `cell_id:u64`, `generation:u64`, `root_tid:u64`, `cpu_ns:u64` (exited + live members), `quota_charged_bytes:u64`, `quota_limit_bytes:u64`, `task_count:u32`, `state:u32` (Live 0 / Retiring 1), `flags:u32` (cpu, quota, limit availability; name_truncated), `reserved:u32 = 0`, `name:[u8;32]` (root task name).
- **`TelemetryTaskV1`** (88 bytes): `tid:u64`, `cell_id:u64`, `generation:u64`, `cpu_ns:u64`, `user_stack_bytes:u64`, `segment_bytes:u64`, `state:u32` (Ready 0 / Running 1 / Waiting 2 / Retiring 3), `flags:u32` (cpu availability, name_truncated, stack_in_quota), `name:[u8;32]`.
- **Ordering, completeness, consistency.**
  - Cells are sorted by `generation`; tasks by `(generation, tid)`.
  - Identity, membership and CPU are taken under one `SCHEDULER` hold. Quota counters are atomics read in the same hold but charged outside it, hence the `memory_interval` flag. `MemInfo` frames are read under `FRAME_ALLOCATOR` after `SCHEDULER` is released.
  - Lock-order check required: `SCHEDULER → QUOTA_LIMITS` already occurs through kernel-heap allocation under the scheduler lock [INFERENCE from `heap.rs:144-145` + `cell_quota.rs:249`]. The limit getter must not introduce a reverse order.
- **Finite profile.** No retained kernel state beyond `cpu_ns` (8 bytes/task), the per-hart dispatch stamp and the per-Cell `exited_cpu_ns`+generation (16 bytes × `MAX_CELLS`). The transient capture is at most `96 + 96·cells + 88·tasks` bytes, bounded by `out_len`.
- **Unchanged.** `GetProcs`, `GetProcs2`, `MemInfo` numbers/layouts/bits, `cpu_run_ticks` semantics, CapSet/policy v6, launch profiles, the cellbox syscall list.
- **Tests.**
  - Layout pins in `syscall_tests.rs`.
  - Kernel host tests: capture builder over a constructed scheduler table (totals, truncation by `out_len`, ordering, generation tagging); `cpu_ns` charge arithmetic with checked math; `exited_cpu_ns` across worker exit and generation reuse of the same `cell_id`; gate refuses without X; sysmem `denied` without M.
  - Private image: `s2tw-probe` gains a `telemetry` case (`/data/bin` denied, `/bin` allowed).

## 6. Deviations from the D04 candidate needing ratification

| # | D04 candidate (`abi-vfs-telemetry.md` §5) | Proposal | Why |
|---|---|---|---|
| DV-1 | `task_incarnation:u64`, owner-issued, "never inferred from recycled TID" | `(provider_epoch, tid)`; no separate field | Kernel TIDs are never reissued within a boot (`scheduler.rs:807-812`), so the TID *is* the owner-issued incarnation |
| DV-2 | `membership_epoch`, `accounting_epoch` per row | Dropped | Unique TIDs + generations let the client derive membership change exactly. Counters never reset within an incarnation; a decrease is reported as `Reset` |
| DV-3 | Retained `SnapshotRef` with 64-row pages, 8 system / 1 per caller, 1 MiB, 5 s expiry | One-shot capture into the caller buffer with totals/dropped/complete (P1); 64-row pages become display paging only | §4.1 D61-3 |
| DV-4 | `heap_current_bytes`, `task_private_bytes`, `attributed_total_bytes` | `quota_charged_bytes` + `quota_limit_bytes` per Cell; task `user_stack_bytes`/`segment_bytes` informational; **no attributed total** | `in_use` is the whole quota charge, not heap. Worker stacks are already inside it (`scheduler.rs:1035-1047`), so a disjoint total cannot be proven |
| DV-5 | Display name up to 128 bytes + truncation flag | 32 bytes + truncation flag | Same bound as existing rows; avoids a variable-length codec |
| DV-6 | Scopes OwnCell/GrantedSet/System | System only (S1); others Unsupported | Does not broaden visibility (Phase 06 security note) |
| DV-7 | Optional machine-normalized CPU view | Not in Phase 06; core-equivalent only | Single-vCPU runners cannot exercise it; the header still carries `online_cpus` |
| DV-8 | New allowlist authority | Shares bit 55 with `GetProcs2` | The u64 allowlist is full |

## 7. Verification plan
Behavioral tests only. No source-text or heading tests, and today's heading-pinning checks (`"PID  STATE"`, `"cpu_tenths":` presence) are replaced, not re-pinned.

- **Host:**
  - `cargo test -p api --target x86_64-unknown-linux-gnu`: layout pins.
  - Kernel host tests (as in S2-T/W §11): S6-1 list in §5.
  - `cargo test -p text-engine --target x86_64-unknown-linux-gnu`:
    - S6-2: new/ok/reset/gone; restart with the same `cell_id` and a new generation never merges; thread exit keeps Cell CPU; two busy workers give Cell > 1000 tenths (multi-core arithmetic; no clamp); unavailable CPU flag; no memory sum; truncation propagates to `End{complete:false, dropped}`.
    - S6-3: Frames stage rules, envelopes, one-over bounds, rejection before effects.
  - `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`, if its harness covers it: classification of the new flags, and **one recorded snapshot rendered by batch, records and the Ratatui view (vitui `CellosBackend` into a byte sink / Ratatui buffer) yields identical numbers, units and quality**. This is the Phase 06 step 6 "same snapshot" comparison; the snapshot is a capture saved from a private run.
  - Bare-metal builds for all touched packages, including the non-x86 kernel builds that compile the new counter path.
- **Private QEMU/TCG (signed fixture images, existing guards; no canonical image, no inbound network):**
  - `qemu-phase04-terminal-test.sh` + driver:
    - `top-load` with known busy/blocked workers (busy ≈ 1000 tenths per worker when alone, blocked ≈ 0, within a stated TCG tolerance);
    - worker exit keeps Cell CPU;
    - relaunch gives the same `cell_id` with a higher `generation`, and drill-down shows only the new TIDs;
    - more than 64 tasks with `complete=true` and no truncation;
    - `top -b`, `top --records`, `top --tasks`, `top --cell`;
    - `frontend-top-quit` on the Cell view;
    - `top | /bin/cat` refused with status 2;
    - Ctrl-C on an endless Frames pipeline;
    - pressing `k`/`K` changes no process set (no lifecycle right);
    - `s2tw-probe` telemetry denied/allowed.
  - `qemu-phase03-pipeline-test.sh`: migrated `ps --records` and `top --records` checks; `take --frame`/`sort-by --frame` on a live stream; consumer close stops sampling.
  - `qemu-phase02-session-test.sh`: serial interactive Ratatui top, `q` and Ctrl-C restore the prompt and modes; abandoned-holder recovery as in S5-3.
  - `qemu-phase04-gui-test.sh` ×6: exact screens for the Cell view, drill-down, sort change, field toggle and resize, plus prompt restoration. This becomes the real-consumer replacement for the archived tui-proof screens.
  - Regression: terminal (397 + new), GUI 6× (259 + new), P03 60 (migrated), P02 48, kernel AP lane.
- **Evidence ceiling (recorded with each run):** TCG, 1 vCPU, HPET-based ns. No CPU-speed, hardware, KVM or multi-hart claim. With D60-5 = no, the >100% Cell CPU claim is host-test only.

## 8. Decisions requested (recommendations first)

**Entry and process**
- **D60-1 Enter Phase 06** with Phase 05 = implemented-private-evidence / CP2 pending and Phase 04 = in_progress (criterion 4) (recommended; the plan rule ties CP2 to rollout/final retirement, not to successor entry), or hold until Phase 05 CP2.
- **D60-2 Ordering against the Phase 05 CP2:** *prepare the Phase 05 CP2 dossier (exact SHA-256 of the final files) before the first Phase 06 source edit* (recommended; freezes the overlapping cellbox/vitui bytes), or let Phase 06 proceed and re-hash later.
- **D60-3 10 MHz assumption outside top** (`sleep`/`uptime`/`date`, `cmd_sys.rs:103,122,139,273`): *record as a separate defect with its own CP1* (recommended; not Phase 06 scope), or fold it into S6-4.
- **D60-4 Slice order:** *S6-1 → S6-2 → S6-3 → S6-4 → S6-5* (recommended), or S6-2/S6-3 first as host-only work while S6-1 is reviewed.
- **D60-5 Multi-hart lane:** *no; >100% Cell CPU stays a host-test claim* (recommended; runners are 1 vCPU and hart 1 is RT-dedicated), or add a private `-smp 2` terminal-runner lane.
- **D60-6 Owner handoff:** confirm the assistant as integrating owner for the S6-1 kernel/API/ostd file set in §4 under your CP1, as in S2-T/W (recommended), or name another owner.

**S6-1 provider**
- **D61-1 CPU time base:** *K1 exact on-CPU ns* (recommended) / K2 tick sampling / K3 G1 module.
- **D61-2 Cell aggregate:** *A1 retained `exited_cpu_ns` per incarnation* (recommended) / A2 stable-membership-only.
- **D61-3 Capture:** *P1 one-shot caller buffer* (recommended) / P2 retained SnapshotRef paging.
- **D61-4 Scope:** *S1 System + X only* (recommended) / S2 add OwnCell without X.
- **D61-5 Provider epoch:** *E1 boot entropy* (recommended, with entry check) / E2 RTC boot ns / E3 constant.
- **D61-6 Allocation:** *syscall 430 sharing allowlist bit 55, X gate, M for the memory block* (recommended) / sub-op of an existing call.
- **D61-7 Ratify DV-1..DV-8** (§6), or name the ones to keep as in D04.
- **D61-8 Approve the S6-1 CP1 as written in §5** (after D61-1..7).

**S6-3 engine**
- **D63-1** *F1 spec Frames* (recommended) / F2 flat rows.
- **D63-2** Envelope shape as in §4.2 (recommended) / another shape you specify.
- **D63-3** *M1 spec §1 amendment: optional Begin `meta`* (recommended) / M2 per-row fields.

**S6-4 consumer**
- **D64-1** *R1 one schema per frame* (recommended) / R2 mixed `kind` rows.
- **D64-2** CLI and key map as in §4.3 (recommended) / changes.
- **D64-3** Text `ps`: *move to the provider* (recommended) / leave as a recorded gap.
- **D64-4** Columns and labels as in §4.3 (recommended) / changes.

**S6-5 proof**
- **D65-1 Denied-telemetry evidence:** *extend `s2tw-probe` (provider denial) plus a host test of top's denied rendering* (recommended), or also add a private restricted-cellbox row to show top's denied UI at runtime (D-TW2 declined such a row earlier).
- **D65-2 Fixture:** *new feature-gated `cells/tests/top-load` + private `/bin/top-load` launch row, removed or kept as a runner fixture after evidence* (recommended: keep as a runner fixture like `s2tw-probe`, since P06 checks must stay re-runnable), or a throwaway removed after evidence.

## 9. Risks and rollback
- **Hot-path change.** K1 adds a clock read to every context switch. Measure switch overhead before and after on the private image; if HPET cost is unacceptable, stop and return with numbers rather than switching silently to K2.
- **Lock order.** Capture holds `SCHEDULER`, reads quota atomics, and adds a limit getter. A reverse order would deadlock; kernel host tests plus the AP lane must cover capture during spawn/exit/retirement.
- **Runner churn.** Frames (F1) changes P03 and terminal-driver expectations. Migrate them in the same slice as the behavior; do not keep flat rows as a hidden fallback.
- **Footprint.** cellbox gains Ratatui through vitui. Size is recorded, not judged; Phase 10 owns the measured footprint.
- **Rollback.** Revert the slice's files together and cold-boot a matched private image. `GetProcs2`/`MemInfo` are untouched, so old consumers keep working. Never reinterpret `TelemetryCellV1`/`TaskV1` bytes as V2 rows. No persisted state. Rollout, CP2 and quarantine stay as they are.

## 10. S6-1 Results (2026-10-23, implemented; private QEMU proof pending)

Slice order per D60-4: S6-1 first. D60-3 (the 10 MHz `sleep`/`uptime`/`date` users in `cmd_sys.rs`) is a separate defect with its own CP1, so S6-1 does not touch it.

**Implemented as §5:**
- `TelemetrySnapshot = 430` (`a0` out_ptr, 8-aligned; `a1` out_len ≥ 96 and ≤ `MAX_USER_BUF`; `a2` bit0 tasks / bit1 system memory; `a3` = 0 → bytes written). Allowlist bit 55, shared with `GetProcs2`. The dispatch gate requires `observe_tasks` (X) through the same `observe_right_held` path as `GetProcs`/`GetProcs2`.
  - Bad arguments → `InvalidInput`, before anything is sampled.
  - Transient allocation refusal → `OutOfMemory`, encoded as the typed OOM sentinel.
  - Bit1 without M → header `SYSMEM_DENIED`, memory fields 0, not an error.
- Record layouts `TelemetryHeaderV1` 96 B, `TelemetryCellV1` 96 B, `TelemetryTaskV1` 88 B, align 8, fields and offsets exactly as §5, in `api::telemetry`.
  - Encoding is explicit little-endian; bytes are never reinterpreted as structs.
  - `TelemetrySnapshot::parse` rejects any of: a length that disagrees with the emitted counts; an unknown version, scope, flag or state; a nonzero reserved field; `COMPLETE` with dropped Cells; memory values without `SYSMEM_PRESENT`; rows out of order.
  - Header flags: bit0 `COMPLETE`, bit1 `CPU_AVAILABLE`, bit2 `SYSMEM_PRESENT`, bit3 `SYSMEM_DENIED`, bit4 `MEMORY_INTERVAL`.
  - Cell flags: `CPU_AVAILABLE`, `QUOTA_AVAILABLE`, `LIMIT_AVAILABLE`, `NAME_TRUNCATED`.
  - Task flags: `CPU_AVAILABLE`, `NAME_TRUNCATED`, `STACK_IN_QUOTA` (= the task has a thread-stack quota charge).
- **K1.** `Task::cpu_ns` holds exact on-CPU ns on the `GetTime(0)` clock. `Scheduler::charge_cpu` runs first in every scheduler pass (timer or voluntary): it charges the hart's current task `now − previous pass` and restamps the hart. A pass with no current task charges nothing, so idle time is never billed.
  - No charge is made for the first stamp, for a zero clock (x86 without HPET) or for a clock that runs backwards.
  - The capture adds `capture_start_ns − stamp` for each hart's current task to that row only; nothing is written back.
- **A1.** `CpuLedger` keeps one `{generation, ns}` slot per CellId.
  - Retention happens where a task leaves the live table: in `exit_task` (move to zombies) and in quiescent root retirement.
  - A zombie still on a hart is charged at the next pass, and its Cell retains that time too.
  - A newer generation takes the slot over from 0; a retention for an older generation is dropped.
  - Cell `cpu_ns` = retained exited CPU + every live member's CPU (+ in-flight time, including a zombie still on a hart).
- **P1/S1.** One `SCHEDULER` hold covers owners (Live/Retiring slots), tasks and CPU. Cells are sorted by generation, tasks by `(generation, tid)`. Cells come first and only whole rows are written.
  - `total_*`, `emitted_*` and `COMPLETE` make any truncation explicit. `COMPLETE` means every *requested* row was emitted; tasks count only when bit0 is set.
  - Quota charge and limit are read once per Cell (`cell_quota::in_use` / the new `cell_quota::limit`).
  - `MemInfo` frames are read under `FRAME_ALLOCATOR` after `SCHEDULER` is released.
- **E1.** `provider_epoch` is drawn once per boot at the first capture, outside `SCHEDULER`:
  - hardware entropy (`virtio_rng::get_random`, 8 bytes, nonzero) when it answers. That answer is the readiness check.
  - else E2: RTC now − monotonic now, i.e. the boot instant.
  - `sample_id` increments per capture under `SCHEDULER`.
- `ostd::syscall::sys_telemetry_snapshot(&mut [u64], flags)` (the slice type guarantees alignment) and `telemetry_bytes`.
  - Malformed calls are refused locally.
  - A kernel generic error → `PermissionDenied`; typed OOM → `OutOfMemory`.
- Unchanged: `GetProcs`, `GetProcs2`, `MemInfo` (numbers, layouts, bits), `cpu_run_ticks`, CapSet/policy v6, launch profiles, the cellbox syscall list (`cellbox-ceiling-diff.py` G-S1a PASS).
- `s2tw-probe` reports `S2TW telemetry <r>`: `denied` from `/data/bin`, and from `/bin` `other:cells-N-tasks-M` (which also requires a decoded V1 snapshot). The terminal driver and runner gain `sandbox-denied-telemetry` and `control-allowed-telemetry` (+2 checks, +2 markers).

**Deviations (implementation detail, within the approved CP1):**
1. K1 charges at every scheduler pass, not as a separate stamp-in plus add-at-switch-out. The result is identical: a pass is the only point where a hart's current task changes. The kernel's own work during a pass or syscall is charged to the current task.
2. E1 is drawn lazily at the first capture, once per boot, rather than during boot. If neither entropy nor RTC answers, the epoch is constant 1 (E3 form), so it is never zero.
3. `supports_typed_spawn_oom` is renamed `supports_typed_oom` (it now covers `TelemetrySnapshot`). The kernel `monotonic_ns` helper moved from `cell_session.rs` to `telemetry.rs` (one clock helper; `cell_session` now uses it). `smp::online_hart_count` is no longer gated to `test-hooks`, because it supplies `online_cpus`.
4. Tasks without a published Cell owner (kernel-context tasks) appear only as task rows. `total_tasks` counts the whole live table, including Retiring members. `MEMORY_INTERVAL` is always set, because quota counters are lock-free.
5. Row names are cut at a UTF-8 boundary ≤ 32 bytes and flagged. `GetProcs2` cuts at byte 32.

**Verification run here:**
- `cargo test -p api --target x86_64-unknown-linux-gnu`: 144 passed. Covers the 7 `abi::telemetry` tests (layout/offset pins, round trip, length/truncation/order/flag refusals, names) and `telemetry_snapshot_shares_the_get_procs2_authority`.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 272 passed, also with `--features phase04-driver-profile`. New tests:
  - `task::telemetry::tests` (8):
    - per-hart pass arithmetic, including missing and backwards clocks;
    - ordering, totals and in-flight Cell/task CPU through API decode;
    - whole-row truncation with explicit counts;
    - worker exit through `exit_task` keeps Cell CPU, including zombie time;
    - generation reuse of one CellId starts from 0, and stale retention is dropped;
    - an unavailable clock is flagged;
    - sysmem denied/present;
    - name truncation.
  - `task::syscall::tests`: the extended X-gate test (TelemetrySnapshot denied without X, M alone never admits it), and `telemetry_snapshot_maps_all_four_args_and_validates_them`.
- `cargo test -p ostd --target x86_64-unknown-linux-gnu`: passes.
- Bare-metal builds:
  - `cellos-kernel` x86_64-unknown-none, release, with the terminal-runner features `phase02-terminal-profile,phase03-filter-profile,phase04-driver-profile` and the runner RUSTFLAGS (throw-away key, private copy of the embedded dir);
  - `cellos-kernel` riscv64gc-unknown-none-elf and aarch64-unknown-none-softfloat, release;
  - `app-cellbox`, `app-s2tw-probe` and `app-phase04-terminal-driver` x86_64-unknown-none, release.
- `scripts/check-lookupservicebound-law1-digests.sh` PASS and `scripts/check-ai-law1-digests.sh` OK.
- Runner pins: the terminal runner `authority` digests cover `kernel/src/loader/launch_profile/{mod,profiles,targets,tests}.rs` and `libs/api/src/abi/cell_session.rs`. None of these changed, and the digests were re-checked and match, so **nothing was re-pinned**. The P03 runner hashes `kernel/src/task/syscall.rs` only as recorded provenance, not as a pin.

**Not run (parent):**
- private QEMU/TCG `qemu-phase04-terminal-test.sh` (expects the 2 new markers);
- `qemu-phase03-pipeline-test.sh`, `qemu-phase02-session-test.sh`, `qemu-phase04-gui-test.sh` ×6 (regression after the scheduler hot-path change);
- the kernel AP lane;
- the §9 K1 switch-overhead measurement (pass cost before/after on the private image).

**Files:**
- `libs/api/src/abi/telemetry.rs` (new), `libs/api/src/abi.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/syscall_tests.rs`
- `libs/ostd/src/syscall.rs`
- `kernel/src/task/telemetry.rs` (new), `kernel/src/task.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/smp.rs`, `kernel/src/task/cell_session.rs`, `kernel/src/memory/cell_quota.rs`
- `cells/tests/s2tw-probe/src/main.rs`, `cells/tests/phase04-terminal-driver/src/main.rs`, `scripts/qemu-phase04-terminal-test.sh`
- `CHANGELOG.md` (Added entry, end of the Unreleased `### Added` list)
