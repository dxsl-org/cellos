# 2026-11-21 — CI green on self-hosted runners, RV64 FP, N-8, semihosting renewal

## What happened
GitHub minutes ran out, so CI moved to two self-hosted runners on this WSL machine (`CI_SELF_HOSTED=true`), with an apt cache, fewer triggers, `ci-local.py` and a pre-push hook. Fixed real bugs that CI had been hiding: RV64 `__switch` did not save FP state (the Ocel font-size panics) and AArch64 `__trap_exit` ran with interrupts unmasked. Brought dead lanes back (executor park, TLS, futex) and closed residual N-8. `B-AARCH64-SEMHOSTING` was renewed twice (#48 and #49) and now expires 2026-12-21T03:53:07Z. `main` is fully green (30 jobs; KVM is skipped for lack of a runner).

## Decisions
- `release.yml` stays on hosted runners because it holds the signing secrets.
- cargo-deny runs as a pinned binary: container actions break self-hosted runners.
- Executor-park witness moved to the `input,ui` image: a headless shell never parks since Cellbox `5043e2cf59`.
- N-8 fix (owner chose option A): `NetRxProof` also accepts a frame drained *before* the NET_RX completion. Changing the oracle network (B) was rejected.
- TTL renewal = `record_correction` and then `blocker_resolution`, one event per push. The schema only resolves from BLOCKED.

## Lessons
- RV64 lanes must use the kernel from `gen-disk-ci.sh`. A plain `cargo build -p cellos-kernel` embeds the fallback init and never reaches a shell.
- Docs-only pushes do not trigger CI (`paths-ignore`). Use `gh workflow run CI --ref main -f heavy=false` when CI confirmation is needed.
- `~/.local/bin` holds QEMU 11.1.3. Evidence must use `/usr/bin` QEMU 8.2.2 (put `PATH=/usr/bin:$PATH` first).
- Edit the ledger in Python with `app_tier_acceptance.checks.canonical_digest` and `validator.state_digest`. Validate against the HEAD baseline the same way CI does.
- Flaky jobs on self-hosted runners usually come from load: both runners running QEMU at once, plus TCG boot windows.

## Next steps
- Renew `B-AARCH64-SEMHOSTING` around 2026-12-14 (expires 2026-12-21T03:53:07Z).
- Residual N-9: six `bootsuite` tests (in progress).
- RV64 trap entry does not save FP. Safe today, revisit if the kernel starts using FP.
- Pre-existing rustfmt drift in `tests/integration/src/*.rs`.
