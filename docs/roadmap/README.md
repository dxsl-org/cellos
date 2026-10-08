# Roadmap Folder

The sole active program is **Cell-to-Cell Anywhere on Intel x86-64**, on one fixed headless hardware configuration. [ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md) is the canonical direction; [current focus](current-focus.md) selects work within it. Every task must name a direct C2C-on-Intel deliverable, dependency, or necessary regression.

The target spans Tier 1 native Cells, Tier 2 C/C++ domains, and Tier 3 VM participants through explicit adapters, across local/LAN/gated-relay paths. It is not transparent distribution of arbitrary applications. Intel VMX remains incomplete, x86 Tier 2 admission is test-only with a C++ shim gap, and physical x86 qualification is still open. Existing ABI, security, purchase, remote-activation, and production gates are unchanged.

GUI, browser, AI, robotics, general OS expansion, and new AMD/ARM/RISC-V platform work are paused as independent programs. Preserve historical code/evidence and necessary cross-architecture regressions; no retained roadmap is an autonomous work queue.

Roadmap content is split by lookup intent:

- [current-focus.md](current-focus.md): active status, immediate gates, and what
  should drive the next implementation slice.
- [hardware-tracks.md](hardware-tracks.md): Intel qualification dependencies and retained/paused board and SoC lanes.
- [product-stages.md](product-stages.md): historical G1-G5 overlay, not independent implementation schedules.
- [runtime-and-platform-tracks.md](runtime-and-platform-tracks.md): C2C dependencies and retained/paused runtime and platform overlays.
- [technical-milestones.md](technical-milestones.md): phase/milestone snapshot
  without per-commit status logs.
- [completed-history.md](completed-history.md): condensed completion ledger for shipped work.
- [open-risk-register.md](open-risk-register.md): confirmed open code or
  production-readiness risks.
- [project-roadmap-legacy.md](../project-roadmap-legacy.md): read-only pre-split
  content snapshot kept for traceability; whitespace was normalized.

Do not put personal task tracking here. Per-user TODO tracking belongs under
`.agents/`.
