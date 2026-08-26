# Scout Report — Part 6 Blocking Decision Closure

## Verdict

Plan scope verified. D1b, D3, and D5 still have live contradictions; D1 has implemented constraints and stale performance prose that must be reconciled. No product docs/code were edited during planning.

## Evidence

- IPC bench gate uses p99 and currently checks IPC against `TARGET_IPC_NS = 50_000`: `cells/tests/bench/src/main.rs:39`, `cells/tests/bench/src/main.rs:41`, `cells/tests/bench/src/main.rs:233`, `cells/tests/bench/src/main.rs:238`.
- Fast-IPC remains constrained by non-PIE/JUMP_SLOT notes and scheduler-derived identity: `kernel/src/fast_ipc.rs:116`, `kernel/src/fast_ipc.rs:120`, `kernel/src/fast_ipc.rs:147`, `kernel/src/fast_ipc.rs:160`.
- Loader scaffold exists but must be treated carefully: `kernel/src/fast_ipc.rs:170`, `kernel/src/loader/reloc.rs:18`.
- Frozen LOC values remain in normative docs: `docs/specs/00-context.md:195`, `docs/specs/15-kernel-boundary.md:323`, `docs/specs/16-rustc-tcb.md:142`, `docs/system-architecture.md:47`.
- PDR already points kernel size toward generated status: `docs/project-overview-pdr.md:57`.
- Bare cell-scale NFR remains in PDR: `docs/project-overview-pdr.md:521`.
- Spec 19 already contains the profile direction but not the final D5 closure: `docs/specs/19-hardware-isolation-layers.md:88`, `docs/specs/19-hardware-isolation-layers.md:105`.

## Top Risks

- Benchmark semantics can accidentally make CI permissive; keep regression gating.
- Fast-IPC scaffold deletion can preempt future PIE/import work; prefer status correction unless build verification proves safe.
- LOC targets can drift immediately; only a generated owner prevents repeated docket churn.
- Cell-scale docs can overclaim unless they tie `1000+` to immutable-frame sharing, demand stacks, and measured per-spawn deltas.
