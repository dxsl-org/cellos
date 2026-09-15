# Plan — Restore the hosted CI gates

**Created**: 2026-09-14
**Ceiling**: hosted runner (the CI gate itself is the evidence class this lane feeds)
**Track**: cross-lane maintenance, opened because every lane's evidence claim depends on this gate

## Goal

Every push to `main` for at least a day has ended in the same seven red jobs:
`Lint (fmt + clippy)`, `Clippy (x86_64)`, `Clippy (aarch64)`, `C2C Broker Oracle`,
`Network Data-Path Integration (riscv64)`, `RedoxFS /srv Integration Test`, and
`QEMU Hypervisor Boot-to-Shell (x86_64)`. A gate that is always red cannot be consumed as evidence,
and the repository's own roadmap ties hosted-run evidence to a passing pipeline — so this work is a
prerequisite for any lane, including the G2 AI lane, to claim anything above `host`.

This plan takes the three lint jobs (the ones with a single, mechanical root cause) and leaves the
four integration jobs as separately-reported defects — with one exception: the CellosFS `/srv` job's
kernel fault was investigated here (phases 02-04) because it halts the kernel, so no lane can consume
that job as evidence until it is fixed.

## Outcome (2026-09-15)

The lane's own target is met: the three lint jobs are green, and `CellosFS /srv Integration Test` is
green on every push from `bc97e394b` onward. The trap-root fault also accounted for most of two other
integration jobs, which changed but did not close:

| job | state after phase 04 |
|---|---|
| `Lint (fmt + clippy)`, `Clippy (x86_64)`, `Clippy (aarch64)` | green (phase 01; the metrics gate needs `scripts/generate-code-metrics.py` re-run whenever kernel nLOC moves) |
| `CellosFS /srv Integration Test` | **green** on `bc97e394b`, `bc7be77ea`, `c5e92f0f1` |
| `Network Data-Path Integration (riscv64)` | 6 failures (4 of them the UART kernel fault) → 2, zero kernel exceptions; the remainder is the HTTP-server pair |
| `C2C Broker Oracle` | PLIC kernel fault gone (487 hosted / 923 local control → 0); now fails on a cell fault at `0x80cbe000` — C2C lane's own defect |
| `QEMU Hypervisor Boot-to-Shell (x86_64)` | unchanged: `Init: fb-console spawn failed` / shell focus timeout, no panic |

## Phases

- [phase-01-lint-jobs.md](phase-01-lint-jobs.md) — the lint debt, what was fixed, what was found
  that should not have been fixed mechanically, and what remains red elsewhere.
- [phase-02-srv-fault.md](phase-02-srv-fault.md) — the CellosFS `/srv` job: one stale assertion
  fixed, one real kernel fault reproduced and handed to its owning lane.
- [phase-03-console-fault.md](phase-03-console-fault.md) — the `console_drv::poll` fault: the
  measurement is applied and the mechanism is proven (the fault runs with a private Cell root live,
  ASID 1, while the kernel root is ASID 0).
- [phase-04-trap-root-discipline.md](phase-04-trap-root-discipline.md) — the fix, implemented and
  measured: trap entry installs the kernel root (parking the interrupted root in the frame), trap
  exit restores it when it differs, and same-domain resume re-programs its root. 10/10 clean boots on
  the phase-03 driver, 10/10 green `srv-cellosfs` runs, RV64 domain regressions re-derived with a new
  `S22-RV64-RESUME-ROOT` fixture. Hosted confirmation lands with the next push.

## Non-goals

- No behaviour change in other lanes' code: every fix is a lint's own suggestion or an equivalent
  rewrite, verified by the exact command the CI job runs.
- No governed record is re-bound. `scripts/validate-app-tier-acceptance.py` fails on a *spec
  amendment that was never re-bound to its ledger*; re-binding asserts the amendment was reviewed,
  which is the owning lane's decision, not a lint.
- No work on the four integration jobs here: their failures are test-assertion and boot-window
  defects in other lanes, reported with their exact log lines instead.
