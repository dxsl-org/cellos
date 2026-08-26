---
phase: 6
title: "Verification and docs"
status: complete
effort: "2-3 days"
---

# Phase 6 — Verification and Docs

## Context Links

- Plan: [plan.md](plan.md)
- Guest harness: `cells/tools/shell/src/shell_test.rs`
- Host gate: `tests/integration/tests/shell-utils.rs`
- Living docs: `docs/{project-roadmap,project-changelog,system-architecture,project-overview-pdr}.md`

## Overview

- **Priority:** P1
- Prove the suite through host tests, QEMU shell scenarios, ABI compatibility and target builds
  before updating delivery status.

## Key Insights

- Current coverage exercises only basic grep and sed substitution.
- Batch `top` is the deterministic CI oracle; interactive `q` is a separate smoke test.
- Compile success is code-complete evidence, not runtime verification.

## Requirements

- Black-box cases cover quoting, pipelines, redirects, statuses, errors and every promised option.
- `top -b` terminates at `-n`; interactive top exits on `q`.
- ABI tests independently pin legacy `GetProcs` and new `GetProcs2`.
- RV64, AArch64 and x86_64 builds pass; CI prerequisites fail loudly rather than silently skipping.

## Architecture

Pure parser/matcher/runtime/render helpers receive host tests. One guest boot runs real VFS,
pipeline, status and batch-top scenarios and emits per-feature plus aggregate serial tokens.

## Related Code Files

- **Modify:** `cells/tools/shell/src/shell_test.rs`
- **Modify:** `tests/integration/tests/shell-utils.rs`, `tests/integration/Cargo.toml`
- **Modify:** ABI/kernel tests, living docs, and this plan during sync-back

## Implementation Steps

1. Add host logic, ABI and scheduler accounting tests.
2. Add guest grep/sed/mini-AWK and batch/interactive-top scenarios.
3. Run three-architecture builds and the dedicated QEMU shell lane.
4. Run adversarial code review, fix findings and repeat affected tests.
5. Synchronize all phase/plan and living-doc statuses with exact evidence.

## Todo List

- [x] host logic and ABI tests pass
- [x] guest shell scenarios pass
- [x] three target builds pass
- [ ] adversarial review passes — not run; the session that closed this phase was
      instructed not to spawn review subagents
- [x] plan and docs synchronized

## Success Criteria

- [x] Every `plan.md` acceptance gate maps to a named passing test.
- [x] No regressions in parser, pipelines, redirects, legacy built-ins, `GetProcs`, or `ps`.
- [x] Runtime commands/log tokens are recorded before marking verified.

## Verification record (2026-07-28)

Host logic — the pure stages were extracted to `libs/text-engine` first: as
written they lived inside `app-shell`, whose `ostd` dependency owns
`#[panic_handler]`/`#[global_allocator]`, so `cargo test -p app-shell` died on
E0152 and **none of the 42 `#[cfg(test)]` cases had ever compiled or run**. Same
rationale as `libs/http-core`.

| Gate | Command | Result |
|------|---------|--------|
| Host logic | `cargo test -p text-engine --target x86_64-pc-windows-msvc` | 38 passed, 0 failed |
| ABI pins | `cargo test -p api --target x86_64-pc-windows-msvc` | 12 passed (incl. `process_info_layouts_are_stable`, `GetProcs2` bit 55) |
| Telemetry opt-in | `const _` assertion in `libs/ostd/src/runtime.rs` | compile-time: `app_syscall_set(true,true,true)` never sets bit 55 |
| Guest scenarios | `scripts/build-shell-test-ci.sh` + QEMU rv64 | `[shell-test] Results: 33 PASS, 0 FAIL` |
| Guest lane | `cargo test --test shell-utils` (CI=1) | 1 passed |
| Boot smoke | `scripts/qemu-boot-test.sh` (rv64) | PASS — FAT16 mounted |
| Lint | `cargo clippy --workspace … --target riscv64gc-unknown-none-elf -- -D warnings` | clean |
| Lint | `cargo clippy -p vicell-kernel --target {aarch64,x86_64} -- -D warnings` | clean |
| Builds | rv64 kernel + cells; aarch64 shell; x86_64 shell | all link |
| Format | `cargo fmt --all --check` | clean |
| Ratchet | `scripts/check-cells-unsafe-ratchet.py` | 326 files, 49/49 allowlisted |

## Risk Assessment

- Consolidate guest cases into one boot to bound CI duration.
- Parser blast radius requires retaining all existing parser/executor regression tests.

## Security Considerations

- Negative tests prove pattern, AST, record, file and telemetry-row limits.
- Bit 55 remains required for rich process telemetry.

## Next Steps

Finalize status/docs, then offer a focused commit while preserving unrelated `docs/TODO.md`.
