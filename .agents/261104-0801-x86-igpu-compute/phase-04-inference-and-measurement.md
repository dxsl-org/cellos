---
phase: 4
title: "LLM inference on the iGPU and Linux comparison"
status: pending
priority: P1
effort: "15d"
dependencies: ["G3"]
tier: thinking
checkpoint: false
---

# Phase G4: LLM inference on the iGPU and Linux comparison

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `cells/services/ai`, `libs/ai-engine`; llama.cpp Vulkan backend (MIT); [Haily roadmap](../../docs/roadmap/haily-personal-assistant.md) comparability rules

## Overview

Run a real model on the iGPU through Vulkan compute, behind the `ai` service, and compare with Linux on the same machine.

## Key Insights

- An iGPU shares system memory bandwidth with the CPU. The gain is mostly prompt processing (compute-bound); token generation is bandwidth-bound and may barely beat the CPU. Measure both.
- Two candidate engines: llama.cpp's Vulkan backend as a Tier-2 Cell, or Vulkan kernels inside `ai-engine`. Pick by measured effort in step 1.

## Requirements

- `ai` service selects the Vulkan GPU path when it holds a GPU client capability and falls back to CPU otherwise, with the choice logged.
- Measurements on the same machine, model, quantization and prompt set: Cellos CPU, Cellos iGPU, Linux CPU, Linux iGPU (Vulkan). Record prompt tokens/s, generated tokens/s, time to first token, memory.

## Related Code Files

- Modify: `cells/services/ai/**`, `libs/ai-engine/**` or create `cells/runtimes/llama-vulkan/**`
- Create: evidence report under this plan; HCL §3 notes for the machine's iGPU

## Implementation Steps

1. Choose the engine.
2. Integrate behind `ai`; fallback path.
3. Measure the four configurations; write the report.

## Todo List

- [ ] Engine choice
- [ ] Integration + fallback
- [ ] Measurement report

## Success Criteria

- [ ] S2: the same prompt produces output through the iGPU path, and the report lists all four configurations with exact versions, including unfavourable results.

## Risk Assessment

- Results may show little gain for generation on an iGPU. That is a valid result and motivates discrete Arc as the next plan.

## Security Considerations

Model files and prompts stay in the `ai` service's authority; the GPU sees only buffers it exports.

## Next Steps

Owner decides: discrete Intel Arc plan, and NVIDIA's route.

## Deviation Log

None.
