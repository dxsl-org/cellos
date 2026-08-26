---
title: "3-Arch Integration Suite Green + Close Known Reds"
description: "Regenerate stale disk images, build a per-arch truth table, and root-cause/fix the input event-delivery reds and the console char-8 stall — debug/verify track."
status: done (2026-07-13)
priority: P1
effort: 4 phases (~M+L+L+S)
branch: main
tags: [testing, ci, riscv64, aarch64, x86_64, input, console, debug]
created: 2026-07-12
---

# 3-Arch Integration Suite Green + Close Known Reds

Debug / fix / verify track. Phases are **investigation-shaped**: each names its
hypotheses and an explicit **oracle** (a boot/suite observation that decides
pass/fail). No fix is invented up front — P01 establishes ground truth first, and
every later phase must reproduce before it repairs.

## Prime constraint — the harness footgun (applies to EVERY oracle)

`QemuRunner::wait_for` (`tests/integration/src/lib.rs:1105`) is a **whole-buffer
`contains` with no cursor**. Consequences baked into every oracle below:

- `wait_for("ViCell >")` *after* `send_line` is a **NO-OP barrier** — the prompt
  is already in the buffer. It proves nothing about the command that followed.
- Correct barrier: send `"<cmd> && echo WROTE$?"` then `wait_for("WROTE0")`.
  boot.rs already uses this at `:716`.
- A substring can match the command's **own serial echo**, not its output
  (see boot.rs:748, :812, :1246 for prior burns). Assert on output that cannot
  appear in the typed line.
- **Any newly-red test is triaged as a harness race FIRST** (timing-shift from a
  fix exposing a latent NO-OP barrier), and only then as a real regression.
- One-shot markers emitted only *after* an injected stimulus
  (`[robot-dashboard] input event received`) are safe to `wait_for` — verify they
  are not also printed at boot.

## Ground state to VERIFY (do not trust — P01 re-derives)

| Arch | Claimed state (2026-07-11) | Regen command (Windows/pwsh) |
|------|----------------------------|------------------------------|
| riscv64 | **images STALE** — must regen for new init binary | `pwsh ./gen_disk.ps1` |
| aarch64 | boot-to-shell 7/7 green | `pwsh ./scripts/build-aarch64-cells.ps1` → rebuild kernel (pic+bti/pac) → `pwsh ./scripts/format-disk-arm.ps1` |
| x86_64 | 13/13 green (boot 7, nvme 3, nic 2, virtio 1 ignored) | `pwsh ./run-x86.ps1 -NoQemu` (builds `build/vicell-x86.iso`) |

**Stale-image risk (top risk):** running any suite on a pre-regen image gives a
**false result** — you debug code that is not on the disk (gen_disk.ps1:51 warns
of exactly this build-skew). init source changed 2026-07-11 (x86 nvme ordering:
spawns `/bin/nvme` pre-VFS + gated net-hook retry; commit `c83adcc6`); the change
is arch-compatible and `cargo check` is green, but riscv64/aarch64 **disk images
were never regenerated**. P01 gate: no truth-table cell is recorded until its
image was rebuilt in the same session.

## Console suite = `boot.rs` (53 tests)

Confirmed: the "53-test console suite" is `tests/integration/tests/boot.rs`. CI
runs an **allowlist subset** (`ci.yml:560`), not all 53 — the rest live outside
CI and their assertions rot. "C′ stall char-8" is a **historical runtime symptom
label** from the 2026-07-07 IPC wildcard-recv fix, **not** a test-fn name — it may
already be resolved (P03 is conditional on reproduction).

## Phases

| # | Phase | Status | Effort | Blockers | Owns (files) |
|---|-------|--------|--------|----------|--------------|
| P01 | [Regenerate 3-arch images + truth table](phase-01-regen-and-truth-table.md) | done | M | — | disk images, `reports/truth-matrix.md` |
| P02 | [Fix input event-delivery reds](phase-02-input-event-delivery.md) | done — resolved, no code change | L | P01 | `cells/services/input/*`, `kernel/src/task.rs` (input dispatch) |
| P03 | [Root-cause console char-8 stall (conditional)](phase-03-console-char8-stall.md) | done — H0 no-repro, guard test added | L | P01 | `kernel/src/task/drivers/console_drv.rs`, `cells/tools/shell/src/async_utils.rs` |
| P04 | [CI gate + regression tests](phase-04-ci-gate.md) | done — allowlist expanded to full 54-test suite | S | P02, P03 | `.github/workflows/ci.yml`, new `#[test]`s |

## Dependency graph

```
P01 (truth) ──┬──▶ P02 (input reds) ──┐
              └──▶ P03 (char-8)     ──┴──▶ P04 (CI gate)
```

P02 and P03 run in parallel **only if** they do not both edit `kernel/src/task.rs`.
The input pending_msgs fix (`task.rs:1248`) and any console-relay change
(`task.rs` `ipc_post_nonblock` :1016) are near each other — **assign `task.rs`
to P02**; P03 confines kernel edits to `console_drv.rs`. If P03 must touch
`task.rs`, serialize P03 after P02.

## Cross-phase risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Suite run on stale image → false pass/fail | High (pre-P01) | High | P01 gate: regen-in-session precondition; record image mtime in matrix |
| Harness NO-OP barrier read as regression | Med | High | Footgun triage step first in P02/P03; WROTE0 barrier pattern |
| Fix regresses a green arch | Med | High | 3-arch regression check is an explicit oracle in P02/P03/P04 |
| char-8 no longer reproduces → phantom work | Med | Low | P03 first action is reproduce-or-downgrade to regression-test-only |
| QEMU TCG timing (10ms input poll) flakes input tests | Med | Med | Generous `wait_for` windows (already 15s); settle sleeps; re-run 2/2 |
| mtools absent on Windows → aarch64 regen fails | Med | Med | P01 prereq check; fall back to CI Linux path (`format-disk-arm.sh`) |
| test-hooks-gated suites need special kernel | Low | Med | P01 uses `scripts/build-*-ci.sh` equivalents for vfs-quota/redoxfs-srv/shell-utils |

## Rollback

- P01 is read-only w.r.t. source (only regenerates artifacts) — nothing to roll back; discard images.
- P02/P03 are isolated commits per phase; revert the phase commit. Each fix keeps
  the prior behavior reachable (no removed markers) so a revert cannot cascade.
- P04 is CI-config only; revert the workflow edit — no runtime impact.

## Success criteria (measurable)

1. ✅ `reports/truth-matrix.md` lists every suite × arch with pass/fail counts and
   exact red test-fn names, each cell tagged with the image it ran against.
2. ✅ `input_keyboard_e2e` and `input_bare_cell` pass 2/2 consecutive runs on riscv64
   (3/3 counting the full-suite pass).
3. ✅ char-8: proven non-reproducing (H0) via new guard test
   `console_long_line_with_backspace_no_stall`, 2/2 green.
4. ✅ No arch regressed. Two REAL bugs were found and fixed along the way (outside
   this plan's original scope, discovered while investigating): a Hypha/init
   GrantAlloc-syscall-denial regression and an RV32/Cellos-Nano compile break —
   both fixed, reviewed, and verified across all 5 kernel targets with no
   cross-arch regressions.
5. ✅ CI allowlist expanded from 22 to the full 54-test `boot.rs` suite (user chose
   the broader option); verified green locally 3× (not yet verified on GitHub
   Actions — deferred, user chose to commit locally without a branch push).

## Closed 2026-07-13

All 4 phases done. See `reports/truth-matrix.md` for the full investigation and
`docs/project-changelog.md` [2026-07-13] for the two bugs found/fixed outside the
original scope (Hypha Grant-syscall denial, RV32 compile break).

## Open questions

- Are `input_keyboard_e2e`/`input_bare_cell` in the *current* red set, or already
  green on fresh images? (P01 answers.) The 2026-07-07 CI comment cites "3 input_*
  tests needing live-input markers" — two of those (`input_service_registered_at_boot`,
  `compositor_input_routing_active`) are already in the green allowlist, so the
  live reds are the two injection tests.
- Does `boot()` route VirtIO-keyboard events at all under `-display none` in the
  installed QEMU version? (P02 H2.)
- Should aarch64/x86 suites be added to CI (currently local-only), or is expanding
  the riscv64 allowlist the 80/20? (P04 decides; default: expand riscv64, document x86 local.)
