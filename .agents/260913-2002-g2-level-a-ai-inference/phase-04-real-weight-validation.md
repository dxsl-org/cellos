# Phase 04 — Real-Weight Validation

**Status**: completed
**Ceiling**: host

## Deliverable

- `scripts/fetch-ai-test-model.sh` — checksum-pinned fetch of one real small GGUF checkpoint into a
  gitignored cache directory (never committed).
- `scripts/ai-generate-host.rs`-equivalent harness (`cargo run` example or host test) that loads the
  real checkpoint through the same `libs/ai-engine` path used by the Cell and generates text.
- Evidence record: exact model file + sha256, prompt, generated text, tokens/sec, peak memory.

## Result

Evidence: `evidence/ai-engine-real-weights.txt`.

```
[ai-engine] real checkpoint: model=Cosmo2 135M Webinst Sc2 layers=30 vocab=49152
            resident=229 MiB tokens=16 tps=3.97 text=" the same as the numeral, as the number of people who are not in"
```

- Checkpoint: `scripts/fetch-ai-test-model.sh` → `.ai-models/SmolLM-135M-Instruct.Q8_0.gguf`,
  sha256 `76520babb0daebccb6e17d2f38504ece61356a0ca958d8e8795ef4d23c23c1f0` (gitignored; not committed).
- The engine loaded a tied-output-embedding checkpoint (no `output.weight`) and used the tied path.
- 3.97 tokens/s in a release host build. This is the *unoptimized* CPU engine: the tied output path
  dequantises every vocabulary row per token, and the kernels are scalar. Throughput is recorded as
  a baseline, not a target; SIMD kernels and a quantized output projection are the obvious next step.
- The generated text is English-like and prompt-conditioned. It is NOT a factual claim about the
  model's answers: this specific fine-tune is a 135M-parameter experiment, and the test asserts
  text-likeness and non-degeneracy, not correctness.

## Gates (G-D)

- Generated text is non-degenerate (real words, prompt-conditioned) and reproducible for a fixed seed.
- The run is reproducible from a clean checkout with the documented fetch command.
- No claim beyond `host`: this does not qualify any hardware, admission, or production path.
