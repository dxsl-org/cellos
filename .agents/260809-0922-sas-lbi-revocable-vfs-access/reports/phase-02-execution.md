# Phase 02 Execution Evidence

## Result

Phase 02 is complete. Shell is the only migrated caller. No edit landed under
`libs/api`, `libs/types`, syscall numbers, VFS wire formats, manifests, Lua,
WASM, Hypha, net-broker, or either HTTPD implementation.

## Delivered semantics

- Shell resolves VFS, performs sender-masked typed `Stat`, allocates a
  caller-owned grant bounded by the observed size, shares it only with VFS,
  and performs sender-masked `ReadFileGrant` with the same maximum.
- A short, oversized, malformed, denied, missing, or transport-failed read is
  an error. Exact `GrantDone` equality is required before copying into the
  caller buffer; RAII frees the grant on every exit.
- No migrated shell path calls `GetFile`, accepts `DataPtr`, uses fast IPC, or
  retries a legacy transport. Existing shell callers use typed reads, bounded
  owned reads, or `Stat` for `test -f`; the zero-sentinel adapter was removed.
- QEMU proves a complete 700-byte read, too-small destination rejection,
  missing-file error, and the former 8-byte `test -f` regression.
- `VfsClient` remains unmigrated: it rejects legitimate `DataPtr` replies and
  characterizes current error mapping, while its wildcard-reply spoof risk is
  explicitly deferred to Phase 05 rather than claimed safe.

## Final verification

- `cargo fmt --all --check`: pass.
- `cargo test -p types -p api --target x86_64-unknown-linux-gnu`: pass.
- RV64 checks for `app-shell`, `service-vfs`, and `ostd`: pass.
- `bash scripts/build-shell-test-ci.sh`: pass; RV64 QEMU `shell-utils`: pass
  with all Phase 02 markers.
- `bash scripts/build-test-hooks-ci.sh`: pass; sequential RV64 QEMU
  `riscv64_vfs_quota_all_pass`: pass 1/1, including clamp/nonzero/post-seal
  denial markers.
- RV64 QEMU `vfs_lifetime_selftest_passes`: pass 1/1.
- Production kernel builds for RV64, AArch64, and x86_64: pass.
- `git diff --check`: pass. Production and focused security reviews: PASS.

Coverage remains unavailable on the bare-metal lane because the existing
instrumented build fails on duplicate `core` lang items and missing
`profiler_builtins`. `ostd` host unit tests are likewise unavailable because
its unconditional no-std panic/allocation handlers conflict with `std`.

One parallel verification attempt observed a stale/shared test-hooks artifact
and failed `vfs-quota`; rebuilding test-hooks and rerunning the QEMU lane
sequentially passed. No runtime or hardware evidence is claimed for Pi 3 or
physical RV64. The stale shell `unsafe` allowlist entry remains a low-risk
hardening follow-up because removing it also requires resolving the crate's
`#[no_mangle]` unsafe-code lint contract; it does not authorize new unsafe code.

## Rollback

Revert the Phase 02 shell adapter/caller/test files and the `VfsClient`
characterization as one slice. Phase 03 stays intact because its lifecycle
bridge is independently complete. No ABI, wire, manifest, or persistent state
migration needs rollback.
