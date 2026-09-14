# Phase 01 — Integer Q8_0 activation path

**Status**: completed
**Ceiling**: host (kernel and token rate), qemu (in-cell token rate); no hardware claim

## What changed

`tensor-math` gained two functions and the engine switched to them:

- `quant::q8_0_row_from_f32(x, out)` — quantize one activation row into the *same* Q8_0 layout the
  weights already use (`d = amax/127` stored as f16, `q = roundf(x/d)` clamped to `i8`), plus
  `quant::f32_to_f16` as the round-to-nearest-even inverse of the existing decoder.
- `matvec_q8_0_int8(out, w_q8, x_q8, rows, cols)` — per 32-weight block, a `i32` dot product of the
  stored `i8` weights and the quantized activation, scaled once by `d_w · d_a` into four f32 lanes.
- `ai-engine`: `matvec_into` quantizes a `Q8_0` matrix's input into `Scratch::xq` (sized for the
  widest projection input) and calls the integer kernel. `F32`/`F16` tensors keep the dense path.

Per 32 weights the inner loop goes from *32 f32 converts + 32 f32 multiplies + 32 f32 loads + 32 f32
MACs* to *32 integer MACs + 2 f32 multiplies*.

## Honest cost: the numerics change

The engine is no longer bit-exactly equal to f32 accumulation over dequantized weights; that property
was documented and tested in the previous slice, and it is structurally impossible once activations
are rounded to 8 bits. What replaces it:

- The integer part is **exact**: `|Σ| ≤ 32·128² = 524_288`, four orders of magnitude inside `i32`, so
  the only error is the activation's half-step quantization. Pinned by
  `matvec_q8_0_int8_stays_within_the_activation_quantization_bound`, which derives the bound
  (`Σ_blocks d_w·d_a·0.5·Σ|q|`) and asserts the measured error is inside it — not a tolerance chosen
  after the fact.
- `matvec_q8_0_int8_is_exact_when_quantization_is_exact` goes further: with both operands exactly
  representable at scale 0.5 the integer kernel equals the dense kernel **bit for bit**, so the block
  scaling and lane order add nothing of their own.
- The `f32`-staging kernel `matvec_q8_0` stays: it is the reference the integer kernel is bounded
  against and the benchmark's A/B row.

**Fixture**: the reference forward pass in `scripts/gen-ai-test-model.py` now runs the same
Q8_0 × Q8_0 integer arithmetic (packed weight bytes, quantized activations, integer block sums), so
the golden file keeps describing the shipped contract instead of a different one. Measured before any
Rust changed, with the reference implementation, and confirmed after: the eight golden token ids are
**unchanged**; the weakest greedy margin moves 0.44 → 0.38 (7.6× the fixture's own 0.05 fragility
floor), and logit/embedding values shift by ~3% (logits) and ~1e-3 (embedding). The frozen model bytes
are untouched — the generator's quantizer now uses GGML's half-away-from-zero rule, which changes none
of the 91,008 fixture weights (verified: zero exact-half cases).

## Measurements

Host, x86_64, release profile (`-O2` for the two hot crates since the previous slice):

| Kernel shape (135M model) | f32 staging | integer | ratio |
|---|---|---|---|
| `attn_q` 576×576 | 6.78 GFLOP/s | 12.68 GFLOP/s | 1.87× |
| `ffn_gate` 1536×576 | 6.77 | 12.43 | 1.84× |
| `ffn_down` 576×1536 | 6.79 | 12.39 | 1.82× |
| `logits` 49152×576 | 6.64 | 12.03 | 1.81× |
| dense f32 `attn_q` | — | 11.75 | integer beats dense |

| Workload | `-Oz` (before both slices) | `-O2` (slice 1) | `-O2` + integer (now) |
|---|---|---|---|
| `stories15M` decode | 24.1 ms/token | 4.67 ms/token | **2.53 ms/token** (395 tok/s) |
| 135M Q8_0 decode | 211.5 ms/token | 41.6 ms/token | **23.6 ms/token** (42.5 tok/s) |
| 135M prefill | 132.7 ms/prompt-token | — | 18.6 ms/prompt-token |

In-cell QEMU RV64 (`stories15M-q8_0`, 24 tokens, `scripts/run-ai-inference-oracle-qemu.sh`):

| Configuration | Generate time | Tokens/s |
|---|---|---|
| `-Oz`, f32 staging | 7852, 7888, 7916 ms | 3.0 |
| `-O2`, f32 staging | 6709, 6986 ms | 3.5 |
| `-O2`, integer activations | 1595, 1601 ms | 15.0, 15.0 |

That is the largest win of the three slices and the one the cell was paying for: QEMU's TCG emulates
every f32 multiply and add as a softfloat call, so removing ~2/3 of the inner loop's f32 operations
moves the cell 4.3× where it moves the host 1.8× — and unlike `-O3`'s host-side gain, it does not
trade the cell away. Cumulative against the pre-optimization engine: **5.0× in-cell** (7.9 s → 1.6 s)
and **9.0× on the host** (211.5 → 23.6 ms/token on the 135M checkpoint).

## Non-claims

- Host and QEMU ceilings only: no board, NPU, or production claim. The in-cell ordering of kernel
  shapes is an emulator property; silicon can only be measured on silicon.
- The activation quantizer divides by the *stored* f16 scale, not GGML's unrounded register value.
  That is deliberate (it keeps the error inside half a step of what a reader multiplies by) and is a
  documented divergence from `quantize_row_q8_0_ref`, which matters only if CP-6 later claims
  bit-exactness with GGML itself.
- No SIMD/target features, no Q4_K/Q5_K, no multi-threading: unchanged from the previous slice's
  non-goals.
