# Phase 07a — local inference backend (`llm-gateway` asks `/bin/ai` first)

## Context Links
- [plan.md](./plan.md) · [architecture.md](./architecture.md) · [os-gaps.md](./os-gaps.md)
- Cell: `cells/apps/hypha/llm-gateway/` · Service: `cells/services/ai/` ([Spec 24](../../docs/specs/24-ai-inference-architecture.md))
- Slice plan: [Spec 24 phases 01–07](../../.agents/260913-2002-g2-level-a-ai-inference/plan.md)

## Overview
- **Priority**: P7a (P7 proper — the NPU backend — stays G3-gated).
- **Status**: completed (2026-09-14) — QEMU gate green 3/3, `hypha-boot`/`hypha-p3-boot` revived.
- **Description**: the gateway answers a turn from the on-device inference service Cell
  (`service::AI = 15`, `/bin/ai`) when one is registered, has a model, and the prompt fits one AI
  IPC message. The network endpoint stays as the second backend, so a board or image without
  `/bin/ai` behaves exactly as before.

## Key Insights
- **The gateway is the LLM client cell.** P7's stated design is "swap `llm-gateway` backend to a
  local model"; this phase does that swap for the CPU service that already exists. Putting the
  local path in `core` instead was rejected: it would duplicate the reply classification and add a
  second LLM client, and `agent-proto` would have to carry two contracts.
- **Two policy rules, not plumbing.** (1) One AI request is one IPC message, so a prompt above
  `ai_proto::MAX_PROMPT_BYTES` (2048) cannot be submitted locally — such a turn is a request for
  the larger-context backend, which is the network one. (2) Only *absence* falls back:
  `NoService`/`NoModel` mean "no local inference here"; `Busy`, a mid-session transport error, or a
  poll limit is reported, because silently re-running the prompt against a remote endpoint would
  hide contention and double the latency of a turn. Both rules are host-tested.
- **Capabilities are unchanged.** The gateway declares `[Send, Recv, Log, LookupService]`; the AI
  client resolves the service through the registry (`LookupService`) and talks typed IPC. No new
  authority, no `network` capability, no `agent-proto` change.
- **Which backend served is printed**, so a serial log alone distinguishes a local turn from a
  networked one (the same truthfulness rule the HTTP front end follows).
- **The canonical image carries the deterministic fixture**, so the QEMU gate proves the wiring
  (prompt → local service → text → reply line), not language quality.
- **Finding: `/bin/hypha` was unreachable from the console.** The first gate run reproduced it —
  `DENY launch edge: caller=18 name=shell route=Elf target=/bin/hypha` followed by
  `shell: command not found: hypha`. The reviewed `(shell, Path, /bin/hypha)` edge exists and
  carries `spawn`, but the VFS+grant attempt goes down the ELF route, which
  `launch_profile::authorize` refuses for any capability-bearing target ("caller-owned bytes must
  not borrow authority"). The documented fallback — the raw `SpawnFromPath` route — resolves
  through the kernel loader, which reads VIFS1 and then the block table; `EarlyLoader::probe()`
  however runs before any block driver exists on RV64 and is never retried, so the block table
  stays unprobed and VIFS1 is the only source that answers. `/bin/hypha` was in neither, so both
  routes failed. Fixed here by staging `/bin/hypha` (and `/bin/tool-spawn`, the same case on
  Hypha's own child edge) into VIFS1 in `gen_disk.ps1` — the same class as `bench`, which is
  kernel-spawn-bound for the identical reason. The stale `hypha-boot`/`hypha-p3-boot` suites never
  caught this because nothing runs them in CI — with the staging in place they pass again (8 s each)
  and are now wired into `boot-suite` as the regression guard for that edge.

## Requirements
- `cells/apps/hypha/llm-gateway`: `ai-proto` + `ai-sdk` deps; `local.rs` policy module with host
  tests; `complete()` becomes local-first with an explicit, printed fallback.
- No change to `cells/apps/hypha/core`, `libs/agent-proto`, `libs/ai-proto`, or `libs/ai-sdk`.
- New integration gate `tests/integration/tests/hypha-local-ai.rs` on the canonical `disk_v3.img`.
- CI: the canonical-image consumer gates run in `boot-suite`; the gateway's host tests run in
  `unit-tests`.

## Related Code Files
- **Modify**: `cells/apps/hypha/llm-gateway/{Cargo.toml,src/lib.rs,src/main.rs}`.
- **Create**: `cells/apps/hypha/llm-gateway/src/{local.rs,local-tests.rs}`,
  `tests/integration/tests/hypha-local-ai.rs`.
- **Modify (image)**: `gen_disk.ps1` — stage `/bin/hypha` and `/bin/tool-spawn` into VIFS1 so the
  kernel-resolved path route can serve them (see the finding above).
- **Modify (CI)**: `.github/workflows/ci.yml` (`boot-suite` step, `unit-tests` packages).

## Todo List
- [x] `local.rs` policy + host tests (`prompt_fits_wire`, `network_fallback_allowed`)
- [x] `complete()` local-first in the gateway, backend printed
- [x] riscv64 build + host tests green
- [x] stage `/bin/hypha` + `/bin/tool-spawn` into VIFS1 (launch-edge finding)
- [x] canonical image rebuilt (`gen_disk.ps1`) with the new gateway
- [x] `hypha-local-ai` QEMU gate green, 3/3 runs
- [x] CI wiring + docs/evidence

## Status: completed (2026-09-14)

## Success Criteria
- `cargo test -p hypha-llm-gateway --target x86_64-unknown-linux-gnu` passes (existing 7 + new 2).
- `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test hypha-local-ai -- --nocapture`
  passes on the canonical image: `/bin/hypha` → typed turns → `[gw] local AI backend:` line →
  `hypha> ` reply lines → clean `exit`.
- `hypha-boot` and `hypha-p3-boot` pass again (they now do: 8.10 s and 8.08 s), which is the
  regression guard for the launch edge this phase had to fix.

## Risk Assessment
- **Undo**: revert the phase commit — the local path is additive inside `complete()`; the network
  path and every banner are unchanged, so removing the call site restores previous behavior.
- **Cannot undo**: once an operator has seen a turn answered locally, the claim "the gateway
  requires a network LLM" is no longer true; the docs/roadmap wording must move with the code.
- **Fixture quality**: the canonical image's model produces non-language text. The gate asserts
  plumbing and says so; it makes no quality claim.
- **Recorded follow-up (not fixed here)**: the block table never gets probed on RV64
  (`EarlyLoader::probe()` runs before any block driver exists and is not retried), so every
  P2-only capability-bearing cell depends on being staged into VIFS1. Making the kernel loader
  probe lazily would remove the staging requirement for all of them — a kernel-loader decision
  with its own evidence, deliberately left out of this slice.

## Security Considerations
- The local backend adds no capability: the gateway keeps `network = false`, `spawn = false`, and
  its syscall allowlist. The AI service Cell holds the model; the gateway only asks.
- A prompt that exceeds the AI wire budget is *not* silently truncated for local submission — it
  goes to the network backend, so a caller can never be shown a local answer to a different
  question than the one it asked.

## Next Steps
- P7 (NPU/GPU backends) remains behind the G3 accelerator envelope; a local *real* checkpoint in
  the canonical image is a deployment decision, not a code change (`scripts/fetch-ai-test-model.sh`).
