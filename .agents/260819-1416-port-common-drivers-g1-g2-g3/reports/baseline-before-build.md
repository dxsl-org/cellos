# Baseline Before Build

- Worktree: `/home/dmin/cellos-worktrees/common-drivers-g1-g2-g3`
- Branch/base: `feat/common-drivers-g1-g2-g3` at `a7e8d512`
- Scope: read-only; clean before and after
- Result after exit-code verification: 6 passed, 0 failed, 0 skipped

## Passed

- `cargo fmt --all --check`
- `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu`
- `cargo test -p hal-soc-arm-virt -p hal-soc-bcm27xx -p hal-soc-riscv -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
- `cargo test -p types -p api --target x86_64-unknown-linux-gnu`
- `bash scripts/check-hal-boundaries.sh`
- `bash scripts/check-board-configs.sh`

## Classifier Correction

- The first test report classified raw compiler stderr from the intentional
  `board-vf2,board-pioneer` negative test as a suite failure.
- A clean full rerun returned exit status `0`; `expect_compile_error` requires
  this exact compiler failure and records a failure only if compilation succeeds
  or the expected message is absent (`scripts/check-board-configs.sh:43-62,150-163`).
- No source or script defect exists at baseline. Later phases must keep all six
  suites passing by process exit status.
