# Diag: RISC-V `boot` suite, `Init: shell spawn failed`

Date: 2026-11-04. HEAD `28dee8b3d`. Read-only on the main tree; builds and runs in a private worktree at HEAD, since removed.

## Verdict

**Stale-artifact hypothesis CONFIRMED.** The artifact is the *tracked* `kernel/src/embedded/init`: the Sep 19 init (`f73596611`, sha256 `5ec67c00…`, 43 664 B). It launches `/bin/shell`. The kernel `init` launch profile has had no such row since `8fe158478` (Oct 29). Any RV64 kernel built without `EMBEDDED_OVERRIDE` embeds this init and stops at `DENY launch edge … target=/bin/shell` → `Init: shell spawn failed.` This is the RV64 copy of the aarch64 bug fixed in CHANGELOG.md:64. The aarch64 copy was refreshed. The RV64 copy was not.

Fresh artifacts alone do not make the two tests green. Two more independent defects sit behind the stale init (findings 2 and 3).

## Findings

[CAUSE] kernel/src/embedded/init (tracked, last commit `f73596611` 2026-09-19): stale init that launches `/bin/shell`. A plain `cargo build -p cellos-kernel --target riscv64gc…` embeds it through kernel/build.rs:181-184, where `src/embedded-riscv64` is absent and the build falls back to `src/embedded`.
[EVIDENCE] [EXPANDED] Main tree, captured before 11:23: `target/riscv64gc-unknown-none-elf/release/cellos-kernel` (10:12:33) contains the committed init bytes at offset 1264564 (`k.find(init)`). `grep -c "shell spawn failed"` is 1 in that kernel and 0 in `app-init`, `disk_v3.img` and `kernel_fs.img`. Its build-script output (`build/cellos-kernel-4962c49582b9745d/output`) reads `rerun-if-changed=src/embedded/init`, and `out/embedded/init` is 43 664 B (10:12:00).
`gen-disk-ci.sh` built the good kernel at about 10:10 with `EMBEDDED_OVERRIDE=target/ViCell_embedded`, where `init` is the fresh `app-init`, sha256 `7d92d0e2…`, 609 040 B. A later plain kernel build at 10:12 then replaced it with one that embeds the committed init.
Worktree repro (fresh disk + `kernel_fs.img`, committed init, plain kernel build): both tests FAIL. Serial: `[loader] DENY launch edge: caller=2 name=init route=Path target=/bin/shell` / `USER: Init: shell spawn failed.`
[FIX] Refresh `kernel/src/embedded/init` with the current signed `app-init`, as was done for aarch64 (CHANGELOG.md:64). Guardrail: make `scripts/gen-disk-ci.sh` also `cp "$REL/app-init" kernel/src/embedded/init`, as gen_disk.ps1:352 does, and delete the false claim at gen-disk-ci.sh:506-508 ("the embedded bytes are identical either way"). Owner: RV64 build scripts, outside the x86 lane; no plan lock holds these files.

[CAUSE] gen_disk.ps1:676-680 (the CI recipe for the boot-suite job, ci.yml:2028): `mkfat32.py` is called without `--config-features input`. The generated plan therefore omits `/bin/input` (scripts/generate-boot-config.py:47-48 selects it only when `"input" in features`). Since Cellbox (`5043e2cf5`/`a8cfaba45`, Oct 29), the serial shell binds the INPUT service: libs/terminal/src/session.rs:200 `service_binding(INPUT)?`. That returns `NotFound` when the service is absent (libs/ostd/src/session.rs:446).
[EVIDENCE] Fresh worktree build through `pwsh ./gen_disk.ps1` with the CI env: the generated `boot-config/services.toml` holds only `virtio-net, e1000, net`. Serial: `SpawnFromElf: /bin/cellbox` → `shell: starting` → `shell: authenticated serial session failed: NotFound` → task exits 125 → no `Cellos >`. Both tests FAIL with a 40 s prompt timeout. `scripts/gen-disk-ci.sh:486` passes `--config-features "${CELLOS_INIT_FEATURES:-input}"`. Its plan holds `input`, and with that build the prompt is reached (see below).
[FIX] gen_disk.ps1:676: add `'--config-features', ($env:CELLOS_INIT_FEATURES ?? 'input')`, mirroring gen-disk-ci.sh:486. Owner: RV64 disk build (gen_disk.ps1), CI boot-suite job.

[CAUSE] tests/integration/tests/boot.rs:125 asserts `"Shell Started"`, a banner that went away with `cells/tools/shell` in `5043e2cf5` (Oct 29). Cellbox prints `shell: starting` (cells/tools/cellbox/src/main.rs:208).
[EVIDENCE] `git grep "Shell Started"` matches only boot.rs:125. `git log -S'Shell Started' -- cells` last hit is `5043e2cf5`. With fresh `gen-disk-ci.sh` artifacts: `boots_to_shell_prompt` panics at boot.rs:125 `shell did not spawn`, after `Cellos >` and `VFS Service` both matched.
[FIX] boot.rs:125: assert on `shell: starting`, or drop the assertion, since `Cellos >` already proves the shell ran. Owner: tests/integration boot suite.

[CAUSE] Secondary: at HEAD the CI recipe stops before any image is built. `24d75559a` (virtio-pci cells) added `unsafe` files with no allowlist entries, so `cellos-sign` F1 refuses.
[EVIDENCE] First `pwsh ./gen_disk.ps1` run in the worktree: `FAIL: F1 … [attribute] driver-virtio-scsi :: …/main.rs — crate root lacks #![forbid(unsafe_code)]`, plus 8 `[token]` files (virtio-blk `hal_x86.rs`/`io_x86.rs`/`mmio.rs`, virtio-net `mmio.rs`/`pci_x86/{hal,raw_net}.rs`, virtio-scsi `config.rs`/`dma.rs`) → `REFUSED: not signing`, exit 1. The main tree's uncommitted `scripts/unsafe-allowlist.toml` diff covers only mlibc-posix-x86.
[FIX] Add reviewed `[[file]]` entries for those 8 files and a `[[crate]] driver-virtio-scsi` entry to `scripts/unsafe-allowlist.toml`. Owner: phase 05 (virtio-PCI cells) of this plan. Workaround used here (worktree only, discarded): placeholder allowlist entries so the build could finish.

## Artifacts (main tree, observed 10:09-10:12, before another session's 11:23 rebuild)

| Path | mtime | Source / note | String |
|---|---|---|---|
| `target/riscv64gc-unknown-none-elf/release/cellos-kernel` | 11-04 10:12:33 | built from `src/embedded/init` (stale) | 1 |
| `kernel/src/embedded/init` (tracked) | 09-27 checkout, commit 09-19 | Sep 19 init, sha `5ec67c00` | 1 |
| `target/.../release/app-init` | 11-04 10:09:33 | HEAD `cells/tools/init`, sha `7d92d0e2` | 0 |
| `target/ViCell_embedded/init` | 11-04 10:09:34 | copy of `app-init` (gen-disk-ci.sh:511) | 0 |
| `kernel/src/embedded/kernel_fs.img` | 11-04 10:10:05 | gen-disk-ci.sh:539, holds `/bin/init` 609 040 B + `/bin/cellbox` | 0 |
| `disk_v3.img` | 11-04 10:10:35 | gen-disk-ci.sh | 0 |

The suite reads `target/riscv64gc-unknown-none-elf/release/cellos-kernel` and `disk_v3.img` from the repo root (boot.rs:40-59). It boots whichever kernel was built there last.
Tracked init blobs that still carry the string: `kernel/src/embedded/init`, `embedded-test-hooks/init`, `embedded-native-workload/init`, `embedded-hv-x86/init` (`git grep`). The string is absent from all source under `cells/tools/init`.

## Fresh-build test results (private worktree at HEAD)

| Build | Embedded init | Boot plan | `boots_to_shell_prompt` | `vfs_fat16_reboot_persistence` |
|---|---|---|---|---|
| `pwsh ./gen_disk.ps1` (CI recipe; F1 workaround) | fresh (ps1:352) | no `input` | FAIL: `serial session failed: NotFound` | FAIL: prompt timeout |
| `bash scripts/gen-disk-ci.sh` | fresh (override) | `input` | FAIL: boot.rs:125 `Shell Started` | **PASS** |
| gen-disk-ci.sh images + committed init, plain kernel build | stale | `input` | FAIL: `Init: shell spawn failed.` | FAIL: same |

Command: `cargo test --target x86_64-unknown-linux-gnu --test boot -- --test-threads=1 --exact boots_to_shell_prompt vfs_fat16_reboot_persistence`. Logs, outside the repo: `/tmp/diagrv-{gen-disk,gen-disk2,gen-disk-ci,fresh-tests,ci-sh-tests,stale-tests}.log`.

## Timeline

- 09-19 `f73596611`: last refresh of the RV64 `kernel/src/embedded/init`.
- 09-28 `9e83f0bb5`: gen-disk-ci.sh uses `EMBEDDED_OVERRIDE` and leaves the tracked init alone, on the false premise that the bytes are identical.
- 10-09 `cb1d63671`: the generated boot plan gates `/bin/input` behind the `input` packaging feature.
- 10-29 `8fe158478` (init launch profile: `/bin/cellbox` only), `5043e2cf5` (Cellbox replaces shell, banner gone), `a8cfaba45` (serial session needs INPUT).
- 11-04 10:12: plain RV64 kernel build in the main tree embeds the committed init. Phase 05 then records the two failures.
- 11-04 10:37 `24d75559a`: virtio-pci cells without allowlist entries break F1.

## Recurrence gap

- Guardrail: kernel/build.rs could refuse to embed an `init` whose bytes differ from `target/<arch>/release/app-init` when that file exists and is newer. Alternatively, stop tracking `kernel/src/embedded/init` and have every RV64 builder stage it. [INFERENCE] Either would have caught this and the aarch64 case.
- Detection: boot.rs prints the output dump only on prompt timeout. An early fail on `Init: shell spawn failed` / `DENY launch edge` would name the cause right away.

## Unresolved

- Whether CI's boot-suite job has ever run green since Oct 29: recent `ci.yml` runs (`gh run list`) fail at about 5 min, before that job.
- Since 11:23 the main-tree `cellos-kernel` (11.7 MB, string count 0) comes from another session's ramdisk lane. Main-tree results keep depending on which lane built last.
