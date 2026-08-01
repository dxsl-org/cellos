# Cellos Performance Baseline Report

> **Status:** Capacity measured; fixed-priority scheduler shipped; consolidated latency baseline still pending.
> Updated weekly by `.github/workflows/perf.yml`.

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

1. **Warmup:** 100 iterations (discarded) — warms QEMU JIT cache
2. **Measurement:** 1,000 iterations per scenario
3. **Statistics:** sort samples → extract `min`, `p50`, `p99`, `max`
4. **Pass/fail:** QEMU-calibrated rows compare p99 against their emulator gate. IPC p99 is
   recorded as `HW-TARGET-MET`/`HW-TARGET-MISS`; only a qualified hardware run can satisfy the PDR.

### Regression Detection

`scripts/compare-bench-results.sh` compares the current run's p99 against the
rolling median of up to 20 historical runs.  A regression is flagged when a metric
is > 10% above the median for 3 **consecutive** weekly runs (single-run noise is
ignored).  The CI build fails only on sustained regressions.

### Environment

| Parameter | Value |
|-----------|-------|
| Machine | `qemu-system-riscv64 -machine virt -smp 1 -m 128M` |
| Kernel | `riscv64gc-unknown-none-elf` release build |
| BIOS | OpenSBI (default) |
| Runner | GitHub Actions `ubuntu-latest` |

This table describes the scheduled latency run. The 2026-08-01 capacity artifact is a separate
test-mode build used for MemInfo and bounded destructive OOM verification.

---

## Baseline Measurements

> **Not yet available** — requires the first complete QEMU CI run.
>
> Run `./scripts/dev-setup.sh` then boot via `./run.ps1` and observe `[bench]`
> lines in the serial output to capture initial numbers.

Expected rough order-of-magnitude for QEMU (10 MHz `mtime`):

| Scenario | Expected p50 | Expected p99 | Target |
|----------|-------------|-------------|--------|
| `context_switch` | ~20 µs | ~40 µs | < 100 µs |
| `ipc_send_recv` | 48.5 µs measured | 86.6 µs measured | QEMU trend only; hardware target < 50 µs |
| `syscall_yield` | ~5 µs | ~10 µs | < 10 µs |
| `memory_footprint` | 129.49 MiB | — | < 10 MiB — FAIL |

## Performance Baseline — Status

**Current status: PARTIALLY MEASURED.** Capacity observability has a real RV64 measurement,
and D1 captured one QEMU IPC run (48.5 µs p50 / 86.6 µs p99). A consolidated QEMU history and
the named-board hardware qualification run remain open.

On 2026-08-01, `/bin/bench` reported:

```text
[bench] allocator_committed_bytes=135782400
[bench] memory_footprint FAIL (exceeds 10 MB target)
```

The 135,782,400-byte value is 129.49 MiB and comes from exact transition-aware frame accounting,
not the former synthetic 3,500,000-byte constant. The unchanged `<10 MiB` objective therefore
fails honestly. Reducing allocator commitment is separate optimization work; changing the metric
or threshold would invalidate the observability gate.

`MemInfo=243` uses allowlist bit 56 and returns the fixed 32-byte `ViMemInfoV1`. It is opt-in
because global used/free totals are cross-cell telemetry. The destructive spawn-exhaustion probe
is included and signed only when `CELLOS_INCLUDE_CAPACITY_PROBE=1`; default images exclude it.

> **Action required:** Complete and pin the remaining latency baseline. Capacity has been measured,
> but the `<10 MiB` objective needs a dedicated memory-reduction plan.

## Spec vs. Implementation Gap

The ruled Tier-1 direct-dispatch rewrite is not reachable in current separately linked Cells.
The measured message path spends about 0.64 µs on request encode plus reply decode and 48.5 µs
p50 on the full rendezvous in QEMU; the table tracks shipped evidence separately from the target.

| Metric | Spec Target | PDR Target (p99) | Estimated Current | Measured |
|--------|------------|-----------------|-------------------|---------|
| IPC round-trip | Future Tier-1 direct dispatch; unmeasured | < 50 µs on qualified hardware | Message rendezvous | QEMU: 48.5 µs p50 / 86.6 µs p99 |
| Context switch | — | < 100 µs | ~40 µs (estimated) | ❌ Not yet |
| Syscall yield | — | < 10 µs | ~10 µs (estimated) | ❌ Not yet |
| Allocator commitment | — | < 10 MiB | 129.49 MiB | ❌ FAIL (measured 2026-08-01) |

## Scheduler Impact on Latency

Cellos no longer uses a flat round-robin model. The shipped scheduler has three fixed tiers, FIFO within a tier, and RT-hart routing on RV64, but the consolidated latency baseline is still pending. The current report therefore cannot claim an end-to-end p99 envelope yet, and architecture-scoped immediate preemption still needs measurement outside RV64.

---

## How to Run Locally

```bash
# 1. Build kernel + bench cell
cargo build --release -p Cellos-kernel -p app-bench

# 2. Boot with disk image containing /bin/bench
./run.ps1   # or: bash scripts/run-aarch64.sh for AArch64

# 3. At the shell prompt, spawn bench
Cellos> /bin/bench

# 4. Read serial output for [bench] lines
# JSON lines (parseable by compare-bench-results.sh):
#   {"name":"context_switch","n":1000,"min":42,"p50":55,"p99":90,"max":120}
```

---

## Adding a New Scenario

1. Create `cells/apps/bench/src/scenarios/<name>.rs` implementing `ViBenchmark`
2. Add `pub mod <name>;` to `cells/apps/bench/src/scenarios.rs`
3. Add the scenario to `cells/apps/bench/src/main.rs`
4. Define a PDR target constant and `meets_target()` check
5. Update this document with the new metric row

See `docs/specs/10-testing.md` for the integration test framework that complements these
benchmarks.
