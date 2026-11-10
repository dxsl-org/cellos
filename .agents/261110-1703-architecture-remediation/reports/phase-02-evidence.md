# Phase 02 evidence — x86 C2C gate, lane manifest, reproducible CI

**Date:** 2026-11-10 · **Base:** `aef3c5b86` + working tree · **Host:** WSL2 x86_64, QEMU 11.1.3, `/dev/kvm` present (TCG used for the gate) · **Ceiling:** host + x86 QEMU TCG. The new CI job itself has not run on GitHub yet (no push).

## Local lane timings (warm cargo cache, `/tmp/s1-lanes/run.sh`, throwaway)

| Step | Result | Seconds |
|---|---|---|
| assemble headless image (isolated out-dir) | rc 0 | 53 |
| kernel build `--features policy-required` | rc 0 | 35 |
| ISO | rc 0 | <1 |
| two-node C2C, TCG, `X86_CPU_MODEL=max` | PASS (peer link-local announce/request/reply logged) | 20 |
| two-node C2C, `C2C_NEGATIVE=1` | FAIL as designed ("no node logged an ARP reply from its peer") | 20 |
| x86 domain test build | rc 0 | 58 |
| x86 domain test, TCG | rc 0 | 120 |
| lifecycle image build | rc 0 | 56 |
| `local-service-lifecycle-x86` | rc 0 | 38 |

Total ≈ 6.7 min, under the 15-minute threshold → **pr tier** (owner decision `decide-by-timing`).

## Guards

| Check | Result |
|---|---|
| `python3 scripts/check-lanes.py` | OK (71 lane scripts, 72 integration targets) |
| negative: untracked-then-intent-added `scripts/qemu-foo.sh` | `FAIL lane scripts/qemu-foo.sh: missing …`, exit 1 |
| `python3 scripts/check-host-test-crates.py` | OK (49 crates run by ci.yml, 50 listed `untested` with reason) |
| negative: drop the `types ci` row | `FAIL types: contains #[test] but is missing …`, exit 1 |
| `cargo metadata --locked` (root and `tests/integration`) | OK |
| x86_64 workspace clippy, `-D warnings`, 8 excludes, CI env | rc 0 (432 crates checked) |
| all six workflow files parse (PyYAML) | OK |
| `check-cargo-config-parity.py`, `check-plan-status.py`, `check-spec-anchors.py --check` | OK (spec status regenerated after the Spec 10 edit) |

## Inventory recorded in `scripts/lanes.toml`

- Lane scripts: 71 — 20 `pr`, 1 `nightly`, 50 `manual`.
- Integration targets: 72 — 25 `pr`, 47 `manual`.
- Every `manual` row added today reads "not named by any workflow as of 2026-11-10; current pass/fail unknown". None was run to classify it.

## Workflow changes

- `ci.yml`: workflow `concurrency` (cancel superseded runs); job-level `timeout-minutes` on the 17 jobs that had none (90, or 30 for `board-configs`/`security`/`attest-catalog`); new guard steps (lanes, host-test crates, lockfiles); new `x86-c2c` job; x86 clippy widened to the workspace.
- All workflows: every `uses:` pinned to a 40-hex commit SHA (resolved with `git ls-remote` on 2026-11-10; tag kept as a comment); `pip install cryptography` → `-r requirements-ci.txt` (`cryptography==41.0.7`, the version the local signing pipeline used for the runs above).
