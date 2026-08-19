# Open Risk Register

**Last updated**: 2026-08-19

This register tracks confirmed production-readiness gaps found while syncing
docs to code. It is not a bug-fix plan.

## Critical

- Net socket caps are still keyed only by predictable `CapId` values in
  `cells/services/net/src/socket_table.rs:20,51`, and the handlers at
  `cells/services/net/src/handlers.rs:157,177,202` do not owner-bind resolution
  to the caller. Another network-capable Cell can therefore guess or reuse a
  live cap and operate on a peer's socket.

## High

- TLS raw fallback still trims at the last non-zero byte in
  `cells/services/net/src/handlers.rs:453-459` before the send path at
  `:608-613`, which can truncate valid binary payloads ending in zero bytes.
  The raw path needs an explicit length contract.
- Lua and WASM host cells do not declare the `StateRestore` / `LookupService`
  syscall pair required by their argv/VFS paths
  (`cells/runtimes/lua/src/main.rs:11`, `cells/tools/wasm/src/main.rs:16-20`).
  Those paths can fail under the enforced syscall allowlist.
- Production cell admission is not signed-only by default. `kernel/src/signing.rs`
  uses the reproducible dev public key under `dev-signing-key`; without that
  feature the key is a zero placeholder (`kernel/src/signing.rs:33`), while
  `signing-required` remains opt-in (`kernel/Cargo.toml:69`). Release builds
  need explicit key provisioning and mandatory signing/policy features.

## Medium

- Net-broker is still partial wiring. `cells/services/net-broker/src/main.rs`
  marks K1 PSK loading, LAN beacon sockets, relay dispatch, lease renewal, and
  enrollment handling as TODOs; docs should not claim completed swarm routing.
- Net service polling still uses a 100 ms cadence in `cells/services/net/src/main.rs`,
  which is acceptable as a stopgap but remains latency debt for more interactive
  workloads.
- The HTTPS smoke path soft-skips when its TLS mock is unavailable
  (`tests/integration/tests/http-smoke.rs:170-175,211-219`), so CI can degrade
  to HTTP-only evidence unless that prerequisite becomes a hard CI gate.
- Native POSIX path handling remains incomplete: canonicalization, `chdir`,
  `rename`, `getcwd`, and `fstat` contain stubs or deferred behavior in
  `kernel/src/task.rs`; Tier 1 must not be documented as POSIX-complete.
- The hypervisor CI lane is documented as intermittent in
  `.github/workflows/ci.yml:512`; dependency installation has also timed out
  before tests on hosted runners. Treat those outcomes as infrastructure
  blockers, not runtime passes or functional failures.
- Several physical hardware lanes remain hardware-gated. Compile/QEMU evidence
  is useful regression evidence but must not be used as VF2/Pioneer/RPi4 physical
  qualification.
- AArch64 test-hooks runtime proof remains blocked where the host-side
  `qemu_exit::AArch64Semihosting` issue is still present.

## Low

- POSIX and Lua libc stubs are intentionally fail-closed or unsupported, but
  docs must avoid implying full POSIX compatibility for Tier 1 native cells.
