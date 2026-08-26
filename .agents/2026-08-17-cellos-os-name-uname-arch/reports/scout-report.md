# Scout Report: Cellos OS Name + Kernel Artifact Rename

## Phase 1 scope verified

- User-visible shell prompt is `prompt: "ViCell > "` in `cells/tools/shell/src/shell.rs:33`.
- Shell readiness banner is `=== ViCell shell ready...` in `cells/tools/shell/src/shell.rs:51`.
- Shell help title is `ViCell Shell v0.2.1...` in `cells/tools/shell/src/commands.rs:7`.
- Shell built-in `uname` hardcodes both OS name and `riscv64` in `cells/tools/shell/src/cmd_sys.rs:16-22`.
- Shell built-in `env` prints `OS=ViCell` in `cells/tools/shell/src/cmd_sys.rs:39-43`.
- Sys-tools `/bin/uname` hardcodes the same full string in `cells/tools/sys-tools/src/bin/uname.rs:8-11`.
- Sys-tools `/bin/env` prints `OS=ViCell` in `cells/tools/sys-tools/src/bin/env.rs:8-14`.
- Init banner still prints `Init: Starting ViCell Orchestrator...` in `cells/tools/init/src/main.rs:70-76`.
- Kernel boot banner is already `[Cellos]` in `kernel/src/main.rs:233`.

## Phase 1 test/gate impact verified

- `tests/integration/tests/boot.rs:1522` waits for `=== ViCell shell ready`.
- `tests/integration/tests/capacity-observability.rs:38` waits for `=== ViCell shell ready`.
- `tests/integration/tests/launch-profile.rs:12` defines `const PROMPT: &str = "ViCell >";`.
- `scripts/qemu-boot-test.sh:66`, `scripts/qemu-aarch64-test.sh:66`, and `scripts/qemu-x86_64-test.sh:47` gate on `ViCell >`.
- Grep count: `git grep -n -F "ViCell >" -- tests scripts | wc -l` returned `146`; update all gate references in `tests/` and `scripts/`, not historical docs.

## Phase 2 package/artifact scope verified

- Current kernel package name is `vicell-kernel` in `kernel/Cargo.toml:2`.
- Current CI package selectors and artifact paths include `.github/workflows/ci.yml:180`, `.github/workflows/ci.yml:230`, `.github/workflows/ci.yml:238`, `.github/workflows/ci.yml:246`, and `.github/workflows/perf.yml:96`.
- Current AArch64/RPi3 paths/commands include `gen_disk_rpi3.ps1:32`, `gen_disk_rpi3.ps1:38`, `run-rpi3.ps1:47`, `run-rpi3.ps1:53`, and `scripts/build-aarch64-cells.ps1:195-198`.
- Current boot configs load `/vicell-kernel` in `limine.conf:6`, `limine-vf2.conf:6`, and `limine-pioneer.conf:6`.
- Current QEMU gates default to old artifact paths in `scripts/qemu-boot-test.sh:17`, `scripts/qemu-aarch64-test.sh:13`, `scripts/qemu-x86_64-test.sh:22`, and `scripts/x86/make-iso-ci.sh:14`.
- Current integration tests reference old artifact paths/package hints, e.g. `tests/integration/tests/boot.rs:31`, `tests/integration/tests/aarch64-boot.rs:31`, `tests/integration/tests/handoff.rs:41-76`, and `tests/integration/tests/x86_64-boot.rs:44`.
- Grep inventory for active scope returned `vicell-kernel` matches across `.github/`, root run/build scripts, `scripts/`, `tests/integration/`, `limine*.conf`, `kernel/Cargo.toml`, and current docs under `docs/baremetal/`, `docs/specs/10-testing.md`, `docs/vf2-bringup.md`, and `docs/pioneer-bringup.md`.

## Preserve verified

- `__ViCell_*` linker sections remain ABI symbols, e.g. `cells/demos/hello-cell/hello-cell.ld:35-44`.
- `ViCell_syscall_dispatch` is an inter-arch call boundary, e.g. `hal/arch/arm/src/aarch64/trap.rs:41` and `kernel/src/task/syscall.rs:5228`.
- `ViResult`/`ViError` are conventionally retained by Cellos standards; `docs/code-standards.md` requires `Vi` prefixes for public traits/errors.
- Excluded from rename scope: `docs/project-changelog.md`, build error logs under `build/`, binary/generated artifacts, `target/`, disk/protocol magic, and old artifacts.

## Dirty worktree note

Existing dirty files include AArch64/HAL/kernel/input changes and generated `target\rpi3-cells/`. This plan must preserve them and only touch files listed in the phase.
