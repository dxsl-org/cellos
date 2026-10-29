# Phase02 Build-entry baseline

Captured after exact [CP1 approval](../phase-02-cp1-approval.md), before implementation tasks were dispatched. Source reference HEAD `fabd6ab7e3f2d9ea90a931fe53b7d43e509888dd` plus [dirty-byte snapshot](../phase-02-source-snapshot.json), not a clean-worktree claim. All build output is private at `/tmp/cellbox-phase02-zRWExI6S/target`.

Toolchain: rustc1.97.0-nightly, commit `f53b654a8882fd5fc036c4ca7a4ff41ce32497a6`,2026-04-30, host x86_64-unknown-linux-gnu, LLVM22.1.4; repository channel nightly-2026-05-01. No dependency acquisition.

## Full workspace host attempt — pre-existing runner incompatibility

```sh
env CARGO_TARGET_DIR=/tmp/cellbox-phase02-zRWExI6S/target cargo test --offline --locked --workspace --exclude cellos-kernel --target x86_64-unknown-linux-gnu
```

Exit101 before test execution. `app-posix-shim-test` build script panicked at `cells/tests/posix-shim-test/build.rs:18`: external C smoke has no toolchain for Linux host; supports only riscv64gc-unknown-none-elf/aarch64-unknown-none-softfloat. Also warned that x86 mlibc libc.a was missing. No individual failing test names exist because build did not reach tests. Kernel excluded to avoid embedding quarantined canonical ramdisk as part of this host attempt. Raw full output `artifact://154` was read. Do not describe the full workspace baseline as green or silently exclude this failure from comparison.

## Applicable library suites

```sh
env CARGO_TARGET_DIR=/tmp/cellbox-phase02-zRWExI6S/target cargo test --offline --locked -p api -p ostd -p terminal-sdk -p text-engine --target x86_64-unknown-linux-gnu
```

Exit0: **243 passed,0 failed,6 ignored**,10 suites including empty doctest suites. Detail: API109 unit+2contract; ostd41unit+7endpoint+19doctests; terminal-sdk14unit; text-engine51unit. Existing6ignored doctests remain ignored; not new skips or runtime proof. Full raw output `artifact://156` lines1..325 was read. Passing legacy terminal model tests do not establish the new owner/session semantics; tests that merely pin obsolete models will be removed on clean cutover rather than repinned.

## Actual-module CSpace harness

Initial direct rustc attempt failed to find API's serde_derive dependency; adding the host proc-macro dependency search path resolved harness configuration. Final command:

```sh
rustc --edition=2021 --test tests/unit_cspace_smoke.rs --extern api=/tmp/cellbox-phase02-zRWExI6S/target/x86_64-unknown-linux-gnu/debug/deps/libapi-afaaa6dcd9fdc027.rlib --extern types=/tmp/cellbox-phase02-zRWExI6S/target/x86_64-unknown-linux-gnu/debug/deps/libtypes-f4a3ddaf1e36daf4.rlib -L dependency=/tmp/cellbox-phase02-zRWExI6S/target/x86_64-unknown-linux-gnu/debug/deps -L dependency=/tmp/cellbox-phase02-zRWExI6S/target/debug/deps -o /tmp/cellbox-phase02-zRWExI6S/baseline-cspace
/tmp/cellbox-phase02-zRWExI6S/baseline-cspace --test-threads=1
```

Exit0:65passed,0failed. This harness includes real cspace methods **and detached models**; do not relabel its aggregate as scheduler/pipe/publication runtime evidence. No test source was changed to get this result.

## Bare-metal affected consumer check

```sh
env CARGO_TARGET_DIR=/tmp/cellbox-phase02-zRWExI6S/target CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS=-Crelocation-model=pic cargo check --offline --locked --release --target x86_64-unknown-none -p app-shell -p service-input -p service-vfs
```

Exit0, no warnings in output. Full raw output `artifact://161` read. This is a compile check only, not an ELF/image/signature or actual boot/session observation. Kernel build baseline is not claimed: fresh admitted private embedded inputs are prepared by the fixture owner rather than embedding quarantined canonical bytes.

## Regression comparison contract

Post-integration repeat applicable library suites and bare-metal consumer checks with exact profiles; compare all new failing test names/compile failures against the baseline. Preserve the full-workspace incompatibility,6ignored doctests and harness limitations explicitly. Actual private signed-x86 child/descendant/two-session scenarios remain mandatory for CBX-03..05. No runtime criterion is closed by this baseline.
