# Plan — Integer Q8_0 activation path

**Created**: 2026-09-14
**Ceiling**: host (kernel and token rate), qemu (in-cell token rate); no hardware claim
**Track**: G2 Level-A AI inference (Spec 24 §4), follow-on to `.agents/260914-cpu-engine-optimization/`

## Goal

The previous slice made the Q8_0 matvec build at `-O2` instead of `-Oz` and bought 5.1× on the
host and 13% in the cell, but it left the kernel's *shape* alone: every weight block is decoded to
`[f32; 32]` on the stack and then dotted with the activation vector. Measured, that costs 1.9× the
dense kernel in the same crate (6.8 vs 12.8 GFLOP/s) and dominates the cell, where QEMU's TCG sends
every f32 operation through softfloat.

This slice replaces the f32 staging with the shape every production CPU inference stack uses for
Q8_0 weights: **quantize the activation to Q8_0 once per projection, accumulate the 32 products of
each block exactly in `i32`, and scale the block sum by `d_w · d_a` in f32** — 32 integer MACs and
2 f32 multiplies per 32 weights, against 32 f32 MACs plus a 32-element f32 decode (i8→f32 convert,
scale multiply, store) today.

`-O3` evidence already says why this should pay in the cell specifically: in TCG every f32 add/mul
is a softfloat call, so op *count* decides, and this removes ~2/3 of them from the inner loop.

## Honest cost

The engine stops being bit-exactly equal to f32 accumulation over dequantized weights — that
property is documented and tested today, and it is structurally impossible once activations are
rounded to 8 bits. The replacement invariants:

- The integer part is exact (a 32-term `i8`×`i8` dot fits `i32` with 4 orders of magnitude to
  spare), so the only error is activation quantization, bounded per block by half a scale step.
- A tolerance-based test pins the new kernel against the f32-staging kernel over randomized blocks.
- The fixture's greedy token ids must not move. Measured before touching Rust, with the reference
  implementation: integer accumulation with quantized activations reproduces the same eight ids
  (margins 0.38 vs 0.44 on the weakest step, well above the fixture's 0.05 fragility floor).

## Method

1. `tensor-math`: `quant::q8_0_row_from_f32` (GGML's rule: `d = amax/127` stored as f16, `q =
   roundf(x/d)` half-away-from-zero, clamp to `i8`) and `f32_to_f16` (round-to-nearest-even, the
   inverse of the existing `f16_to_f32`), then `matvec_q8_0_int8`.
2. `ai-engine`: quantize each projection's input into a scratch row sized `max(n_embd, n_ff)` and
   call the integer kernel. The loader already refuses `cols % 32 != 0` Q8_0 tensors, so there is no
   alignment fallback to invent.
3. The reference generator (`scripts/gen-ai-test-model.py`) mirrors the shipped arithmetic, so the
   golden file keeps meaning "what an independent implementation of the same contract produces".
   The frozen model bytes must not change; only the golden values may.
4. Measure host (`benches/cpu_engine.rs`) and in-cell (oracle on the canonical image), then verify
   with the unit suites and the QEMU consumer gates.

## Non-goals

- No SIMD/target-feature change: AVX2/NEON/RVV stay a separate per-target build decision, because
  cells also run on CPUs without those features and under a kernel that does not save vector state.
- No Q4_K/Q5_K quantizers and no GPU/NPU backend: the wire and storage formats are frozen by Spec 24
  and the accelerator envelope is G3.
- No multi-threading: a Cell is one thread by design.
- No change to attention, RoPE, sampling, tokenizer or the IPC contract.
