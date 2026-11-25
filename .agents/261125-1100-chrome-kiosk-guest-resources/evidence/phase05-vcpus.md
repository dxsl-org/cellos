# Phase 05 evidence: more vCPUs

All numbers are medians of same-round bench runs (`scripts/qemu-tier3-chrome-bench-x86.sh`,
targets benched in turns). `host_ms` is the build host's fixed loop: 175-190 = quiet.
Baseline ISO: `~/.cache/cellos-chrome-bench/phase04e-deadline-in-kernel.iso` (one vCPU).
Logs: `/tmp/cellos-tier3-bench/bench-phase05*.log`.

## Default build (one vCPU), 4-CPU machine, 2 rounds (`bench-phase05g.log`)

The kernel side of this phase (per-vCPU locks, host interrupts taken inside a `RunVcpu`
call) runs for the one-vCPU guest too.

| metric | head (phase 05, 1 vCPU) | phase 04e |
|---|---|---|
| host_ms | 180 | 186 |
| ui_s | 21.9 | 22.4 |
| raf_frames | 302 | 293.5 |
| timer_p95 ms | 1.6 | 1.6 |
| js1_ms | 370 | 381 |
| jsN_ms | 943 | 957 |
| scroll_frames | 292.5 | 285.5 |
| paint_frames | 297.5 | 281 |

## Four vCPUs (`/chrome-guest-vcpus` = 4), 8-CPU machine, 3 rounds (`bench-phase05e.log`)

| metric | 4 vCPUs | phase 04e (1 vCPU) |
|---|---|---|
| host_ms | 187 | 189 |
| ui_s | 23.4 | 22.4 |
| raf_frames | 279 | 302 |
| timer_p95 ms | 3.2 | 1.3 |
| js1_ms | 385 | 370 |
| jsN_ms | 517 | 958 |
| scroll_frames | 155 | 283 |
| paint_frames | 242 | 286 |

## Steps that led there

| run | change | result |
|---|---|---|
| `bench-phase05.log` | 4 vCPUs on a 5-CPU machine, AP runs 1 ms + yield | raf 148 vs 301, timer p95 17 vs 3 ms (loaded host) |
| `bench-phase05-smp8.log` | same, 8-CPU machine | jsN 529 vs 1015, raf 302 vs 302, scroll 189 vs 281 |
| `bench-phase05b.log` | AP re-enters after `Preempted` without yielding | no UI in 2 of 2 runs (Grant bug present too) |
| `bench-phase05c.log` | 2 vCPUs, 8 CPUs | jsN 723 vs 972, scroll 189 vs 283 |
| `bench-phase05d.log` | host interrupts taken inside `RunVcpu` | paint 271 vs 283, scroll 194 vs 286 |
| `bench-phase05e.log` | GPU served on the first vCPU's thread only | table above; 3 of 3 rounds reported |

First SMP lane run (`/tmp/lane-p05.log`, 10 ms AP runs): the first vCPU's `away_ms` was
119,979 of a ~185 s run and the resize took 121 s; with 1 ms AP runs it was 11,533 and the
resize took 1 s.

## Chrome lane (four vCPUs, 24 checks)

Final design: passed 24/24 in four of five runs on 2026-11-26 (UI after 24-27 s, resize
followed in 1 s). The failed run (`relaunch-shows-chromium`): the first vCPU's thread was
away 249 s of ~330 s (`[hv-x86] vm 1 vcpu 1 stats: … away_ms=249336`), cage's first frame
came 300 s after it started; in the passing run after it `away_ms` was 14,381. Idle vCPUs
still hold their host CPU for up to 1 ms per run; making an idle vCPU block until its next
timer needs a one-shot host timer (owner deferred, 2026-11-26).
