all-pass: 4/4, coverage unavailable
Mode: diff-aware | 9 changed files
Mapped: `cells/tests/bench/src/bench-probe.rs`, `cells/tests/bench/src/main.rs`, `cells/tests/bench/src/scenarios/smp.rs`, `kernel/src/main.rs`, `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/ipc_guardrail_selftest.rs`, `tests/integration/tests/boot.rs`, `tests/integration/tests/hotswap-smoke.rs`
Unmapped: none
Ran 4/4: 4 passed, 0 failed, 0 skipped
Build/typecheck: pass
`cargo fmt --all --check` PASS
`cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` PASS; first non-incremental retry hit a WSL incremental lock-file false-negative, then `CARGO_INCREMENTAL=0` passed
`cargo check -p app-bench -Z build-std=core,alloc && cargo build -p app-bench --release -Z build-std=core,alloc` PASS; warning only: `run_heartbeat_peer` dead code, `VERGEN_GIT_SHA` default, and non-suitable git worktree / strip-tool warnings
`cargo test --test boot --test hotswap-smoke --no-run` PASS
Diff/scope check PASS: `git diff --check && git diff -- libs/api libs/types` clean, with no `libs/api`, `libs/types`, `executor`, or VFS source modifications
Root QEMU evidence retained, not rerun here: `peer_death_guardrail_is_bounded` PASS, `shell_ready_hypha_burst_is_lossless` PASS after final disk rebuild; earlier root evidence also includes `build-test-hooks PASS`, `input_keyboard_e2e PASS`, `input_bare_cell PASS`, and `console_long_line_with_backspace_no_stall PASS`
Coverage note: unavailable in this pass
