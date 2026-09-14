# Plan — CPU inference engine throughput

**Created**: 2026-09-14
**Ceiling**: host (kernel and token rate), qemu (in-cell characterization)
**Track**: G2 Level-A AI inference (Spec 24 §4), follow-on to `.agents/260913-2002-g2-level-a-ai-inference/`

## Goal

The engine works and is qualified end to end; its *speed* was never measured or tuned. Phase 04 of
the G2 track recorded 3.90–3.97 tok/s on a 135M-parameter Q8_0 checkpoint and named the lever
without pulling it: "the engine is limited by the scalar f32 kernels (~1.1 GFLOP/s here) … SIMD
kernels and a quantised matvec for the remaining dense paths are the real levers".

This slice measures the engine on real checkpoints, finds where the time actually goes, and takes
the wins that are real on the shipping targets — without changing any numerics: the Q8_0 kernel's
bit-exact agreement with the dense kernel is a documented, tested property, and the fixture's pinned
token ids depend on it.

## Method

1. **Instrument first.** `libs/ai-engine/benches/cpu_engine.rs` (a `harness = false` cargo bench):
   engine load, prefill ms/token, decode ms/token and tok/s on a real checkpoint, plus per-kernel
   timings at the *model's own shapes*. Runs on the host; prints `[bench] key=value` lines.
   Numbers are compared between runs, never asserted in CI.
2. **Change one thing at a time**, re-measure, keep what moves the number, revert what does not.
3. **Verify** with the existing unit suites (which pin the numerics) and the QEMU consumer gates.

## Baseline (host, x86_64, `-Oz`, real checkpoints)

| Model | Decode | Q8_0 matvec | f32 matvec |
|---|---|---|---|
| `stories15M-q8_0` (6 layers, 288 embd, vocab 32000) | 24.1 ms/token (41.5 tok/s) | 1.34 GFLOP/s | 9.78 GFLOP/s |
| `SmolLM-135M-Instruct.Q8_0` (30 layers, 576 embd, vocab 49152) | 211.5 ms/token (4.7 tok/s) | 1.35 GFLOP/s | 10.24 GFLOP/s |

The Q8_0 kernel is 7× slower per MAC than the dense one *in the same crate*, and the whole decode
is that kernel: 30 layers of projections plus the tied output projection. Nothing else is close
(`softmax` 127 µs, `sample_top_k` 2.0 ms, `rms_norm` 370 ns per token at 49152 vocab).

## Phases

- [phase-01-kernel-throughput.md](phase-01-kernel-throughput.md) — where the time goes, what was
  kept (`opt-level = 2` for the two hot crates), what was reverted, and the in-cell delta.
  **Result: 5.1× on the host (135M checkpoint 4.7 → 24.0 tokens/s), 13% in-cell, 6 KB smaller cell.**

## Non-goals

- No numerics change: no FMA contraction, no integer Q8×Q8 accumulation, no reordering of the
  accumulation lanes.
- No target-feature changes. AVX2/NEON/RVV kernels need a per-target build decision (the cells also
  run on CPUs without those features, and on a kernel that does not save vector state), so they stay
  open in the roadmap rather than being smuggled into the default profile.
- No multi-threading: a Cell is one thread by design.
