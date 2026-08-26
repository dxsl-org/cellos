---
title: Harden Hypha JSON Parser
status: completed
priority: P1
created: 2026-08-26
---

# Harden Hypha JSON Parser

## Goal

Replace substring-based JSON extraction in the Hypha LLM gateway with strict
structured parsing and duplicate-key rejection while preserving its no-std and
IPC contracts.

## Phases

| Phase | Name | Status | Depends on |
|---|---|---|---|
| 01 | [Harden parser and add regressions](phase-01-harden-hypha-json-parser.md) | completed | — |

## Locked Scope

- Touch only `cells/apps/hypha/llm-gateway/` plus this plan/evidence.
- Preserve public function signatures and `ToolCall { name, args_json }`.
- Reject malformed, trailing, and duplicate-key JSON at every object depth.
- Decode escaped keys before duplicate comparison.
- Parse response content only from `choices[0].message.content`.

## Verification

- Host parser regressions: 7/7 passed.
- RV64 target compile, Clippy, formatting, and diff checks passed.
- Independent production-readiness review: PASS 10/10, no residual findings.
