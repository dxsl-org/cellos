# Phase 05 Caller Migration Execution

Date: 2026-08-10  
Base commit: `c7c3ca31`  
Caller migration commit: `845ce926`  
Runtime unblock commit: `e3ecc4a2`  
Status: caller migration implemented; Law 1 checkpoint B **not ready**

## Implemented slices

- `ostd::clients::VfsClient` now reads through `OpenRootDir` / `OpenDir` /
  `OpenFileAt` / bounded `ReadFileHandle` / `CloseFile`, with exact sender
  masking and explicit cleanup. It never retries `GetFile`, `DataPtr`, fast
  IPC, async polling, or grant copy-out.
- Hypha tool-fs, net-broker config/key loading, shell reads, Lua reads, the
  WASM loader, service HTTPD, and net-tools HTTPD use bounded handle reads.
- Lua and WASM no longer hard-code VFS tid `3`. Hypha and Lua remain unsealed
  because their write/list/stat/startup operations are still path-addressed.
- Both HTTPDs preflight `Stat` through exact masked IPC. Raw wire `Err(1)` is
  404, existing empty files are 200, all other preflight failures are 500.
  TCP response writes fail-stop on bounded zero progress and surface failure
  before the existing close path.

## Inventory gate

The production exclusions below are clean in the current working tree:

- no `VfsRequest::GetFile`, `VfsResponse::DataPtr`, or direct `get_file_ptr`
  outside the VFS implementation, tests/bench, and reserved ABI fixtures;
- no `VfsRequest::ReadAsync` / single `VfsRequest::Poll` file reader outside
  the VFS implementation and tests;
- no `sys_recv(0)` in either HTTPD;
- no Phase 05 diff under `libs/api/` or `libs/types/`.

Remaining legacy symbols are deliberate: VFS serving/backends, tests/bench,
reserved ABI fixtures, and the existing spawn `ReadFileGrant` path in
`libs/ostd/src/fs.rs`.

## Verification evidence

Passed:

- `cargo fmt --all --check`
- `git diff --check`
- API/types host tests: 78 API, 2 contract, and 10 types tests
- host and RV64 compile for ostd, Hypha tool-fs, net-broker, shell, WASM,
  service HTTPD, and net-tools
- AArch64 and x86_64 bare-metal compile for the same set except Lua
- shell-utils RV64 QEMU lane, including exact-bound, greater-than-480-byte,
  truncation, directory, missing-file, and reuse-after-error cases
- Slice 5 adversarial review after the final HTTP send and Cargo target
  auto-discovery corrections
- fresh `gen_disk.ps1` rebuild, F1/F5 signing, 42 signed cells, and no stale
  optional Lua/Tetris-Lua artifact packaged
- RV64 QEMU `shell_executes_echo`, proving queued UART/input IPC delivery after
  removing the nested `SCHEDULER` relock in `Recv`, `RecvTimeout`, and `TryRecv`
- RV64 QEMU `network_httpd_serves_file` and
  `network_httpd_dynamic_content`, proving net-tools HTTPD bounded VFS reads,
  background spawn through the VFS `/bin` overlay, and repeated read freshness
- net-broker reached its bounded configuration read on RV64 QEMU and returned
  the expected explicit missing-config path (`no peers configured`)

Host-gated or incomplete:

- Lua compile remains blocked by the existing host C/signedness/sysroot
  failures, not by a proven runtime lane.
- `cargo test` for ostd-dependent no-std cells conflicts with the host `std`
  allocation and panic handlers; focused cases compile but do not execute.
- Service HTTPD runtime is still unrecorded; the two passing HTTP lanes exercise
  the net-tools `/bin/httpd` binary.
- QEMU runtime evidence is not yet recorded for Lua, WASM, Hypha tool-fs, or
  net-broker key loading with a present configuration.
- The existing `hypha_p3_tool_cells_spawn` lane is not a valid Phase 05 runtime
  oracle: it asks the capability-free shell to launch `/bin/hypha`; the kernel
  correctly denies the VFS-backed `SpawnFromElf` edge because Hypha carries a
  non-empty spawn ceiling. The raw-path fallback cannot reach the userspace
  cell-store. Do not widen shell authority or the launch profile to make this
  stale test pass; a separately designed init/supervisor-owned test launch is
  required.

## Law 1 checkpoint B disposition

Do not disable message-path `GetFile -> DataPtr` or the fast VFS `GetFile`
arm yet. The existing Law 1 confirmation pair covers reserved-slot
disablement, but the plan also requires every migrated caller to pass its
runtime gate before checkpoint B is recorded. That evidence is incomplete.

No public variants or discriminants may be removed or renumbered. No
`libs/api`, syscall number, wire format, manifest, Tier 2, async DMA, reactor,
or SMP work is authorized by this execution record.

## Rollback

The caller migration and queued-IPC runtime fix are committed separately as
`845ce926` and `e3ecc4a2`. Roll back those commits in reverse order; never add
per-read fallback to `GetFile`, `DataPtr`, fast IPC, async polling, or
`ReadFileGrant`.
