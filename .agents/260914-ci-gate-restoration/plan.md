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
four integration jobs as separately-reported defects.

## Phases

- [phase-01-lint-jobs.md](phase-01-lint-jobs.md) — the lint debt, what was fixed, what was found
  that should not have been fixed mechanically, and what remains red elsewhere.

## Non-goals

- No behaviour change in other lanes' code: every fix is a lint's own suggestion or an equivalent
  rewrite, verified by the exact command the CI job runs.
- No governed record is re-bound. `scripts/validate-app-tier-acceptance.py` fails on a *spec
  amendment that was never re-bound to its ledger*; re-binding asserts the amendment was reviewed,
  which is the owning lane's decision, not a lint.
- No work on the four integration jobs here: their failures are test-assertion and boot-window
  defects in other lanes, reported with their exact log lines instead.
