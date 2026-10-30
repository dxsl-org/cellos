# NET-GCD CP2 dossier (graceful-close deadline aborts)

Implements `reap-and-rst-cp1.md` (approved as recommended, D-GCD-1..; D-GCD-7 A: T3 lane pin list NOT edited). Host tests + bare-metal builds only; **no QEMU, no network.** No commit made. `CHANGELOG.md` left to the main agent.

## Pre-edit
- Closure hashes equal to CP1 section 6 (service-runtime.rs `89d9334a`, socket_table.rs `1964bc9d`, socket_table/tests.rs `28529be3`, owner-death-tests `3c12780f`, docs/network-api.md `5879be39`). `CHANGELOG.md` differed (`0323764a`, main agent's; not edited). Pinned list (handlers/tcp.rs, tls/dispatch.rs, tls_handler.rs, main.rs, Cargo.toml, Cargo.lock, ipc.rs, syscall.rs, clients/net.rs, ssh.rs, reactor.rs, libs tree `5828504d`) all equal before and after.
- Note: `scripts/qemu-tier3-ssh-guest-x86.sh` / judge hashes already differ from CP1 (guest-pipeline lane commits); unrelated and untouched.
- Baseline: `cargo test -p service-net --target x86_64-unknown-linux-gnu`: 65 passed. Clippy (no target): bin 8 warnings (incl. 2 api/ostd lib lines separately).

## After
- Hashes: service-runtime.rs `1b3c7c63...2a2a` (802 -> 854 lines), socket_table.rs `154e7b77...8603` (-> 317), socket_table/tests.rs `b97a554a...dcd4` (-> 340), service-runtime-owner-death-tests.rs `b7b90b85...b62d` (-> 669), docs/network-api.md `c2f72cc6...1a22` (-> 326).
- Tests: **76 passed**, 3 consecutive runs equal (65 + 10 runtime tests G1-G10 [G1..G10 as CP1 8.1] + 1 table test = 76). Two reaper calls in `a_graceful_close_in_flight_...` retargeted with `now = 0` and now assert the return `0`.
- Clippy host (`--all-targets --target x86_64-unknown-linux-gnu`): bin 6 warnings, bin test 49 (5 duplicates) vs NET-OD recorded 6 / 50; no warning on any added line. (Clippy without `--target` cannot build tests: `can't find crate for test` on the default bare-metal target, same as before the change.)
- Builds: x86_64 `RUSTFLAGS="-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic" cargo build --release --locked --offline -p service-net --target x86_64-unknown-none -Z build-std=core,alloc`: OK. Default riscv64 release and aarch64-unknown-none-softfloat release: OK. Debug bare-metal builds do not link (rust-lld), as already noted at baseline in the NET-OD dossier (not re-proven at baseline here).
- `python3 scripts/cellos-sign --check` rc 0 (F1/F5 OK).
- `git status`: the 5 closure paths modified by this slice; other modified `kernel/src/task*` files belong to other lanes.

## Code summary
`GracefulEnd`/`graceful_end` (pure), `GRACEFUL_REAP_PER_TURN`, `reap_graceful_closes(table, sockets, now) -> usize` (aborts via `abort()` + `graceful_to_abort`), `graceful_deadline_line`, loop call site prints the line; table: `graceful_to_abort`, `begin_graceful_close` ignores an aborting cap. `handlers/tcp.rs` unchanged.

## Deviations
- Test named `fin_wait1_and_last_ack_are_aborted_at_the_deadline` (CP1: `..._closing_and_last_ack...`; CLOSING covered by G1).
- G4 initially closed both ends before a poll (simultaneous close -> both TIME-WAIT); fixed to the sequential close used by the existing test. No other test needed adjusting.
- No QEMU observation of the new behaviour; the T3 lane preflight will refuse until a lane CP1 re-pins the four net files (D-GCD-7 A, as approved).
