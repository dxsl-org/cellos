# G2 Level A — Native AI Inference Service Implementation Plan

**Goal**: implement [Spec 24](../../docs/specs/24-ai-inference-architecture.md) on the CPU path so an
in-tree client Cell can submit a prompt to a native AI inference service Cell and receive real
generated tokens, with no Linux guest, no NPU, and no vendor runtime.
**Stage**: G2 Level A (per [project-roadmap-legacy.md §G3 NPU path](../../docs/project-roadmap-legacy.md)).
**Plan owner**: Main (solo maintainer, ADR-0013).
**Status**: completed (phases 01-04; CP-2/CP-4/CP-5/CP-6/CP-7 remain gated)
**Priority**: P1
**Evidence ceiling**: `host` for engine numerics and real-weight generation; `qemu` for the
service/IPC/oracle path. No physical, production, admission, or G3 claim is made by this plan.

---

## 1. Scope Contract

### In scope

- `libs/ai-proto` — the typed wire contract for the unified AI inference service (Spec 24 §6
  opcodes, records, limits, error vocabulary, capability description).
- `libs/ai-sdk` — `AiClient` (Spec 24 §6 public interface) over typed IPC.
- `libs/tensor-math`, `libs/gguf-rs`, `libs/ai-tokenizer`, `libs/ai-engine` — the CPU engine:
  GGUF v3 parsing with Q8_0/F16/F32 dequantization, byte-level BPE tokenizer, f32 kernels, and a
  Llama-architecture transformer runner with KV cache and bounded sampling.
- `cells/services/ai` — the unified inference service Cell (single event loop, bounded cooperative
  generation slices, bounded session table).
- `cells/tests/ai-test` — QEMU oracle client cell (spawns/uses the service, asserts tokens).
- Image/launch wiring: kernel launch profile, init service table entry, `gen_disk.ps1`,
  `scripts/build-boot-ramdisk-ci.sh`.

### Explicitly out of scope (documented, not stubbed)

- **CP-2 Tier 2 GGML backend** — Tier 2 has no public application admission/loader route
  (`organization-deployment-profiles.md` §Activation 3). Not attempted here.
- **CP-4 NPU / CP-5 GPU backends** — hardware-gated (RK3588 / Vulkan). `DeviceTarget` values exist
  in the wire vocabulary; the service reports them unsupported (`AiError::NotSupported`) with the
  exact reason in `Describe`, which is truthful capability reporting, not a silent fallback.
- **True async `TokenStream`** — the async reactor lane (Spec 20 §5) is not landed. `AiClient::prompt`
  keeps the Spec 24 signature shape but is backed by a bounded poll loop (`Iterator`), and the
  service holds sessions across calls, so streaming semantics (incremental tokens, cancel) are real.
- **Q4_K_M / K-quants, F16 CUDA-style kernels, multi-model serving** — later engine work.

## 2. Phases

| Phase | Title | Deliverable | Status | Depends | Ceiling |
|---|---|---|---|---|---|
| 01 | [Wire contract](./phase-01-wire-contract.md) | `libs/ai-proto`, `service::AI`, Spec 17 registry row, host tests | completed | — | host |
| 02 | [CPU engine](./phase-02-cpu-engine.md) | `gguf-rs`, `ai-tokenizer`, `tensor-math`, `ai-engine` + golden-oracle numerics | completed | — | host |
| 03 | [Service + oracle](./phase-03-service-and-oracle.md) | `cells/services/ai`, `cells/tests/ai-test`, launch/init/image wiring, QEMU oracle | completed | 01, 02 | qemu |
| 04 | [Real-weight validation](./phase-04-real-weight-validation.md) | Real GGUF weights generated on the host; tokens/sec + memory recorded | completed | 02 | host |
| 05 | [Real checkpoints in a Cell](./phase-05-real-model-in-cell.md) | SentencePiece + special tokens; attention scaling; a real checkpoint served from `/bin/ai` in QEMU | completed | 01-03 | qemu |

## 3. Hard Gates

- **G-A (contract)**: `AiRequest`/`AiResponse` encode/decode round-trips, bounded-payload rejection,
  and byte-0 registry amendment — verified. Law 1: **frozen** — 2 of 2 confirmations recorded
  2026-09-14 ([record](./law1-confirmation.md), digest-checked).
- **G-B (engine correctness)**: engine output must match an independent reference implementation on
  a deterministic tiny model — same logits within `1e-4`, same greedy token ids exactly.
- **G-C (service path)**: QEMU RV64 oracle prints exactly one `[ai-test] PASS` marker; no cell fault,
  no kernel panic; the service survives a client that exits mid-session.
- **G-D (real weights)**: a real GGUF checkpoint generates non-degenerate text on the host
  (documented command + sample output + tokens/sec). Weights are fetched into a gitignored cache,
  never committed.

## 4. Evidence

| Gate | Where |
|---|---|
| G-A contract | `libs/ai-proto` 6/6, `libs/ai-sdk` 8/8 host tests; Spec 17 §3 registry row |
| G-B engine | `libs/ai-engine` golden-oracle test (independent Python reference in `scripts/gen-ai-test-model.py`); `gguf-rs` 20/20, `ai-tokenizer` 19/19, `tensor-math` 22/22 |
| G-C service path | `evidence/ai-oracle-20260913T230711Z.log` — QEMU RV64, one `[ai-test] PASS`, no cell fault or panic; runner `scripts/run-ai-inference-oracle-qemu.sh` |
| G-E real checkpoint in a Cell | `evidence/ai-real-model-in-cell.txt` — `stories260K` (SentencePiece, real trained weights) read, loaded and served from `/bin/ai` in QEMU RV64: 24-token continuation, all four oracle scenarios `PASS` |
| G-D real weights | `evidence/ai-engine-real-weights.txt` — 30-layer Q8_0 checkpoint, 16 tokens at ~3.9 tok/s (scalar kernels; SIMD is the lever), 229 MiB resident, non-degenerate text |

## 5. Non-claims

- QEMU results are software-only; they do not qualify G2 server/PC hardware, admission, or production.
- Random/synthetic test models prove machinery and numerics only; language quality is claimed only
  for the real-weight host run (G-D) at the `host` ceiling.
- This plan does not activate ORG-SRV-01/ORG-PC-01 application compatibility work.
- The AI interface is **frozen** as of 2026-09-14 (Law 1, 2 of 2 confirmations —
  [record](./law1-confirmation.md)). Changing it now requires the ABI process; that is a governance
  fact about the interface, not evidence that any accelerator, hardware, or application path works.
