# Phase 07 evidence: AVX2 for the guest

Same-round bench runs (`scripts/tier3_chrome_bench.py`), one vCPU, 4-CPU machine, quiet
host (`host_ms` 174-198). `p05` = `~/.cache/cellos-chrome-bench/phase05g-default.iso`
(before this phase). Per-call overhead = (`run_ms` - `guest_ms`) / `runs` of vCPU 1.

## Three benches, medians

| run | images | paint frames | scroll frames | js1 ms | jsN ms | µs per call |
|---|---|---|---|---|---|---|
| `/tmp/cellos-p07-3way` (3 rounds) | p05 / AVX / AVX withheld | 285 / 311 / 263 | 291 / 297 / 273 | 392 / 382 / 397 | 947 / 950 / 981 | 77-84 / 112-122 / 121-123 |
| `bench-phase07b.log` (3 rounds) | p05 / AVX (no per-entry XGETBV) | 290 / 299 | 261 / 292 | 386 / 404 | 996 / 998 | 80-82 / 116-124 |
| `/tmp/cellos-p07-x7b` (2 rounds) | p05 / AVX / AVX, host XCR0 = 7 | 284 / 308 / 306 | 286 / 297 / 294 | 371 / 398 / 379 | — | 76-94 / 115 / 103 (1 run) |

The first AVX bench (`bench-phase07.log`) ran on a host whose load moved between rounds
(`host_ms` 179-474); its medians are not used.

## What it says

- Painting gains 4-9 %, scrolling 2-4 %; JavaScript does not gain (one-thread 0-7 % slower).
- Each `RunVcpu` call costs 30-40 µs more outside the guest once the host runs with
  `CR4.OSXSAVE`, with or without AVX for the guest (the "withheld" image executes no
  `XSETBV`). Removing the per-entry `XGETBV` did not change it. A host XCR0 equal to the outer
  host's (0x7) gave 103 µs in one run. Cause not identified; the outer hypervisors (KVM under
  Hyper-V) are where it is paid.

## Functional checks

- Chrome guest: `CHROME_GUEST_SIMD avx=1 avx2=1` (lane check `guest-avx2`).
- Display lane: `guest-ymm-intact` for both guests (`fpu-check.S`: ymm0..ymm15, 512 bytes,
  across a 2·10⁹-iteration spin), `guest-xmm-intact` unchanged.
