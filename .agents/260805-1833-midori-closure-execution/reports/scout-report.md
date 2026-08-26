# Scout Report — Midori Closure Execution

## Repo state

- Observed branch: `main`.
- Observed status: `main...origin/main [ahead 1]`, modified `docs/project-changelog.md`, `docs/specs/18-cell-trust-tiers.md`, `docs/specs/19-hardware-isolation-layers.md`, untracked `docs/specs/18b-cell-admission-consent-adr.md`.
- Local-only commit: `3776b1ac docs(project): sync Midori closure status`.
- `b5a97125` and `eecfbb72` are not ancestors of `main`; `git branch --contains` maps them to `codex/midori-phase01-evidence` and `codex/midori-vfs-region-fold`.

## Load-bearing evidence

- Portfolio WIP limit: `.agents/plan-portfolio.md:8-12` says Midori is sole active feature program and exits after runtime-close 02 plus completion of 04/07/08.
- Phase status: `.agents/260727-2101-midori-lessons-cellos/plan.md:32` says Phase 02 is code-done but runtime-unverified with `/srv/` and `/` gaps; `:34` says Phase 04 is partial; `:37` says Phase 07 lacks reactor/CQ/executor; `:38` says Phase 08 sizing is blocked on 07.
- Dependency order: `.agents/260727-2101-midori-lessons-cellos/plan.md:46-48` and `:121-123` require 02 before 06 and 07 before 08 because pre-07 watermarking is invalid.
- Law 1: `docs/code-standards.md:12-18` requires two explicit confirmations for `libs/api/` and `libs/types/`.
- Done gate: `docs/code-standards.md:270-291` requires clean build, QEMU evidence, CI wiring, fail-loud behavior, and status text in same commit.
- Phase 02 code evidence: `cells/services/vfs/src/handle_table.rs:55-84` has owner-checked `insert_ro` and lookup; `cells/services/vfs/src/pending.rs:57-97` has owner-checked pending reads; `cells/services/vfs/src/main.rs:97-125` fast-IPC `GetFile` is authorized but still only serves `GetFile`.
- Phase 04 code evidence: `kernel/src/loader.rs:274-282` applies `boot_ceiling`; `kernel/src/task/syscall.rs:2269-2276` leaves `LookupService` open; `cells/tools/init/src/main.rs:89-115` currently supervises 9 services and registers known service IDs.
- Phase 07 evidence: `libs/api/src/abi/completion.rs:46-70` defines `ViCompletion`; `kernel/src/main.rs:602-605` runs a completion queue self-test; `cells/tools/shell/src/async_utils.rs:15-42` depends on `TaskState::Recv` delivery.
- Phase 08 evidence: `kernel/src/task.rs:40` keeps global `STACK_PAGES = 64`; `.agents/260727-2101-midori-lessons-cellos/phase-08-stack-sizing-table.md:54-63` requires zeroing from allocated stack and guard/probe before size reduction.

## Tooling gaps

- `docs/coding.md` absent.
- `docs/engineering-standards.md` absent.
- `.claude/scripts/set-active-plan.cjs` absent, so active-plan sync could not run.

## Provenance notes

- Memory-derived Git cleanup guidance was treated as PRIOR and re-verified against current refs before being included.
- Research-agent messages matched current grep, but every plan-critical branch/symbol/status claim above is OBSERVED from current repo commands or files.
