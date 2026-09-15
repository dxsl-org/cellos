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

The lane has achieved complete closure across all 23 jobs in the CI pipeline
(GitHub Actions Run `34964009461` on commit `2bb82e50a` — 100% green).

| job | final state (Phase 05) |
|---|---|
| `Lint (fmt + clippy)`, `Clippy (x86_64)`, `Clippy (aarch64)` | **green** (phase 01 + metrics sync) |
| `CellosFS /srv Integration Test` | **green** (phase 04 trap-root discipline) |
| `Network Data-Path Integration (riscv64)` | **green** (phase 05 `service-httpd` CLI args support; 54/54 tests pass) |
| `C2C Broker Oracle` | **green** (phase 05 dev cell signing in `run-c2c-broker-oracle-qemu.sh`) |
| `QEMU Hypervisor Boot-to-Shell (x86_64)` | **green** (phase 05 10 MiB custom heap + `pci=off` + early exit) |
| `QEMU Hypervisor Machinery Smoke (TCG)` | **green** |
| All other 17 jobs | **green** |
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
- [phase-05-full-gate-closure.md](phase-05-full-gate-closure.md) — the remaining three jobs:
  (1) `service-httpd` CLI args support closes the two HTTPD test failures in `Network Data-Path`;
  (2) cell signing in `run-c2c-broker-oracle-qemu.sh` closes the C2C Oracle failure;
  (3) 10 MiB custom heap in `service-hypervisor` closes the x86 hypervisor OOM crash loop.
## Non-goals

- No behaviour change in other lanes' code: every fix is a lint's own suggestion or an equivalent
  rewrite, verified by the exact command the CI job runs.
- No governed record is re-bound. `scripts/validate-app-tier-acceptance.py` fails on a *spec
  amendment that was never re-bound to its ledger*; re-binding asserts the amendment was reviewed,
  which is the owning lane's decision, not a lint.
- No work on the four integration jobs here: their failures are test-assertion and boot-window
  defects in other lanes, reported with their exact log lines instead.
