# Red-Team Judge Verdict

## Verdict

`MODIFY / BLOCKED`: direction accepted, but implementation remains blocked until lifecycle, caller transport, directory bootstrap, ABI retirement, and verification contracts are explicit.

## Accepted Corrections

1. Require a resource-by-terminal-path matrix or one audited terminal cleanup helper covering kernel caps, VFS handles, pending reads, grants/quarantine, fast IPC, service death/restart, caller death, cancellation, watchdog, and hot-swap.
2. Forbid automatic fallback from a migrated read to `GetFile/DataPtr`; rollback is an explicit source/operator action only.
3. Add service HTTPD and net-tools HTTPD; inventory every VFS read surface and helper, not only raw pointer matches.
4. Select transport, maximum size, and syscall authority per caller; do not silently grant GrantCap.
5. Characterize the current `VfsClient` mismatch before migration.
6. Define directory-handle bootstrap for every production caller before raw reads retire.
7. Add file/pending owner purge, atomic tombstone, drain/error semantics, and non-reuse/epoch behavior.
8. Disable old serving while preserving public discriminants; physical removal needs a later major ABI checkpoint.
9. Pin exact verification commands and distinguish runtime, compile-only, hardware-gated, and deferred evidence.
10. Test the actual directory component validators.

## Modified Corrections

- Async Pinning Registry is conditional: inline synchronous bounded replies do not pin; any grant-backed/cancellable/caller-memory-surviving path must reuse the existing pin-aware reclaim interface and stop if it cannot.
- New functionality owns a dedicated `file_handles.rs`; no unrelated pre-refactor is required.

## Rejected Findings

None; duplicate findings were merged.

## Evidence

- `reports/red-team-security.md`
- `reports/red-team-assumptions.md`
- `reports/red-team-failures.md`
- `reports/red-team-dependencies.md`
- `kernel/src/task/syscall.rs:2055-2067,2151,2281-2315`
- `kernel/src/task.rs:597-608,670`
- `kernel/src/task/scheduler.rs:843,981-984`
- `kernel/src/cell/hotswap.rs:295-306`
