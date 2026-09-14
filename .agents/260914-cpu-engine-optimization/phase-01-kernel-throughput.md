# Phase 01 — Kernel throughput

**Status**: completed
**Ceiling**: host (kernel and token rate), qemu (in-cell token rate); no hardware claim

## Where the time went

Baseline measured with `libs/ai-engine/benches/cpu_engine.rs` on the host (workspace release
profile), evidence `evidence/bench-baseline-*.txt`:

| Model | Decode | Q8_0 matvec | f32 matvec |
|---|---|---|---|
| `stories15M-q8_0` (6 layers, 288 embd, 32000 vocab) | 24.1 ms/token | 1.34 GFLOP/s | 9.78 GFLOP/s |
| `SmolLM-135M-Instruct.Q8_0` (30 layers, 576 embd, 49152 vocab) | 211.5 ms/token | 1.35 GFLOP/s | 10.24 GFLOP/s |

The decode is entirely the Q8_0 matvec: 30 layers of projections plus the tied output projection —
which alone is 43.4 ms of the 211.5 ms token, because the vocabulary is 49152 and that projection is
21% of all the model's weights. Everything else is noise: `softmax` 127 µs, `sample_top_k` (k=40)
2.0 ms, `rms_norm` 370 ns, 48 KiB of KV per position.

The Q8_0 kernel was 7× slower per MAC than the *dense* kernel in the same crate. The reason was not
the quantized format, the staging block, or the dequantize multiply — the emitted assembly says it
plainly: at `-Oz` LLVM leaves the per-block helpers **out of line**. Six calls per 32-weight block
(`quant::q8_0_block_to_f32`, `accumulate_products`, `chunks_exact`, `Zip::new`, `q8_0_row_bytes` and
an indirect one), and the same shape on riscv64. A 32-weight block carries ~100 multiply-adds, so
six calls and a stack round trip through the decoded block is most of the work.

## Result

| Configuration | Q8_0 matvec | 135M decode | `stories15M` decode | in-cell (QEMU RV64) | `service-ai` |
|---|---|---|---|---|---|
| `-Oz` (workspace default) | 1.35 GFLOP/s | 209 ms/token | 24.1 ms/token | 7.89 s | 236,328 B |
| **`-O2` (shipped)** | **6.8 GFLOP/s** | **42 ms/token** | **4.7 ms/token** | **6.85 s** | **230,288 B** |
| `-O3` | 10.5 GFLOP/s | 29 ms/token | 3.1 ms/token | 8.77 s | 228,392 B |

`[profile.release.package.{tensor-math,ai-engine}] opt-level = 2` — the only change that ships.

- Host: 5.1× (135M checkpoint: 4.7 → 24.0 tokens/s) and 5.2× (`stories15M`: 41.5 → 214 tokens/s).
- In-cell: 13% faster (7.89 → 6.85 s for 24 tokens, three `-Oz` runs and two `-O2` runs).
- Image: 6 KB *smaller* than the size-optimized default — that default was paying for calls.

`-O3` was rejected despite being the fastest on the host: in the cell it is 11% *slower* than the
default it replaces (8.77 s, three runs, ±1%). TCG emulates every f32 op through softfloat, so the
kernel's cost is dominated by op count, and the longer unrolled body of `-O3` costs more in
translation shape than its inlining saves. Both targets are measured; neither is extrapolated.

In-cell measurement: `CELLOS_AI_REAL_MODEL=$PWD/.ai-models/stories15M-q8_0.gguf
scripts/run-ai-inference-oracle-qemu.sh`, with a cell-side timer added to `cells/tests/ai-test`
(`GetTime`, cell-observed: submit → every poll → tokens). `-Oz` 7888/7916/7852 ms, `-O2` 6986/6709 ms,
`-O3` 8723/8756/8843 ms. Raw logs: `evidence/oracle-*.txt`.

## What was measured, then reverted

Recorded so the next person does not pay for them twice:

- **`#[inline(always)]` on the per-block helpers, at `-Oz`**: no effect (1.32 GFLOP/s, unchanged).
  Inlining alone is not the win — the decoded 32-element block stays in memory without the unrolling
  and promotion that `-O2`/`-O3` add.
- **Fusing the decode into the accumulation loop** (i8 converted inside the dot, no staging block):
  10.3 → 7.5 GFLOP/s at `-O3`. A conversion buried in the accumulation loop defeats the widening of
  both stages.
- **Processing two rows per block** for ILP, bit-identical by construction: 10.4 → 10.3 GFLOP/s,
  i.e. nothing. The row loop is already data-parallel and LLVM interleaves it.

## Non-claims

- Throughput numbers are host x86_64 and QEMU TCG. No board, accelerator, or production claim, and
  no claim about a real FPU (the TCG ordering of `-O2` over `-O3` is an emulator property, and a
  silicon re-measurement is the way to settle it).
- The resident test model's output is not language quality and is not asserted here; the numerics
  are pinned by `tensor-math`'s bit-exactness tests and by the fixture's golden ids, both unchanged.
- `sample_top_k` (k=40, 49152 vocab) stays at 1.1–1.7 ms: it is off every shipping path (both
  consumers ask for greedy, and `temperature_milli == 0` short-circuits to `argmax`), and rewriting
  its selection is not this slice's business.

## Open levers

- **Wider ISA kernels** (AVX2/FMA, NEON, RVV): the remaining 2–4× on the host. Needs a per-target
  decision — `core::arch` needs `unsafe`, `target-feature` must be chosen per build (cells also run
  on CPUs without the feature, under a kernel whose vector-context switching is a separate
  question), and FMA contraction would break the kernel's bit-exact agreement with the dense path
  unless both kernels move together.
- **Integer Q8_0 × Q8_0 accumulation**: with SIMD it is the largest remaining win, and in a *TCG*
  cell — where every f32 op is softfloat — it would replace three f32 ops per weight with one
  integer MAC. It quantizes the activations, so the fixture's golden ids have to be re-derived
  first.
- **`sample_top_k`**: O(distinct × n) with two passes per candidate; see the non-claims above.
