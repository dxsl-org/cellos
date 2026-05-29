# Phase 22 — Benchmarking Suite

**Effort:** 80h | **Priority:** P3 | **Status:** pending | **Blockers:** Phases 1-3 (foundation), most service phases helpful

## Overview

Build a structured performance-measurement system: a benchmark Cell, a `ViBenchmark` trait, baseline metrics for the four PDR targets (context switch, IPC, syscall, memory footprint), and a weekly CI regression gate. Validates v1.0 performance goals and prevents silent regressions.

## Context Links

- `libs/api/src/benchmark.rs` — existing `ViBenchmark` trait stub
- `docs/project-overview-pdr.md` — performance targets
- Phase 11 (tests) — bench leverages same QEMU harness pattern
- Phase 19 (docs/release) — publishes results to docs site

## Key Insights

- Microbenchmarks within QEMU show *relative* changes well but absolute numbers are misleading (QEMU JIT translates each instruction). For v1.0 commitments, the QEMU number must clear targets with a 2x margin.
- Each bench needs: warm-up phase, N iterations, report min/p50/p99/max. Always seed any randomness.
- Regression detection: compare current main vs N=20 historical samples; flag if median > p99 of history. Avoid noise by requiring 3 consecutive runs to confirm regression.
- Targets (from PDR):
  - Context-switch latency: < 100 µs
  - Message Send/Recv: < 50 µs
  - Syscall overhead: < 10 µs
  - Memory footprint (kernel + 3 services): < 10 MB

## Requirements

**Functional**
- `ViBenchmark` trait: `start_timer()`, `stop_timer()`, `report()`
- A `bench` cell with sub-benches: context_switch, ipc_send_recv, syscall_yield, memory_footprint
- `bench --json` outputs machine-readable results
- CI runs all benches weekly, archives results, flags regressions
- HTML report on docs site (Phase 19) showing trends

**Non-functional**
- Each bench < 30s in QEMU
- Total bench wall-time < 5 min in CI
- Output deterministic enough that two clean runs of same commit are within 5%

## Architecture

```
cells/apps/bench/src/
  main.rs            ── arg parsing, run each registered bench
  framework/
    timer.rs         ── high-resolution timer (cycle counter where available)
    report.rs        ── stats (min, p50, p99, max), JSON emit
    runner.rs        ── warm-up, N iterations, recording
  scenarios/
    context_switch.rs
    ipc_send_recv.rs
    syscall_yield.rs
    memory_footprint.rs

.github/workflows/perf.yml
  weekly cron:
    boot kernel, run /bin/bench --json > out.json
    upload artifact
    compare vs main_history.json (in gh-pages branch)
    fail if regression > 10% on any metric for 3 consecutive runs
```

## Related Code Files

**Modify:**
- `libs/api/src/benchmark.rs` — finalize `ViBenchmark` trait
- `gen_disk.ps1` — bake `/bin/bench` into disk image
- `.github/workflows/security.yml` or new `perf.yml` — schedule + report

**Create:**
- `cells/apps/bench/Cargo.toml` — new cell crate
- `cells/apps/bench/src/main.rs`
- `cells/apps/bench/src/framework/timer.rs`
- `cells/apps/bench/src/framework/report.rs`
- `cells/apps/bench/src/framework/runner.rs`
- `cells/apps/bench/src/scenarios/context_switch.rs`
- `cells/apps/bench/src/scenarios/ipc_send_recv.rs`
- `cells/apps/bench/src/scenarios/syscall_yield.rs`
- `cells/apps/bench/src/scenarios/memory_footprint.rs`
- `.github/workflows/perf.yml`
- `scripts/compare-bench-results.sh` — diff current vs historical, emit GitHub annotation
- `docs/performance-report.md` — published baseline metrics + methodology
- `docs/performance-history.json` (auto-updated on gh-pages branch) — historical sample database

## Implementation Steps

1. **Finalize `ViBenchmark` trait** in `libs/api/src/benchmark.rs`:
   ```rust
   pub trait ViBenchmark {
       fn name(&self) -> &'static str;
       fn warmup(&mut self, iters: u32);
       fn iteration(&mut self) -> u64;  // returns elapsed ticks
       fn finish(&mut self) -> BenchReport;
   }
   pub struct BenchReport {
       pub name: &'static str,
       pub n: u32,
       pub min_ns: u64, pub p50_ns: u64, pub p99_ns: u64, pub max_ns: u64,
   }
   ```
2. **High-res timer** `framework/timer.rs`:
   - RV64: read `rdtime` (cycle counter)
   - AArch64: read `CNTPCT_EL0`
   - x86_64: read `rdtsc` (preferring `rdtscp` for serialization)
   - Convert ticks → ns via kernel-published timer frequency (Config Cell key `system.timer_freq_hz`)
3. **Runner** `framework/runner.rs`:
   - `Runner::run(bench: &mut impl ViBenchmark, warmup: 100, iters: 10_000) -> BenchReport`
   - Sort sample vec; compute percentiles
4. **Scenario: context_switch**
   - Create 2 tasks that yield to each other
   - Measure ticks between yield → next yield
   - Iterations = 100K
5. **Scenario: ipc_send_recv**
   - Two cells, sender + echo
   - Sender: send 64B → recv → measure RT
   - Iterations = 10K
6. **Scenario: syscall_yield**
   - One task calls `Yield` syscall in a tight loop
   - Measure ticks between yield → next instruction returns
   - Iterations = 100K
7. **Scenario: memory_footprint**
   - Static measurement: read `MemInfo()` syscall after init+config+vfs+shell up
   - Output: total used bytes; compare against 10 MB target
8. **`main.rs`**:
   - Parse `--json`, `--name <bench>` (run single), `--iters N`
   - Iterate scenarios; emit JSON or human-readable
9. **CI workflow** `.github/workflows/perf.yml`:
   - Cron weekly Sunday 06:00 UTC
   - Build kernel + bench cell
   - Boot QEMU, run bench, capture JSON
   - Upload as artifact `perf-YYYY-MM-DD.json`
   - Download historical from gh-pages branch
   - Compare via `scripts/compare-bench-results.sh`; fail if any metric > 10% worse than median of last 20 runs, for 3 consecutive new runs (track via state file in gh-pages)
10. **Publish report** `docs/performance-report.md`:
    - Baseline numbers (first run after each phase milestone)
    - Methodology + how to reproduce
    - Targets vs actuals table
11. **GitHub Pages historical chart**:
    - Append each run to `docs/performance-history.json`
    - A small client-side `index.html` on docs site renders a Chart.js timeline

## Todo List

- [x] Finalize `ViBenchmark` trait (extended with `BenchReport` p50/p99 + JSON in libs/api/src/benchmark.rs)
- [x] Implement high-res timer (sys_get_time → GetTime syscall; fallback 10 MHz in framework/timer.rs)
- [x] Implement runner (warmup=100, iters=1000, sort → percentiles in framework/runner.rs)
- [x] Implement context_switch scenario (double sys_yield round-trip)
- [x] Implement ipc_send_recv scenario (64-byte message to VFS endpoint)
- [x] Implement syscall_yield scenario (single ecall → U-mode return)
- [x] Implement memory_footprint scenario (approximation; TODO: MemInfo syscall)
- [x] Implement bench `main.rs` (4 scenarios + PASS/FAIL summary + JSON output)
- [x] Create `.github/workflows/perf.yml` (weekly cron + artifact upload + PDR check)
- [x] Write `scripts/compare-bench-results.sh` (rolling median + 3-run regression gate)
- [ ] Bake `/bin/bench` in disk image (needs gen_disk.ps1 update)
- [x] Write `docs/performance-report.md` (methodology + expected numbers + add-scenario guide)
- [ ] Chart.js historical timeline on docs site
- [ ] Capture actual baseline numbers (first QEMU CI run)
- [ ] Verify all 4 PDR targets met (pending first boot)
- [ ] CI green

## Success Criteria

- All 4 PDR performance targets met with ≥ 2x margin in QEMU
- Weekly perf CI runs and uploads artifacts
- Regression > 10% on any metric for 3 consecutive runs fails build
- Historical chart visible on docs site
- Bench wall-time < 5 min in CI

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| QEMU JIT cache warmup dominates first iterations | Cert | Med | Discard first 100 warmup samples; document |
| GitHub Actions runner variance week-to-week | Cert | Med | Compare to rolling median of last 20 runs, not single baseline; require 3-run regression |
| `rdtsc` non-invariant across CPU migration | Low | Med | Pin to single CPU via QEMU `-smp 1`; document |
| Storing historical JSON on gh-pages adds noise to docs | Low | Low | Keep history in a separate orphan branch `perf-history`; gh-pages only links to it |
| One subsystem regression hides another's improvement | Med | Med | Per-metric thresholds; not aggregate |
| Phase 21 (32-bit) targets miss the 10 MB footprint goal | Med | Med | Set per-arch footprint targets (≤ 8 MB on 32-bit, ≤ 10 MB on 64-bit) in PDR addendum |

## Security Considerations

- Bench cell has no special capabilities; runs as a normal cell
- Timer reads from kernel are non-sensitive
- JSON output may include cell names; ensure no path/file content leak

## Rollback

Bench is purely additive. Revert removes the cell + workflow; runtime unaffected. Historical data on gh-pages stays.

## Next Steps

Continuous bench enables data-driven performance tuning post-v1.0. Phase 23 community may contribute new bench scenarios (web server throughput, multi-cell concurrent IPC, etc.). v1.0 release ships a one-page performance summary in the release notes (Phase 19).
