# R4 — shared-file patch (H-1 launch rows)

Written 2026-11-05 by the R4 slice (`feat/r4-hosted-cxx`). R6 freed the H-5 files on 2026-11-05, and R4 committed the guide, ledger and mlibc patch itself. Only the H-1 launch rows remain. R4 may not edit `kernel/**`.

## Needed rows: `/bin/cpp-hosted-smoke`

These mirror `/bin/mlibc-posix-x86` (runtime plan slice RE):
- capability-free;
- only the pre-annex X/M/W rights;
- launched from the shell by `scripts/x86/qemu-cpp-hosted-smoke.sh`.

The manifest declares `block_io = false`, `network = false`, `spawn = false`, untrusted tier (`cells/tests/cpp-hosted-smoke/src/main.rs`). Its syscalls are the R3 witness set: `Write`, `Log`, `Exit`, `GetTime`, `Yield`, `Spawn`, `FutexWait`, `FutexWake`, `Open`, `Close`, `GetRandom`.

```diff
--- a/kernel/src/loader/launch_profile/targets.rs
+++ b/kernel/src/loader/launch_profile/targets.rs
@@
         // x86 Tier-2 runtime witnesses (owner H-1), launched from the shell by
-        // scripts/x86/qemu-mlibc-witness.sh.
+        // scripts/x86/qemu-mlibc-witness.sh and scripts/x86/qemu-cpp-hosted-smoke.sh.
         | "/bin/mlibc-smoke"
         | "/bin/mlibc-posix-x86"
+        | "/bin/cpp-hosted-smoke"
         | "/bin/desktop"
--- a/kernel/src/loader/boot_ceiling.rs
+++ b/kernel/src/loader/boot_ceiling.rs
@@
         // x86 Tier-2 runtime witnesses (owner H-1): mlibc start-up, threads and
-        // timed waits. Capability-free; only the pre-annex X/M/W rights.
+        // timed waits, and the cpp-hosted C++ runtime. Capability-free; only the
+        // pre-annex X/M/W rights.
         | "/bin/mlibc-smoke"
-        | "/bin/mlibc-posix-x86" => LEGACY,
+        | "/bin/mlibc-posix-x86"
+        | "/bin/cpp-hosted-smoke" => LEGACY,
```

The `launch_profile` census tests pin only pre-annex rows (slice RE deviation 5). Re-check them when the rows land.

## After the rows land

```sh
bash scripts/build-mlibc.sh
bash scripts/build-llvm-runtimes.sh x86_64
bash scripts/build-x86_64-cpp-hosted-ci.sh
X86_ACCEL=tcg bash scripts/x86/qemu-cpp-hosted-smoke.sh
X86_ACCEL=kvm bash scripts/x86/qemu-cpp-hosted-smoke.sh
```

These close the two remaining phase-04 success criteria:
- 11/11 markers under TCG and KVM;
- an uncaught exception that ends only the Cell with the libc++abi line, and no fault.
