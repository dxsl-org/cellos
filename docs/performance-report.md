# Cellos Performance Baseline Report

> **Status:** The RV64/QEMU collection and validity path is live and fail-closed, but retained captures do not bind the dirty source tree that produced them. Target/history results are diagnostic until source-bound recapture; hardware-qualified latency remains pending.
> Scheduled by `.github/workflows/perf.yml`.

---

## PDR Targets (v1.0 Requirements)

| Metric | Target | Margin | Notes |
|--------|--------|--------|-------|
| Context-switch latency | < 100 µs | ≥ 2× in QEMU | Measured via double `sys_yield` round-trip |
| IPC send/recv round-trip | < 50 µs | Qualified hardware only | p99 for a named board/clock/build profile; QEMU tracks regressions |
| Syscall overhead (`Yield`) | < 10 µs | ≥ 2× in QEMU | Single ecall → return to U-mode |
| Allocator-committed memory | < 10 MiB | — | Exact global frame commitment via opt-in MemInfo |

QEMU measurements provide repeatable trend evidence, not hardware qualification. Context-switch
and syscall rows have explicit QEMU-calibrated gates; IPC keeps its 50 µs hardware target and is
checked in QEMU by sustained historical regression instead of that absolute ceiling.

---

## Methodology

### Timer Resolution

The bench cell reads ticks via `sys_get_time()` → kernel `GetTime` syscall →
RV64 `mtime` register (10 MHz on QEMU `virt` machine).  One tick = 100 ns at 10 MHz.

### Run Protocol

1. Boot the normal release profile, wait for the exact `Cellos >` prompt, then send `bench`.
2. Discard scenario warmups; retain the frozen per-scenario sample counts in
   `.agents/260905-1139-sas-lbi-outcome-closure/phase01-wire-contract.md`.
3. Capture the unfiltered byte-oriented serial log and strict ASCII JSON records. A start event,
   all 17 required records, and a final complete event with zero invalid scenarios are mandatory.
4. Validate producer completion, source/artifact hashes, QEMU machine/hart/RAM/version, units,
   sample counts, peer replies, and setup/measure/teardown outcomes before evaluating targets.
5. Report validity, unchanged target attainment, and historical regression as separate verdicts.

### Regression Detection

`scripts/compare-bench-results.sh` compares each valid current metric against the rolling median
of up to 20 accepted non-regressing observations from distinct, profile-compatible valid captures.
A value more than 10% worse advances the regression streak but is not added to that median. A
regression is sustained only after three distinct consecutive runs exceed the median by more than
10%; replaying one capture ID cannot advance the streak. Invalid runs do not advance or reset
history. Missing/corrupt state is reconstructed from immutable captures or fails the affected
history row closed. The first valid capture for a genuinely new profile is `BASELINE_ONLY`, not a
regression pass.

### Environment

| Parameter | Value |
|-----------|-------|
| Machine | `qemu-system-riscv64 -machine virt -accel tcg -smp 2 -m 256M` |
| Profile | `rv64-qemu-virt-2h-256m-v2` |
| Kernel/cells | `riscv64gc-unknown-none-elf` release build, default init/shell policy |
| BIOS | OpenSBI (default) |
| Runner | GitHub Actions `ubuntu-latest`; local evidence below used QEMU 8.2.2 |

The attached disk remains provenance/environment input. Benchmark launch uses the VIFS1-embedded
cell through the normal shell because the early kernel loader no longer owns the userspace block
Driver Cell.

---

## Baseline Measurements

> **Phase 04 diagnostic result (2026-09-05):** three captures under
> `rv64-qemu-virt-2h-256m-v2` pass the collector's structural validation, but they identify only
> the committed revision and artifact hashes; they do not bind the dirty source boundary that
> produced the benchmark protocol. They therefore do not qualify as the required current-source
> baseline and no Phase-04 history verdict is published. Raw captures and serial logs are retained
> under `.agents/260905-1139-sas-lbi-outcome-closure/evidence/perf-results-phase04-generic/`.
> Values below are diagnostic medians across the three repetitions: QEMU software evidence, not
> hardware qualification.

| Scenario | n per repetition | Median p50 | Median p99 | Median max / value | Target verdict |
|----------|------------------|------------|------------|--------------------|----------------|
| `context_switch` | 1,000 | 11.9 µs | 25.6 µs | 93.2 µs | PASS |
| `ipc_send_recv` | 1,000 | 41.8 µs | 91.7 µs | 170.2 µs | INFORMATIONAL_MISS against the hardware target |
| `syscall_yield` | 1,000 | 6.9 µs | 19.0 µs | 98.9 µs | PASS |
| `memory_footprint` | 1 | — | — | 79,773,696 bytes (76.08 MiB) | FAIL |
| `preempt_latency` | 500 | 35.6 µs | 79.2 µs | 128.8 µs; 0 misses | PASS |
| `control_loop` | 200 | 19.76 ms | 29.78 ms | 29.82 ms; 0 misses | PASS on deadline-miss gate |
| `smp_spawn_rate` | 8 | — | — | 9 operations/s | FAIL |
| `smp_ipc_throughput` | 1,000 | — | — | 24,681 operations/s | PASS |
| `smp_work_distribution` | 2 harts | — | — | 1.89× | PASS |

All three completion records report `invalid=0`. The retained comparator output is not a
cross-revision Phase-04 verdict: the new directory reset compatible retained history, and the
captures lack an immutable dirty-source digest. Recapture with a full-tree/patch digest and rebuild
one canonical history from all compatible retained valid captures before declaring history status.

### Local C2C broker oracle

Three isolated, unprofiled RV64/QEMU oracle logs are retained under
`.agents/260905-1139-sas-lbi-outcome-closure/evidence/c2c-oracle-phase04-rep{1,2,3}.log`.
They do not record the full source/artifact/QEMU profile envelope. Median observations:

| Workload | Success / attempted | Median p50 | Median p99 | Outcome |
|----------|---------------------|------------|------------|---------|
| baseline | 1,000 / 1,000 | 515.2 µs | 844.0 µs | 0 busy/indeterminate/correlation/timing failures |
| concurrency 1 | 1 / 1 | 641.9 µs | 641.9 µs | PASS |
| concurrency 2 | 2 / 2 | 1.14 ms | 1.50 ms | PASS |
| concurrency 4 | 4 / 4 | 1.90 ms | 18.78 ms | PASS |
| concurrency 8 | 8 / 8 | 20.05 ms | 22.89 ms | PASS |
| concurrency 16 | 16 / 16 | 18.98 ms | 21.14 ms | PASS |
| soak | 10,000 / 10,000 | — | — | 0 busy/indeterminate/duplicate/stale/correlation/silent-drop; 65,216-byte queue-body occupancy high-water; 143,930-byte static broker state |
| overflow | 17 / 18 | — | — | expected 1 busy at queue peak 16 |

Network progress remains nonzero, with zero heartbeat misses or watchdog expiry. Restart reports
`PASS`, state reset and retry pass, while stale-send outcome remains explicitly `INDETERMINATE`.
This is a local single-guest broker oracle and supporting evidence, not remote RPC or Tier-3 proof.

## Performance Baseline — Status

**Current status: SOURCE PROVENANCE INCOMPLETE; TARGET DIAGNOSTIC FAIL; HISTORY UNQUALIFIED.**
The collector proves complete 17-record runs and refuses malformed, incomplete, duplicate,
wrong-profile, or wrong-count evidence. A source-bound recapture and named-board hardware
qualification remain open.

The diagnostic allocator commitment is 79,773,696 bytes (76.08 MiB). The fixed kernel heap accounts
for 32 MiB of committed frames. The remaining 44.08 MiB is deliberately unassigned: it includes
live kernel/cell/image/stack/page-table/device allocations that the current global `MemInfo`
contract does not break down. This supersedes the 2026-08-01 observation of 135,782,400 bytes
without rewriting it. Both exceed the unchanged `<10 MiB` objective.

`MemInfo=243` uses allowlist bit 56 and returns the fixed 32-byte `ViMemInfoV1`. It is opt-in
because global used/free totals are cross-cell telemetry. The destructive spawn-exhaustion probe
is included and signed only when `CELLOS_INCLUDE_CAPACITY_PROBE=1`; default images exclude it.

> **Action required:** add immutable dirty-source provenance, rebuild canonical compatible history,
> and recapture first. If the source-bound result preserves the misses, prove allocator high-water
> headroom before reducing commitment and investigate SMP spawn rate. Real grant-backed VFS rows
> and named-board evidence remain separate gates.

## Spec vs. Implementation Gap

The ruled Tier-1 direct-dispatch rewrite is not reachable in current separately linked Cells.
The diagnostic profile-v2 captures measure the existing mediated rendezvous and keep QEMU and
hardware claims separate.

| Metric | Spec Target | Diagnostic QEMU measurement | Status |
|--------|-------------|-----------------------------|--------|
| IPC round-trip | < 50 µs on qualified hardware | median p99 91.7 µs | INFORMATIONAL_MISS; no hardware claim |
| Context switch | < 100 µs QEMU gate | median p99 25.6 µs | PASS |
| Syscall yield | < 40 µs QEMU gate | median p99 19.0 µs | PASS |
| Allocator commitment | < 10 MiB | 76.08 MiB | FAIL |

## Scheduler Impact on Latency

Cellos uses three fixed scheduler tiers, FIFO within a tier, and RT-hart routing on RV64. The diagnostic QEMU captures supply preemption and control-loop observations, but TCG is not cycle-accurate hardware evidence and does not establish a hardware p99 envelope.

---

## How to Run Locally

```bash
# Build the release image as in .github/workflows/perf.yml, then run:
python3 scripts/bench_results.py collect \
  --results-dir perf-results \
  --capture-id <unique-id>-rv64-qemu-virt-2h-256m-v2-1 \
  --profile rv64-qemu-virt-2h-256m-v2 \
  --stdin-line bench --stdin-after "Cellos >" \
  ...provenance and artifact arguments... -- \
  qemu-system-riscv64 -machine virt -accel tcg -smp 2 -m 256M ...

bash scripts/compare-bench-results.sh perf-results \
  --current perf-results/perf-<capture-id>.json \
  --current-id <capture-id>
```

Use the workflow command as the canonical complete invocation; omitted provenance arguments are
required, not optional. The collector retains raw output on every exit.

---

## Adding a New Scenario

1. Create `cells/tests/bench/src/scenarios/<name>.rs` implementing `ViBenchmark`.
2. Export it from `cells/tests/bench/src/scenarios/mod.rs` and invoke it from `main.rs`.
3. Add its exact fields, units, sample count, direction, and target policy to the versioned profile contract.
4. Update collector validation, behavioral comparator tests, workflow profile ID when compatibility changes, and this report.

See `docs/specs/10-testing.md` for the integration test framework that complements these
benchmarks.
