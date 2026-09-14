# Phase 02 — The RedoxFS /srv job: one stale test, one real kernel fault

**Status**: stale test fixed; the kernel fault is reproduced and handed off
**Ceiling**: qemu (riscv64), local reproduction

## What the job has been failing on

Two tests in `tests/integration/tests/redoxfs-srv.rs`, for two unrelated reasons.

### 1. `riscv64_redoxfs_srv_degrade_no_disk` — stale expectation (fixed)

The test boots a kernel with no block device and waits for
`[vfs] WARNING: RedoxFS P5 open failed`. That string is gone from `main`: the VFS backend is
CellosFS (`cells/services/vfs/src/backend_cellosfs.rs`); `backend_redoxfs.rs` only still exists in
stale worktrees. The current message, confirmed by running the test, is

```
[vfs] WARNING: CellosFS mount/format failed — volume unavailable
```

The expectation now asserts that substring. The test's intent — the VFS degrades to "no volume"
rather than failing the boot — is unchanged.

### 2. `riscv64_redoxfs_srv_basic` — a kernel fault, not a stale marker (handed off)

Reproduced locally with:

```
tests/integration $ cargo test --test redoxfs-srv srv_basic
```

after `scripts/build-test-hooks-ci.sh` and `scripts/build-srv-test-ci.sh`. The guest side is healthy:
`srv-test` prints `Results: 6 PASS, 0 FAIL` and `ALL TESTS PASSED`. Then the harness types
`posix-shim-test` at the shell, the cell prints

```
[posix-shim] POSIX-FSTAT-OPEN: OK
[posix-shim] POSIX-FSTAT: OK
```

and the kernel halts:

```
[KERNEL PANIC] Critical failure.
panicked at hal/arch/riscv/src/rv64/trap.rs:184:21:
Cellos: Kernel exception: scause=13 sepc=0x8023d708 stval=0x10000005 sstatus=0x8000000200006100
```

Symbolized against the kernel that produced it (`target/riscv64gc-unknown-none-elf/release/cellos-kernel-srv-test`):

- `sepc=0x8023d708` is inside `cellos_kernel::task::drivers::console_drv::viConsole::poll` (+0x210).
- The instruction there is `lbu a0, 5(a1)` with `a1 = 0x10000000`: a **load of the 8250 UART line-status
  register** (`0x10000000 + 5`) on the riscv-virtio machine.
- `scause=13` is a load page fault, so the kernel executed that MMIO read while the active `satp`
  did not map the UART identity region.

Two observations that scope it:

- **It is intermittent.** The first local run got past `POSIX-MKDIR-RMDIR: OK` and then timed out
  waiting for `POSIX-RENAME: OK`; the second never saw the mkdir marker at all. A deterministic
  pointer bug does not behave that way; an ordering/state-dependent fault does.
- It is the `posix-shim-test` cell's `test_mkdir_rmdir` path — the first thing that test does with
  the new directory-lifecycle syscalls is pass deliberately hostile arguments (`NULL`, invalid UTF-8,
  an empty directory removed twice). The shell-launched-spawn + interrupt + MMIO sequence is
  therefore happening around those calls, and the fault surfaces on the next timer-driven console
  poll.

Not this lane: the fault is in the kernel's console/trap/domain path (or in how a hostile pointer
argument reaches it from the new `mkdir`/`rmdir` syscalls), and the owning lane needs the reproduction
above, not a guess. What this phase rules out is the comfortable explanation — "the test is just
waiting for an old RedoxFS string" — which is true of the *other* test in the same job and would have
hidden a halting kernel behind a test-hygiene fix.
