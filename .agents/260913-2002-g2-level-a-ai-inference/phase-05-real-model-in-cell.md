# Phase 05 — Real checkpoints in a Cell

**Status**: completed
**Ceiling**: qemu (in-cell) + host (numerics)
**Depends**: 01, 02, 03

## Why this phase exists

Phases 01–04 proved the machinery against a deterministic fixture and ran real weights only on the
host. That left the headline claim — "a Cell on the device runs a real model" — untested, and it
turned out to hide three defects that the fixture could not expose:

1. **Attention was unscaled.** Every Llama-family implementation divides the attention scores by
   `sqrt(head_dim)`; the engine did not. The fixture still matched its (equally unscaled) Python
   reference, so the golden oracle stayed green while real weights produced word salad. Found by
   running `stories15M`: `"Once upon a time"` continued as `", was a a flew fle fle fle ridw …"`
   before the fix and `", there was a big bear named Benny was a very big bear…"` after it.
2. **SentencePiece vocabularies were refused** (`tokenizer.ggml.model = "llama"`), which excludes
   Llama-2/TinyLlama/llama2.c — exactly the models small enough to fit a Cell.
3. **Control tokens were not matched before pre-tokenization.** `<|im_start|>` is one token in the
   SmolLM vocabulary (type CONTROL); the tokenizer shredded it into characters, so a chat-templated
   prompt reached the model as noise (measured: repeated newline ids).

Running a real checkpoint in a Cell also exposed a **shared VFS defect**: `ReadFileHandle` read the
entire file into a fresh `Vec` on every request, so a chunked read cost O(file²). At the 103 KB
fixture that was invisible; at 1.18 MB the service stalled for minutes. Fixed by reading the
requested range (`VfsManager::read_at`) and giving the VIFS1 backend a positional read on the
`SeekCap` syscall (which shares `ReadCap`'s allowlist bit, so no new authority).

That first cut was itself incomplete, and CI said so: `RamFsBackend` (the `/tmp` scratch space the
shell writes and reads back) left `read_at` at the trait default, so every redirected write read
back as empty — 10 shell-utility scenarios went red. The backend now implements the positional read,
and the dispatch path falls back to the whole-file copy when a backend answers a non-empty request
with nothing, so a future backend cannot silently truncate. Re-running the CI commands locally:
shell-utils PASS (was 81 PASS / 10 FAIL), vfs-quota 2/2, oracle PASS on both paths; and on the
hosted runner the three jobs are green again (`Shell Utilities`, `VFS Quota`, `AI Inference
Oracle`), with `redoxfs-srv`'s `degrade_no_disk` confirmed pre-existing by re-running it against the
pre-change VFS cell.

## Deliverables

- `libs/ai-tokenizer`: SentencePiece (`llama`) family — U+2581 escaping, per-character symbols, byte
  fallback, score-ordered merges, control/unknown/unused-aware decoding — plus special-token
  splitting for both families, and `gguf-rs::metadata_i32_array` for `tokenizer.ggml.token_type`.
- `libs/ai-engine`: `1/sqrt(head_dim)` attention scaling; host tests for both checkpoint classes.
- `cells/services/ai`: 16 MiB arena sized for the story model's 2048-token context; read/load timing
  reported on the console.
- `cells/tests/ai-test`: the oracle now decides its scenario from the service's truthful `Describe`
  (fixture ⇒ pinned ids and embedding; real checkpoint ⇒ tokenizer round-trip, text-like
  continuation, non-empty embeddings) and runs the abandonment and streaming scenarios for both.
- `scripts/run-ai-inference-oracle-qemu.sh`: `CELLOS_AI_REAL_MODEL=<path>` deploys a real checkpoint
  instead of the fixture and switches the required markers.
- `cells/services/vfs` + `libs/ai-engine` are the only shared components touched; the VFS change is
  a correctness-preserving performance fix.

## Evidence

| What | Where |
|---|---|
| Real SPM checkpoint in a Cell, QEMU RV64 | `evidence/ai-real-model-in-cell.txt` (`ai` log: 1,185,376 B read in 5.4 s, engine load 8 ms, 24-token continuation, `[ai-test] PASS`) |
| Fixture regression through the same code | same file, second section |
| Canonical `gen_disk.ps1` image (cell-store read path) | `evidence/ai-oracle-canonical-image.log` regenerated in this phase |
| Real weights on the host, both checkpoint classes | `evidence/ai-engine-real-weights.txt` |
| 94 host tests across the six crates | `cargo test -p ai-proto -p ai-sdk -p tensor-math -p gguf-rs -p ai-tokenizer -p ai-engine --target x86_64-unknown-linux-gnu` |

## Non-claims

- The in-cell model is `stories260K`: real trained weights, toddler-level text. `stories15M` (26.7 MB
  of weights) does **not** fit a Cell yet — the engine still copies every tensor out of the model
  buffer, so a 26.7 MB file needs ~55 MB, above the 32 MiB Cell VA slot. Zero-copy weight loading
  (`Engine` borrowing the model buffer) is the next lever and is recorded in the plan.
- 5.4 s to read 1.18 MB through the VFS is now linear but still slow in QEMU TCG; it is not a
  hardware throughput claim.
- Nothing here qualifies an accelerator, a board, or a production path.
