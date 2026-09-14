# Phase 06 — Zero-copy weights and demand-sized KV

**Status**: completed
**Ceiling**: qemu (in-cell) + host (numerics, memory)
**Depends**: 05

## Why this phase exists

The engine copied every tensor out of the model file at load time, so a model cost *twice* its size:
the file plus its weights. Two consequences:

1. A 26.7 MB checkpoint needed ~55 MB and could not fit a Cell whose virtual-address slot is 32 MiB,
   regardless of how the Cell was sized.
2. Every session pre-allocated the model's full declared context. For a 30-layer model with a
   2048-token context that is 94 MB of KV before the first token — which is why SmolLM-135M reported
   231 MiB resident for a 138 MiB file and looked like the weights had been duplicated.

## What changed

- **Weights live in the file.** `Engine::load` takes ownership of the model buffer and describes each
  tensor by `(offset, rows, cols, dtype)`. `ai-tokenizer`-style copies are gone; only a tensor the
  kernels cannot consume as stored — F16, or a float region that is not 4-byte aligned — is converted
  once. The byte→`f32` view is `zerocopy::FromBytes::ref_from_bytes`, which checks alignment and keeps
  the engine `#![forbid(unsafe_code)]`.
- **KV cache grows on demand.** The cache is position-major (`pos * (n_layer * kv_dim) + layer * kv_dim`)
  so growth is a `Vec::resize` rather than a repack, starts at 64 positions, doubles, and stops at the
  model's `n_ctx`.
- **Read sizing.** The VFS client asks for the file's size before reading so the buffer is allocated
  once instead of doubling (which transiently needs ~1.5× the file).
- **Cell sizing is a feature.** `cells/services/ai` keeps a 16 MiB arena by default; `large-arena`
  sizes it for a 25 MB checkpoint. The oracle runner selects it from the deployed model's size, so
  images that serve small models do not pay 29 MB of resident RAM.

## Measured

| Model | File | Resident before | Resident after |
|---|---|---|---|
| SmolLM-135M fine-tune (30 layers, ctx 2048) | 138 MiB | 229–231 MiB | **144 MiB** |
| llama2.c `stories15M` (6 layers, ctx 128) | 25 MiB | ~55 MiB peak during load, could not fit a Cell | **26 MiB, loads under a 30 MiB ceiling** |

Host-side acceptance test: `a_real_checkpoint_fits_a_cell_slot` refuses to pass unless a 26.7 MB
checkpoint loads under 30 MiB *and* stays within `file + 6 MiB` resident.

## Result in a Cell (QEMU RV64)

`CELLOS_AI_REAL_MODEL=.ai-models/stories15M-q8_0.gguf scripts/run-ai-inference-oracle-qemu.sh` builds
the service with `large-arena` and serves the 25 MiB SentencePiece checkpoint from `/bin/ai`:
read 25.5 MB in ~150 s, engine load ~86 ms, **28 MB resident**, and the oracle's scenarios pass.

## Model load: a hypothesis, a measurement, and the fix that worked

The 25 MB load took ~150 s, and the first explanation — `BootFsProxy::read_at` re-opens and re-seeks
per call, so every chunk pays a FAT walk — was **wrong**. Retaining the capability and its cursor in
the backend (`BootFsProxy` now keeps one open `OpenCap` plus its position, re-seeking only when the
caller jumps) recovered 2.4%: 150,126 ms to 146,484 ms. The cost is per *IPC round trip* (~22 ms in
TCG), and there are ~6,700 of them at one 4 KiB message each.

The fix that worked reads the model through the **kernel capability path** instead: `OpenCap` +
`ReadCap` move up to the kernel's 64 MiB user-buffer ceiling per *syscall*, so a 25 MB model needs
~100 syscalls and no cell-to-cell IPC at all — **2,978 ms**, a 49× improvement. `cells/services/ai`
tries that path first (uppercasing for FAT16) and falls back to the VFS service, which is the only
path that can see the on-disk cell-store; the canonical `gen_disk` image exercises exactly that
fallback and still passes. The retained cursor stays: it is correct, and it removes the backwards-seek
shape for callers that jump around a file.

Evidence: `evidence/ai-model-load-paths.txt`.

## Non-claims

- Memory numbers are host or QEMU TCG; no board, accelerator, or production qualification follows.
- `stories15M` remains a 15M-parameter story model: real weights, small-model text quality.
