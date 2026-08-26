**VERDICT:** PASS_WITH_RISK — no blocking auth bypass found in the Phase 04 launch-profile diff; one caller-visible ABI contract drift should be corrected before relying on the new policy externally.

[MED]      libs/api/src/abi/syscall.rs:119 — public ABI text still says `SpawnFromElf` "Requires SpawnCap", and `libs/api/src/abi/syscall.rs:571` still says the handler enforces SpawnCap; the current handlers instead authorize exact launch edges at `kernel/src/task/syscall.rs:2519` and `kernel/src/task/syscall.rs:2673`, allowing `/bin/shell` to use `SpawnFromPath`/`SpawnFromElf` without `SpawnCap`. Update the public syscall contract and allowlist comments to describe the new two-gate model: allowlist bit plus kernel launch-profile edge.
[LOW]      kernel/src/loader/launch_profile/profiles.rs:34 — Lua compatibility edge is dead or misleading: `cells/runtimes/lua/src/main.rs:11` does not declare `SpawnFromPath`, and startup strips `os.execute` at `cells/runtimes/lua/src/main.rs:88-89`, while the dormant binding still documents spawn behavior at `cells/runtimes/lua/src/bindings_io.rs:37`. Either remove the Lua launch profile row/comment or re-enable it deliberately with tests.
[POSITIVE] kernel/src/loader/launch_profile/mod.rs:56 — launch authority is keyed by current TCB name plus live cap state, not by caller-supplied target labels, so task-name spoofing through `SpawnFromMem` did not appear reachable in this diff.
[POSITIVE] kernel/src/loader/launch_profile/targets.rs:5 — reviewed user target ceilings are exact path matches; privileged boot paths like `/bin/vfs` are not present in the shell/tool-spawn user table.
[POSITIVE] kernel/src/task/syscall.rs:2647 — `SpawnFromElf` validates grant ownership and bounds `len` before copying ELF bytes into the loader path.
[POSITIVE] kernel/src/loader.rs:307 — `/bin/vfs` still fails closed if the cell-store block-region fold is stripped before caps are applied.

Verification noted from parent context: fmt/F1/policy/build-boot/test-hooks passed; manual QEMU passed launch-profile selftest, init registry, shell `vfs-test`, and snapshot denial after decoder fix. I did not rerun the full QEMU suite in this review turn.

Known residual: existing no_std unit-test harness failure remains separate from this Phase 04 diff and is not counted as a Phase 04 blocker.
